use crate::db;
use crate::db::sender_status::SenderStatus;
use crate::dto::*;
use crate::error::{HootError, HootResult};
use crate::mail_event::MailMessage;
use crate::relay::RelayStatus;
use nostr::{EventId, FromBech32, PublicKey, ToBech32};

pub(crate) fn database_error(value: anyhow::Error) -> HootError {
    HootError::Database {
        message: value.to_string(),
    }
}

pub(crate) fn keyring_error(value: anyhow::Error) -> HootError {
    HootError::Keyring {
        message: value.to_string(),
    }
}

pub(crate) fn nostr_error(value: impl std::fmt::Display) -> HootError {
    HootError::Nostr {
        message: value.to_string(),
    }
}

pub(crate) fn parse_public_key(pubkey: &str) -> HootResult<PublicKey> {
    match PublicKey::from_hex(pubkey) {
        Ok(parsed) => Ok(parsed),
        Err(hex_err) => PublicKey::from_bech32(pubkey).map_err(|bech32_err| HootError::Nostr {
            message: format!(
                "invalid public key `{}`: hex parse failed: {}; bech32 parse failed: {}",
                pubkey, hex_err, bech32_err
            ),
        }),
    }
}

pub(crate) fn parse_event_ids(event_ids: Vec<String>) -> HootResult<Vec<EventId>> {
    event_ids
        .into_iter()
        .map(|event_id| {
            EventId::parse(&event_id).map_err(|e| HootError::Nostr {
                message: format!("invalid event id `{}`: {}", event_id, e),
            })
        })
        .collect()
}

pub(crate) fn draft_dtos(drafts: Vec<db::Draft>) -> Vec<DraftDto> {
    drafts
        .into_iter()
        .map(|draft| DraftDto {
            id: draft.id,
            subject: draft.subject,
            to_field: draft.to_field,
            content: draft.content,
            parent_events: draft.parent_events,
            selected_account: draft.selected_account,
            selected_nip05: draft.selected_nip05,
            created_at: draft.created_at,
            updated_at: draft.updated_at,
        })
        .collect()
}

pub(crate) fn contact_dtos(
    contacts: Vec<(String, Option<String>, ProfileMetadata)>,
) -> HootResult<Vec<ContactDto>> {
    Ok(contacts
        .into_iter()
        .map(|(pubkey, petname, metadata)| ContactDto {
            pubkey,
            petname,
            metadata,
        })
        .collect())
}

pub(crate) fn mail_message_dto(message: MailMessage) -> MailMessageDto {
    MailMessageDto {
        id: message.id.map(|id| id.to_hex()),
        created_at: message.created_at,
        author_pubkey: message.author.map(|pubkey| pubkey.to_hex()),
        to_pubkeys: message
            .to
            .into_iter()
            .map(|pubkey| pubkey.to_hex())
            .collect(),
        cc_pubkeys: message
            .cc
            .into_iter()
            .map(|pubkey| pubkey.to_hex())
            .collect(),
        bcc_pubkeys: message
            .bcc
            .into_iter()
            .map(|pubkey| pubkey.to_hex())
            .collect(),
        parent_event_ids: message
            .parent_events
            .unwrap_or_default()
            .into_iter()
            .map(|event_id| event_id.to_hex())
            .collect(),
        subject: message.subject,
        content: message.content,
        sender_nip05: message.sender_nip05,
    }
}

pub(crate) fn sender_status(status: SenderStatusDto) -> SenderStatus {
    match status {
        SenderStatusDto::Allowed => SenderStatus::Allowed,
        SenderStatusDto::Junked => SenderStatus::Junked,
    }
}

pub(crate) fn sender_status_dto(status: SenderStatus) -> SenderStatusDto {
    match status {
        SenderStatus::Allowed => SenderStatusDto::Allowed,
        SenderStatus::Junked => SenderStatusDto::Junked,
    }
}

pub(crate) fn relay_connection_status(status: RelayStatus) -> RelayConnectionStatus {
    match status {
        RelayStatus::Connecting => RelayConnectionStatus::Connecting,
        RelayStatus::Connected => RelayConnectionStatus::Connected,
        RelayStatus::Disconnected => RelayConnectionStatus::Disconnected,
    }
}

