use nostr::{Event, EventBuilder, EventId, Keys, Kind, PublicKey, Tag, TagKind, TagStandard};
use pollster::FutureExt as _;
use std::collections::HashMap;

pub const MAIL_EVENT_KIND: u16 = 2024;

// The provided MailMessage struct
pub struct MailMessage {
    pub id: Option<EventId>,
    pub created_at: Option<i64>,
    pub author: Option<PublicKey>,
    pub to: Vec<PublicKey>,
    pub cc: Vec<PublicKey>,
    pub bcc: Vec<PublicKey>,
    /// The events that this message references, used to keep track of threads.
    pub parent_events: Option<Vec<EventId>>,
    pub subject: String,
    pub content: String,
    /// Optional NIP-05 identifier for the sender
    pub sender_nip05: Option<String>,
}

impl MailMessage {
    pub fn to_events(&mut self, sending_keys: &Keys) -> HashMap<PublicKey, Event> {
        let mut pubkeys_to_send_to: Vec<PublicKey> = Vec::new();
        let mut tags: Vec<Tag> = Vec::new();

        for pubkey in &self.to {
            tags.push(Tag::public_key(*pubkey));
            pubkeys_to_send_to.push(*pubkey);
        }

        for pubkey in &self.cc {
            tags.push(Tag::custom(
                TagKind::p(),
                vec![pubkey.to_hex().as_str(), "cc"],
            ));
            pubkeys_to_send_to.push(*pubkey);
        }

        if let Some(parent_events) = &self.parent_events {
            for event in parent_events {
                tags.push(Tag::event(*event));
            }
        }

        // Add NIP-05 tag if sender has one
        if let Some(nip05) = &self.sender_nip05 {
            tags.push(Tag::custom(TagKind::custom("nip05"), vec![nip05.as_str()]));
        }

        tags.push(Tag::from_standardized(TagStandard::Subject(
            self.subject.clone(),
        )));

        let base_event = EventBuilder::new(Kind::Custom(MAIL_EVENT_KIND), &self.content).tags(tags);

        let mut event_list: HashMap<PublicKey, Event> = HashMap::new();
        for pubkey in pubkeys_to_send_to {
            // TODO: randomize gift wrap created_ats
            let wrapped_event =
                EventBuilder::gift_wrap(sending_keys, &pubkey, base_event.clone(), None)
                    .block_on()
                    .unwrap();
            event_list.insert(pubkey, wrapped_event);
        }

        event_list
    }

    pub fn try_to_events(
        &mut self,
        sending_keys: &Keys,
    ) -> Result<HashMap<PublicKey, Event>, String> {
        let mut pubkeys_to_send_to: Vec<PublicKey> = Vec::new();
        let mut tags: Vec<Tag> = Vec::new();

        for pubkey in &self.to {
            tags.push(Tag::public_key(*pubkey));
            pubkeys_to_send_to.push(*pubkey);
        }

        for pubkey in &self.cc {
            tags.push(Tag::custom(
                TagKind::p(),
                vec![pubkey.to_hex().as_str(), "cc"],
            ));
            pubkeys_to_send_to.push(*pubkey);
        }

        if let Some(parent_events) = &self.parent_events {
            for event in parent_events {
                tags.push(Tag::event(*event));
            }
        }

        if let Some(nip05) = &self.sender_nip05 {
            tags.push(Tag::custom(TagKind::custom("nip05"), vec![nip05.as_str()]));
        }

        tags.push(Tag::from_standardized(TagStandard::Subject(
            self.subject.clone(),
        )));

        let base_event = EventBuilder::new(Kind::Custom(MAIL_EVENT_KIND), &self.content).tags(tags);

        let mut event_list: HashMap<PublicKey, Event> = HashMap::new();
        for pubkey in pubkeys_to_send_to {
            let wrapped_event =
                EventBuilder::gift_wrap(sending_keys, &pubkey, base_event.clone(), None)
                    .block_on()
                    .map_err(|err| err.to_string())?;
            event_list.insert(pubkey, wrapped_event);
        }

        Ok(event_list)
    }
}
