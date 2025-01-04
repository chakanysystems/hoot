use crate::nostr::{Event, EventBuilder, EventKind, Tag};
use bitcoin::secp256k1::{PublicKey, Keypair};
use std::collections::HashMap;
use pollster::FutureExt as _;

pub const MAIL_EVENT_KIND: u32 = 1059;

pub struct MailMessage {
    pub to: Vec<PublicKey>,
    pub cc: Vec<PublicKey>,
    pub bcc: Vec<PublicKey>,
    pub subject: String,
    pub content: String,
}

impl MailMessage {
    pub fn to_events(&mut self, sending_keys: &Keypair) -> HashMap<PublicKey, Event> {
        let mut pubkeys_to_send_to: Vec<PublicKey> = Vec::new();
        let mut tags: Vec<Tag> = Vec::new();

        for pubkey in &self.to {
            tags.push(Tag::new_with_values(vec!["p".to_string(), *pubkey.to_hex().as_str()]));
            pubkeys_to_send_to.push(*pubkey);
        }

        for pubkey in &self.cc {
            tags.push(Tag::new_with_values(vec!["p".to_string(), *pubkey.to_hex().as_str(), "cc".to_string()]));
            pubkeys_to_send_to.push(*pubkey);
        }

        tags.push(Tag::new_with_values(vec!["subject".to_string(), self.subject.clone()]));

        let base_event = EventBuilder::new()
            .kind(EventKind::MailEvent)
            .content(&self.content)
            .tags(tags)
            .build();
        base_event.pubkey = sending_keys.clone().public_key();

        let mut event_list: HashMap<PublicKey, Event> = HashMap::new();
        for pubkey in pubkeys_to_send_to {
            let wrapped_event =
                EventBuilder::gift_wrap(sending_keys, &pubkey, base_event.clone(), None).block_on().unwrap();
            event_list.insert(pubkey, wrapped_event);
        }

        event_list
    }
}