pub(crate) fn dedup_events(events: Vec<BackendEvent>) -> Vec<BackendEvent> {
    let mut has_mailboxes = false;
    let mut has_relays = false;
    let mut has_nip05 = false;
    let mut has_accounts = false;
    for event in events {
        match event {
            BackendEvent::MailboxesChanged => has_mailboxes = true,
            BackendEvent::RelayStatusesChanged => has_relays = true,
            BackendEvent::NIp05ResultsChanged => has_nip05 = true,
            BackendEvent::AccountsChanged => has_accounts = true,
        }
    }
    let mut deduped = Vec::new();
    if has_mailboxes {
        deduped.push(BackendEvent::MailboxesChanged);
    }
    if has_relays {
        deduped.push(BackendEvent::RelayStatusesChanged);
    }
    if has_nip05 {
        deduped.push(BackendEvent::NIp05ResultsChanged);
    }
    if has_accounts {
        deduped.push(BackendEvent::AccountsChanged);
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{Keys, ToBech32};

    #[test]
    fn draft_contact_and_mail_message_dtos_preserve_backend_fields() -> HootResult<()> {
        let draft = db::Draft {
            id: 7,
            subject: "Subject".to_string(),
            to_field: "alice@example.com".to_string(),
            content: "Body".to_string(),
            parent_events: vec!["parent-a".to_string(), "parent-b".to_string()],
            selected_account: Some("account".to_string()),
            selected_nip05: Some("sender@example.com".to_string()),
            created_at: 10,
            updated_at: 20,
        };
        let drafts = draft_dtos(vec![draft]);
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, 7);
        assert_eq!(drafts[0].subject, "Subject");
        assert_eq!(drafts[0].to_field, "alice@example.com");
        assert_eq!(drafts[0].content, "Body");
        assert_eq!(drafts[0].parent_events, vec!["parent-a", "parent-b"]);
        assert_eq!(drafts[0].selected_account.as_deref(), Some("account"));
        assert_eq!(
            drafts[0].selected_nip05.as_deref(),
            Some("sender@example.com")
        );
        assert_eq!(drafts[0].created_at, 10);
        assert_eq!(drafts[0].updated_at, 20);

        let metadata = ProfileMetadata {
            name: Some("alice".to_string()),
            display_name: Some("Alice".to_string()),
            picture: Some("https://example.com/alice.png".to_string()),
            nip05: Some("alice@example.com".to_string()),
        };
        let contacts = contact_dtos(vec![(
            "pubkey".to_string(),
            Some("Pet".to_string()),
            metadata.clone(),
        )])?;
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].pubkey, "pubkey");
        assert_eq!(contacts[0].petname.as_deref(), Some("Pet"));
        assert_eq!(contacts[0].metadata, metadata);

        let author = Keys::generate();
        let recipient = Keys::generate();
        let parent =
            EventId::from_hex("1111111111111111111111111111111111111111111111111111111111111111")
                .map_err(nostr_error)?;
        let message = MailMessage {
            id: Some(parent),
            created_at: Some(123),
            author: Some(author.public_key()),
            to: vec![recipient.public_key()],
            cc: vec![author.public_key()],
            bcc: vec![recipient.public_key()],
            parent_events: Some(vec![parent]),
            subject: "Mail".to_string(),
            content: "Content".to_string(),
            sender_nip05: Some("sender@example.com".to_string()),
        };
        let dto = mail_message_dto(message);
        let parent_hex = parent.to_hex();
        let author_hex = author.public_key().to_hex();
        assert_eq!(dto.id.as_deref(), Some(parent_hex.as_str()));
        assert_eq!(dto.created_at, Some(123));
        assert_eq!(dto.author_pubkey.as_deref(), Some(author_hex.as_str()));
        assert_eq!(dto.to_pubkeys, vec![recipient.public_key().to_hex()]);
        assert_eq!(dto.cc_pubkeys, vec![author.public_key().to_hex()]);
        assert_eq!(dto.bcc_pubkeys, vec![recipient.public_key().to_hex()]);
        assert_eq!(dto.parent_event_ids, vec![parent.to_hex()]);
        assert_eq!(dto.subject, "Mail");
        assert_eq!(dto.content, "Content");
        assert_eq!(dto.sender_nip05.as_deref(), Some("sender@example.com"));

        Ok(())
    }

    #[test]
    fn parser_and_enum_mapping_helpers_accept_valid_values_and_reject_invalid_values(
    ) -> HootResult<()> {
        let keys = Keys::generate();
        let hex = keys.public_key().to_hex();
        let npub = keys.public_key().to_bech32().map_err(nostr_error)?;
        assert_eq!(parse_public_key(&hex)?, keys.public_key());
        assert_eq!(parse_public_key(&npub)?, keys.public_key());
        assert!(matches!(
            parse_public_key("not-a-key"),
            Err(HootError::Nostr { .. })
        ));

        let event_id =
            EventId::from_hex("2222222222222222222222222222222222222222222222222222222222222222")
                .map_err(nostr_error)?;
        assert_eq!(parse_event_ids(vec![event_id.to_hex()])?, vec![event_id]);
        assert!(matches!(
            parse_event_ids(vec!["not-an-event-id".to_string()]),
            Err(HootError::Nostr { .. })
        ));

        assert_eq!(
            sender_status(SenderStatusDto::Allowed),
            SenderStatus::Allowed
        );
        assert_eq!(sender_status(SenderStatusDto::Junked), SenderStatus::Junked);
        assert_eq!(
            sender_status_dto(SenderStatus::Allowed),
            SenderStatusDto::Allowed
        );
        assert_eq!(
            sender_status_dto(SenderStatus::Junked),
            SenderStatusDto::Junked
        );
        assert!(matches!(
            relay_connection_status(RelayStatus::Connecting),
            RelayConnectionStatus::Connecting
        ));
        assert!(matches!(
            relay_connection_status(RelayStatus::Connected),
            RelayConnectionStatus::Connected
        ));
        assert!(matches!(
            relay_connection_status(RelayStatus::Disconnected),
            RelayConnectionStatus::Disconnected
        ));
        Ok(())
    }

    #[test]
    fn dedup_events_returns_each_event_type_once_in_stable_order() {
        let events = dedup_events(vec![
            BackendEvent::AccountsChanged,
            BackendEvent::MailboxesChanged,
            BackendEvent::AccountsChanged,
            BackendEvent::NIp05ResultsChanged,
            BackendEvent::RelayStatusesChanged,
            BackendEvent::MailboxesChanged,
        ]);

        assert!(matches!(events[0], BackendEvent::MailboxesChanged));
        assert!(matches!(events[1], BackendEvent::RelayStatusesChanged));
        assert!(matches!(events[2], BackendEvent::NIp05ResultsChanged));
        assert!(matches!(events[3], BackendEvent::AccountsChanged));
        assert_eq!(events.len(), 4);
    }
}

/// Converts a hex pubkey to its bech32 npub encoding.
/// Returns None if the hex string is not a valid 32-byte pubkey.
pub fn npub_string(pubkey_hex: &str) -> Option<String> {
    PublicKey::from_hex(pubkey_hex)
        .ok()
        .and_then(|pk| pk.to_bech32().ok())
}
