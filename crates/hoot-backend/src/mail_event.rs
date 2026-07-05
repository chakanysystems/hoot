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

        for pubkey in &self.bcc {
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

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::nips::nip59::UnwrappedGift;
    use nostr::UnsignedEvent;

    fn unwrap_rumor(recipient_keys: &Keys, gift_wrap: &Event) -> UnsignedEvent {
        let unwrapped = UnwrappedGift::from_gift_wrap(recipient_keys, gift_wrap)
            .block_on()
            .expect("gift wrap should unwrap with the recipient's keys");

        assert_eq!(
            unwrapped.sender, unwrapped.rumor.pubkey,
            "seal signer should be the rumor author"
        );
        unwrapped.rumor
    }

    fn tags_named(rumor: &UnsignedEvent, name: &str) -> Vec<Vec<String>> {
        rumor
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(|part| part.as_str()) == Some(name))
            .map(|tag| tag.as_slice().to_vec())
            .collect()
    }

    fn assert_mail_rumor(
        rumor: &UnsignedEvent,
        sending_keys: &Keys,
        to_key: PublicKey,
        cc_key: PublicKey,
        parent_events: &[EventId],
    ) {
        assert_eq!(rumor.pubkey, sending_keys.public_key());
        assert_eq!(rumor.kind, Kind::Custom(MAIL_EVENT_KIND));
        assert_eq!(rumor.content, "workflow body");

        assert_eq!(
            tags_named(rumor, "subject"),
            vec![vec!["subject".to_string(), "Workflow subject".to_string()]]
        );
        assert_eq!(
            tags_named(rumor, "p"),
            vec![
                vec!["p".to_string(), to_key.to_hex()],
                vec!["p".to_string(), cc_key.to_hex(), "cc".to_string()],
            ]
        );
        assert_eq!(
            tags_named(rumor, "e"),
            parent_events
                .iter()
                .map(|event_id| vec!["e".to_string(), event_id.to_hex()])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            tags_named(rumor, "nip05"),
            vec![vec!["nip05".to_string(), "sender@example.com".to_string()]]
        );
    }

    #[test]
    fn try_to_events_wraps_same_rumor_for_each_to_and_cc_recipient() {
        let sending_keys = Keys::generate();
        let to_keys = Keys::generate();
        let cc_keys = Keys::generate();
        let parent_events = vec![
            EventId::from_hex("1111111111111111111111111111111111111111111111111111111111111111")
                .expect("valid parent id"),
            EventId::from_hex("2222222222222222222222222222222222222222222222222222222222222222")
                .expect("valid parent id"),
        ];
        let mut message = MailMessage {
            id: None,
            created_at: None,
            author: None,
            to: vec![to_keys.public_key()],
            cc: vec![cc_keys.public_key()],
            bcc: Vec::new(),
            parent_events: Some(parent_events.clone()),
            subject: "Workflow subject".to_string(),
            content: "workflow body".to_string(),
            sender_nip05: Some("sender@example.com".to_string()),
        };

        let events = message
            .try_to_events(&sending_keys)
            .expect("mail should wrap for valid recipients");

        assert_eq!(events.len(), 2);
        for (recipient_keys, recipient_pubkey) in [
            (&to_keys, to_keys.public_key()),
            (&cc_keys, cc_keys.public_key()),
        ] {
            let gift_wrap = events
                .get(&recipient_pubkey)
                .expect("recipient should receive a gift wrap");
            assert_eq!(gift_wrap.kind, Kind::GiftWrap);

            let rumor = unwrap_rumor(recipient_keys, gift_wrap);
            assert_mail_rumor(
                &rumor,
                &sending_keys,
                to_keys.public_key(),
                cc_keys.public_key(),
                &parent_events,
            );
        }
    }

    #[test]
    fn try_to_events_returns_empty_map_for_empty_recipient_lists() {
        let sending_keys = Keys::generate();
        let mut message = MailMessage {
            id: None,
            created_at: None,
            author: None,
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            parent_events: None,
            subject: "No recipients".to_string(),
            content: "nothing to send".to_string(),
            sender_nip05: None,
        };

        let events = message
            .try_to_events(&sending_keys)
            .expect("empty recipients should not be an error");

        assert!(events.is_empty());
    }

    #[test]
    fn try_to_events_keeps_single_map_entry_for_duplicate_recipient_key() {
        let sending_keys = Keys::generate();
        let recipient_keys = Keys::generate();
        let recipient_pubkey = recipient_keys.public_key();
        let mut message = MailMessage {
            id: None,
            created_at: None,
            author: None,
            to: vec![recipient_pubkey],
            cc: vec![recipient_pubkey],
            bcc: Vec::new(),
            parent_events: None,
            subject: "Duplicate recipient".to_string(),
            content: "duplicate body".to_string(),
            sender_nip05: None,
        };

        let events = message
            .try_to_events(&sending_keys)
            .expect("duplicate recipient should not panic or fail");

        assert_eq!(events.len(), 1);
        let gift_wrap = events
            .get(&recipient_pubkey)
            .expect("duplicate recipient should leave one final map entry");
        let rumor = unwrap_rumor(&recipient_keys, gift_wrap);
        assert_eq!(
            tags_named(&rumor, "p"),
            vec![
                vec!["p".to_string(), recipient_pubkey.to_hex()],
                vec!["p".to_string(), recipient_pubkey.to_hex(), "cc".to_string()],
            ]
        );
    }
    #[test]
    fn try_to_events_sends_to_bcc_without_exposing_bcc_pubkeys_in_rumor_tags() {
        let sending_keys = Keys::generate();
        let to_keys = Keys::generate();
        let bcc_keys = Keys::generate();
        let mut message = MailMessage {
            id: None,
            created_at: None,
            author: None,
            to: vec![to_keys.public_key()],
            cc: Vec::new(),
            bcc: vec![bcc_keys.public_key()],
            parent_events: None,
            subject: "Hidden recipient".to_string(),
            content: "bcc body".to_string(),
            sender_nip05: None,
        };

        let events = message
            .try_to_events(&sending_keys)
            .expect("bcc recipient should receive a gift wrap");

        assert_eq!(events.len(), 2);
        for (recipient_keys, recipient_pubkey) in [
            (&to_keys, to_keys.public_key()),
            (&bcc_keys, bcc_keys.public_key()),
        ] {
            let gift_wrap = events
                .get(&recipient_pubkey)
                .expect("each visible and hidden recipient should receive a gift wrap");
            let rumor = unwrap_rumor(recipient_keys, gift_wrap);
            assert_eq!(
                tags_named(&rumor, "p"),
                vec![vec!["p".to_string(), to_keys.public_key().to_hex()]],
                "bcc pubkeys must not be disclosed in the wrapped rumor tags"
            );
        }
    }
}
