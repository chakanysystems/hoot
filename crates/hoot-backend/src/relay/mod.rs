use crate::error::{Error, Result};
use ewebsock::{WsEvent, WsMessage};
use std::collections::HashSet;
use tracing::{debug, error, info};

mod pool;
pub use pool::RelayPool;

mod message;
pub use message::{ClientMessage, RelayMessage};

mod subscription;
pub use subscription::Subscription;

const DEFAULT_RELAY_URLS: [&str; 2] = ["wss://relay.chakany.systems", "wss://talon.quest"];

pub fn default_relay_urls() -> &'static [&'static str] {
    &DEFAULT_RELAY_URLS
}

#[derive(PartialEq, Clone, Copy)]
pub enum RelayStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Default)]
pub struct RelayAuthState {
    pub challenge: Option<String>,
    pub authenticated_keys: HashSet<String>,
}

pub struct Relay {
    pub url: String,
    reader: ewebsock::WsReceiver,
    writer: ewebsock::WsSender,
    pub status: RelayStatus,
    pub auth_state: RelayAuthState,
}

impl Relay {
    pub fn new_with_wakeup(
        url: impl Into<String>,
        wake_up: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        let new_url: String = url.into();
        let (sender, reciever) =
            ewebsock::connect_with_wakeup(new_url.clone(), ewebsock::Options::default(), wake_up)
                .map_err(|err| Error::Generic(format!("{:?}", err)))?;
        let relay = Self {
            url: new_url,
            reader: reciever,
            writer: sender,
            status: RelayStatus::Connecting,
            auth_state: RelayAuthState::default(),
        };

        Ok(relay)
    }

    // TODO: investigate whether this can cause a message to be dropped due to the writer being
    // overwritten
    pub fn reconnect(&mut self, wake_up: impl Fn() + Send + Sync + 'static) -> Result<()> {
        let (sender, reciever) =
            ewebsock::connect_with_wakeup(self.url.clone(), ewebsock::Options::default(), wake_up)
                .map_err(|err| Error::Generic(format!("{:?}", err)))?;

        self.reader = reciever;
        self.writer = sender;
        self.auth_state = RelayAuthState::default();
        Ok(())
    }

    pub fn send(&mut self, message: WsMessage) -> Result<()> {
        if self.status != RelayStatus::Connected {
            return Err(Error::RelayNotConnected);
        }
        debug!("sending message to {}: {:?}", self.url, message);

        self.writer.send(message);
        Ok(())
    }

    pub fn try_recv(&mut self) -> Option<WsEvent> {
        if let Some(event) = self.reader.try_recv() {
            use WsEvent::*;
            match &event {
                Message(_) => {}
                Opened => {
                    self.status = RelayStatus::Connected;
                }
                Error(error) => {
                    error!("error in websocket connection to {}: {}", self.url, error);
                    self.status = RelayStatus::Disconnected;
                }
                Closed => {
                    info!("connection to {} closed", self.url);
                    self.status = RelayStatus::Disconnected;
                }
            }

            return Some(event);
        }

        None
    }

    pub fn ping(&mut self) {
        let ping_msg = WsMessage::Ping(Vec::new());
        match self.send(ping_msg) {
            Ok(_) => {
                info!("Ping sent to {}", self.url);
                self.status = RelayStatus::Connected;
            }
            Err(e) => {
                error!("Error sending ping to {}: {:?}", self.url, e);
                self.status = RelayStatus::Disconnected;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_bootstrap_policy_returns_product_relays_in_stable_order() {
        assert_eq!(
            default_relay_urls(),
            &["wss://relay.chakany.systems", "wss://talon.quest"]
        );
    }

    #[test]
    fn relay_auth_state_defaults_to_no_challenge_and_no_authenticated_keys() {
        let state = RelayAuthState::default();

        assert_eq!(state.challenge, None);
        assert!(state.authenticated_keys.is_empty());
    }
}
