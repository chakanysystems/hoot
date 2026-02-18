use crate::error::Result;
use crate::relay::message::ClientMessage;
use crate::relay::Subscription;
use crate::relay::{Relay, RelayStatus};
use ewebsock::{WsEvent, WsMessage};
use nostr::Event;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, error};

pub const RELAY_RECONNECT_SECONDS: u64 = 5;

pub struct RelayPool {
    pub relays: HashMap<String, Relay>,
    pub subscriptions: HashMap<String, Subscription>,
    last_reconnect_attempt: Instant,
    last_ping: Instant,
    // Track subscriptions that need auth retry per relay
    pending_auth_subscriptions: HashMap<String, Vec<String>>, // relay_url -> list of subscription IDs
}

impl RelayPool {
    pub fn new() -> Self {
        Self {
            relays: HashMap::new(),
            subscriptions: HashMap::new(),
            last_reconnect_attempt: Instant::now(),
            last_ping: Instant::now(),
            pending_auth_subscriptions: HashMap::new(),
        }
    }

    pub fn get_last_reconnect_attempt(&mut self) -> Instant {
        return self.last_reconnect_attempt;
    }

    pub fn keepalive(&mut self, wake_up: impl Fn() + Send + Sync + Clone + 'static) {
        let now = Instant::now();

        // Check disconnected relays
        if now.duration_since(self.last_reconnect_attempt)
            >= Duration::from_secs(RELAY_RECONNECT_SECONDS)
        {
            for relay in self.relays.values_mut() {
                if relay.status != RelayStatus::Connected {
                    relay.status = RelayStatus::Connecting;
                    relay.reconnect(wake_up.clone());
                }
            }
            self.last_reconnect_attempt = now;
        }

        // Ping connected relays
        if now.duration_since(self.last_ping) >= Duration::from_secs(30) {
            for relay in self.relays.values_mut() {
                if relay.status == RelayStatus::Connected {
                    relay.ping();
                }
            }
            self.last_ping = now;
        }
    }

    pub fn add_subscription(&mut self, sub: Subscription) -> Result<()> {
        {
            let cloned_sub = sub.clone();
            self.subscriptions.insert(cloned_sub.id.clone(), cloned_sub);
        }

        let client_message = ClientMessage::Req {
            subscription_id: sub.id,
            filters: sub.filters,
        };

        let payload = serde_json::to_string(&client_message)?;
        self.send(ewebsock::WsMessage::Text(payload))?;

        Ok(())
    }

    pub fn add_url(
        &mut self,
        url: String,
        wake_up: impl Fn() + Send + Sync + 'static,
    ) -> Result<()> {
        let relay = Relay::new_with_wakeup(url.clone(), wake_up);
        self.relays.insert(url, relay);

        Ok(())
    }

    pub fn remove_url(&mut self, url: &str) -> Option<Relay> {
        self.relays.remove(url)
    }

    pub fn try_recv(&mut self) -> Option<(String, String)> {
        let relay_urls: Vec<String> = self.relays.keys().cloned().collect();
        for relay_url in relay_urls {
            if let Some(relay) = self.relays.get_mut(&relay_url) {
                if let Some(event) = relay.try_recv() {
                    use WsEvent::*;
                    match event {
                        Message(message) => {
                            if let Some(msg_text) = self.handle_message(relay_url.clone(), message)
                            {
                                return Some((relay_url, msg_text));
                            }
                        }
                        Opened => {
                            for sub in self.subscriptions.clone() {
                                let client_message = ClientMessage::Req {
                                    subscription_id: sub.1.id,
                                    filters: sub.1.filters,
                                };

                                let payload = match serde_json::to_string(&client_message) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        error!("could not turn subscription into json: {}", e);
                                        continue;
                                    }
                                };

                                match relay.send(ewebsock::WsMessage::Text(payload)) {
                                    Ok(_) => (),
                                    Err(e) => {
                                        error!(
                                            "could not send subscription to {}: {:?}",
                                            relay.url, e
                                        )
                                    }
                                };
                            }
                        }
                        _ => {
                            // we only want to know when the connection opens
                        }
                    }
                }
            }
        }
        None
    }

    fn handle_message(&mut self, url: String, message: WsMessage) -> Option<String> {
        use WsMessage::*;
        match message {
            Text(txt) => {
                return Some(txt);
            }
            Binary(..) => {
                error!("recived binary messsage, your move semisol");
            }
            Ping(m) => {
                let pong_msg = WsMessage::Pong(m);
                match self.send(pong_msg) {
                    Ok(_) => {}
                    Err(e) => error!("error when sending websocket message {:?}", e),
                }
            }
            Pong(m) => {
                debug!(
                    "pong recieved from {} after approx {} seconds",
                    &url,
                    self.last_ping.elapsed().as_secs()
                );
            }
            _ => {
                // who cares
            }
        }

        None
    }

    pub fn send(&mut self, message: ewebsock::WsMessage) -> Result<()> {
        for relay in self.relays.values_mut() {
            if relay.status == RelayStatus::Connected {
                relay.send(message.clone())?;
            }
        }
        Ok(())
    }

    pub fn ping_all(&mut self) -> Result<()> {
        for relay in self.relays.values_mut() {
            relay.ping();
        }
        Ok(())
    }

    pub fn send_auth(&mut self, relay_url: &str, event: Event) -> Result<()> {
        if let Some(relay) = self.relays.get_mut(relay_url) {
            let client_message = ClientMessage::Auth { event };
            let payload = serde_json::to_string(&client_message)?;
            relay.send(ewebsock::WsMessage::Text(payload))?;
        }
        Ok(())
    }

    pub fn get_challenge(&self, relay_url: &str) -> Option<String> {
        self.relays
            .get(relay_url)
            .and_then(|relay| relay.auth_state.challenge.clone())
    }

    pub fn add_authenticated_key(&mut self, relay_url: &str, pubkey: String) {
        if let Some(relay) = self.relays.get_mut(relay_url) {
            relay.auth_state.authenticated_keys.insert(pubkey);
        }
    }

    pub fn is_key_authenticated(&self, relay_url: &str, pubkey: &str) -> bool {
        self.relays
            .get(relay_url)
            .map(|relay| relay.auth_state.authenticated_keys.contains(pubkey))
            .unwrap_or(false)
    }

    /// Track a subscription that failed due to auth-required so we can retry after auth
    pub fn track_pending_auth_subscription(&mut self, relay_url: &str, subscription_id: &str) {
        self.pending_auth_subscriptions
            .entry(relay_url.to_string())
            .or_default()
            .push(subscription_id.to_string());
    }

    /// Get and clear pending subscriptions for a relay (call after successful auth)
    pub fn take_pending_auth_subscriptions(&mut self, relay_url: &str) -> Vec<String> {
        self.pending_auth_subscriptions
            .remove(relay_url)
            .unwrap_or_default()
    }

    /// Send a specific subscription to a specific relay
    pub fn send_subscription_to_relay(&mut self, relay_url: &str, sub_id: &str) -> Result<()> {
        if let Some(sub) = self.subscriptions.get(sub_id) {
            if let Some(relay) = self.relays.get_mut(relay_url) {
                if relay.status == RelayStatus::Connected {
                    let client_message = ClientMessage::Req {
                        subscription_id: sub_id.to_string(),
                        filters: sub.filters.clone(),
                    };
                    let payload = serde_json::to_string(&client_message)?;
                    relay.send(ewebsock::WsMessage::Text(payload))?;
                }
            }
        }
        Ok(())
    }
}
