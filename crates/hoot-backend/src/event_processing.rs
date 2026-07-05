use crate::account_manager::AccountManager;
use crate::backend::BackendInner;
use crate::conversions::{database_error, nostr_error};
use crate::dto::ProfileMetadata;
use crate::error::{HootError, HootResult};
use crate::mail_event::MAIL_EVENT_KIND;
use crate::profile_metadata::ProfileOption;
use crate::relay;
use nostr::{Event, Kind, TagKind};
use std::collections::HashSet;
use tracing::{debug, warn};

pub(crate) fn process_verification_queue(inner: &mut BackendInner) -> bool {
    inner.nip05_verifier.process_queue(&inner.db)
}

pub(crate) fn process_resolution_queue(inner: &mut BackendInner) -> bool {
    inner.nip05_resolver.process_queue()
}

pub(crate) fn process_message(
    inner: &mut BackendInner,
    relay_url: &str,
    message: &relay::RelayMessage<'_>,
) -> HootResult<bool> {
    match message {
        relay::RelayMessage::Event(subscription_id, event_json) => {
            process_event(inner, subscription_id, event_json)
        }
        relay::RelayMessage::Notice(message) => {
            debug!("Relay notice: {}", message);
            Ok(false)
        }
        relay::RelayMessage::OK(result) => {
            debug!("Command result: {:?}", result);
            if result.message.starts_with("auth-required:") {
                perform_auth(inner, relay_url)?;
            } else if result.status {
                let pending = inner.relays.take_pending_auth_subscriptions(relay_url);
                for subscription_id in pending {
                    inner
                        .relays
                        .send_subscription_to_relay(relay_url, &subscription_id)
                        .map_err(HootError::from)?;
                }
            }
            Ok(false)
        }
        relay::RelayMessage::Eose(subscription_id) => {
            debug!("End of stored events for subscription {}", subscription_id);
            Ok(false)
        }
        relay::RelayMessage::Closed(subscription_id, message) => {
            debug!("Subscription {} closed: {}", subscription_id, message);
            if message.starts_with("auth-required:") {
                inner
                    .relays
                    .track_pending_auth_subscription(relay_url, subscription_id);
                perform_auth(inner, relay_url)?;
            }
            Ok(false)
        }
        relay::RelayMessage::Auth(challenge) => {
            if let Some(relay) = inner.relays.relays.get_mut(relay_url) {
                relay.auth_state.challenge = Some((*challenge).to_string());
            }
            perform_auth(inner, relay_url)?;
            Ok(false)
        }
    }
}

pub(crate) fn perform_auth(inner: &mut BackendInner, relay_url: &str) -> HootResult<()> {
    let challenge = match inner.relays.get_challenge(relay_url) {
        Some(challenge) => challenge,
        None => {
            warn!("No challenge available for relay {}", relay_url);
            return Ok(());
        }
    };

    for keys in &inner.account_manager.loaded_keys {
        let pubkey = keys.public_key().to_hex();
        if inner.relays.is_key_authenticated(relay_url, &pubkey) {
            continue;
        }
        let event =
            AccountManager::create_auth_event(keys, relay_url, &challenge).map_err(nostr_error)?;
        inner
            .relays
            .send_auth(relay_url, event)
            .map_err(HootError::from)?;
        inner.relays.add_authenticated_key(relay_url, pubkey);
    }
    Ok(())
}

pub(crate) fn process_event(
    inner: &mut BackendInner,
    _subscription_id: &str,
    event_json: &str,
) -> HootResult<bool> {
    let event = match serde_json::from_str::<Event>(event_json) {
        Ok(event) => event,
        Err(err) => {
            warn!("Failed to parse event JSON: {}", err);
            return Ok(false);
        }
    };

    if let Err(err) = event.verify() {
        warn!("Event verification failed for event {}: {}", event.id, err);
        return Ok(false);
    }

    if event.kind == Kind::EventDeletion {
        let event_ids: Vec<String> = event.tags.event_ids().map(|id| id.to_hex()).collect();
        if event_ids.is_empty() {
            return Ok(false);
        }
        let author_pubkey = event.pubkey.to_string();
        let deletion_id = event.id.to_string();
        return apply_deletions(
            inner,
            event_ids,
            Some(author_pubkey.as_str()),
            Some(deletion_id.as_str()),
        );
    }

    let event_id = event.id.to_string();
    let event_author = event.pubkey.to_string();
    if inner
        .db
        .is_deleted(&event_id, Some(event_author.as_str()))
        .map_err(database_error)?
    {
        return Ok(false);
    }
    if inner.db.is_trashed(&event_id).map_err(database_error)? {
        return Ok(false);
    }

    if event.kind == Kind::Metadata {
        let metadata: ProfileMetadata = match serde_json::from_str(&event.content) {
            Ok(metadata) => metadata,
            Err(err) => {
                warn!("Invalid metadata event {}: {}", event.id, err);
                return Ok(false);
            }
        };
        if let Some(nip05) = &metadata.nip05 {
            cache_and_verify_nip05(inner, nip05, &event.pubkey.to_string(), false)?;
        }
        inner
            .profile_metadata
            .insert(event.pubkey.to_string(), ProfileOption::Some(metadata));
        inner
            .db
            .update_profile_metadata(event)
            .map_err(database_error)?;
        return Ok(true);
    }

    if event.kind == Kind::GiftWrap {
        if inner
            .db
            .gift_wrap_exists(&event.id.to_string())
            .map_err(database_error)?
        {
            return Ok(false);
        }
        if inner
            .db
            .is_deleted(&event.id.to_string(), None)
            .map_err(database_error)?
        {
            return Ok(false);
        }

        let unwrapped = match inner.account_manager.unwrap_gift_wrap(&event) {
            Ok(unwrapped) => unwrapped,
            Err(err) => {
                warn!("Failed to unwrap gift wrap {}: {}", event.id, err);
                return Ok(false);
            }
        };
        if unwrapped.sender != unwrapped.rumor.pubkey {
            warn!("Gift wrap seal signer mismatch for event {}", event.id);
            return Ok(false);
        }

        let mut rumor = unwrapped.rumor.clone();
        rumor.ensure_id();
        if let Err(err) = rumor.verify_id() {
            warn!("Invalid rumor id for gift wrap {}: {}", event.id, err);
            return Ok(false);
        }
        let rumor_id = match rumor.id.as_ref() {
            Some(id) => id.to_hex(),
            None => {
                warn!("Gift wrap rumor has no id for event {}", event.id);
                return Ok(false);
            }
        };
        let author_pubkey = rumor.pubkey.to_string();
        if inner
            .db
            .is_deleted(&rumor_id, Some(author_pubkey.as_str()))
            .map_err(database_error)?
        {
            inner
                .db
                .record_deletion_markers(&[event.id.to_string()], None)
                .map_err(database_error)?;
            return Ok(false);
        }

        let recipient = event
            .tags
            .find(TagKind::p())
            .and_then(|tag| tag.content())
            .map(|value| value.to_string());

        if inner.db.is_trashed(&rumor_id).map_err(database_error)? {
            inner
                .db
                .save_gift_wrap_map(
                    &event.id.to_string(),
                    &rumor_id,
                    recipient.as_deref(),
                    event.created_at.as_u64() as i64,
                )
                .map_err(database_error)?;
            return Ok(false);
        }

        inner
            .db
            .store_event(&event, Some(&unwrapped), recipient.as_deref())
            .map_err(database_error)?;
        inner.events.push(event);

        if rumor.kind == Kind::Custom(MAIL_EVENT_KIND) {
            if let Some(nip05_tag) = rumor.tags.find(TagKind::custom("nip05")) {
                if let Some(nip05_value) = nip05_tag.content() {
                    cache_and_verify_nip05(inner, nip05_value, &rumor.pubkey.to_string(), false)?;
                }
            }
        }
        return Ok(true);
    }

    if inner
        .db
        .has_event(&event.id.to_string())
        .map_err(database_error)?
    {
        return Ok(false);
    }
    inner
        .db
        .store_event(&event, None, None)
        .map_err(database_error)?;
    inner.events.push(event);
    Ok(true)
}

pub(crate) fn apply_deletions(
    inner: &mut BackendInner,
    event_ids: Vec<String>,
    author_pubkey: Option<&str>,
    source_event_id: Option<&str>,
) -> HootResult<bool> {
    if event_ids.is_empty() {
        return Ok(false);
    }

    let mut scoped_event_ids = Vec::new();
    let mut unscoped_event_ids = Vec::new();
    for event_id in event_ids {
        if let Some((kind, pubkey)) = inner
            .db
            .get_event_kind_pubkey(&event_id)
            .map_err(database_error)?
        {
            let is_gift_wrap = kind == i64::from(Kind::GiftWrap.as_u16());
            let is_mail = kind == i64::from(MAIL_EVENT_KIND);
            if is_gift_wrap {
                continue;
            }
            if is_mail {
                if let Some(author) = author_pubkey {
                    if author == pubkey {
                        scoped_event_ids.push(event_id);
                    }
                } else {
                    unscoped_event_ids.push(event_id);
                }
            } else {
                unscoped_event_ids.push(event_id);
            }
        }
    }

    let mut apply_event_ids = Vec::new();
    apply_event_ids.extend(scoped_event_ids.iter().cloned());
    apply_event_ids.extend(unscoped_event_ids.iter().cloned());
    if apply_event_ids.is_empty() {
        return Ok(false);
    }

    if !scoped_event_ids.is_empty() {
        inner
            .db
            .record_deletions(&scoped_event_ids, author_pubkey, source_event_id)
            .map_err(database_error)?;
    }
    if !unscoped_event_ids.is_empty() {
        inner
            .db
            .record_deletions(&unscoped_event_ids, None, source_event_id)
            .map_err(database_error)?;
    }
    inner
        .db
        .delete_from_trash(&apply_event_ids)
        .map_err(database_error)?;

    let mut wrap_ids = Vec::new();
    for event_id in &apply_event_ids {
        wrap_ids.extend(
            inner
                .db
                .get_wrap_ids_for_inner(event_id)
                .map_err(database_error)?,
        );
    }
    if !wrap_ids.is_empty() {
        inner
            .db
            .record_deletion_markers(&wrap_ids, source_event_id)
            .map_err(database_error)?;
    }

    let mut removed_ids: HashSet<String> = apply_event_ids.into_iter().collect();
    removed_ids.extend(wrap_ids);
    inner
        .events
        .retain(|event| !removed_ids.contains(&event.id.to_string()));
    Ok(true)
}

pub(crate) fn cache_and_verify_nip05(
    inner: &mut BackendInner,
    nip05: &str,
    pubkey_hex: &str,
    is_own: bool,
) -> HootResult<()> {
    inner
        .db
        .add_nip05(pubkey_hex, nip05, is_own)
        .map_err(database_error)?;
    let wake_up = inner.wake_up.clone();
    inner
        .nip05_verifier
        .request(nip05.to_string(), pubkey_hex.to_string(), wake_up);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::AccountManager;
    use crate::db::Db;
    use crate::nip05::{Nip05Resolver, Nip05Verifier};
    use crate::profile_metadata::ProfileOption;
    use crate::relay;
    use nostr::{EventBuilder, Keys};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn test_inner() -> HootResult<BackendInner> {
        Ok(BackendInner {
            storage_dir: PathBuf::new(),
            db: Db::new_in_memory().map_err(database_error)?,
            relays: relay::RelayPool::new(),
            events: Vec::new(),
            account_manager: AccountManager::new(),
            active_account_pubkey: None,
            profile_metadata: HashMap::new(),
            nip05_verifier: Nip05Verifier::new(),
            nip05_resolver: Nip05Resolver::new(),
            wake_up: Arc::new(|| {}),
        })
    }

    fn signed_event(keys: &Keys, kind: Kind, content: impl Into<String>) -> Event {
        EventBuilder::new(kind, content)
            .sign_with_keys(keys)
            .expect("event should sign")
    }

    #[test]
    fn process_event_updates_profile_metadata_and_ignores_invalid_events() -> HootResult<()> {
        let mut inner = test_inner()?;
        let keys = Keys::generate();
        let metadata = ProfileMetadata {
            name: Some("alice".to_string()),
            display_name: Some("Alice".to_string()),
            picture: Some("https://example.com/alice.png".to_string()),
            nip05: Some("alice@example.com".to_string()),
        };
        let metadata_event = signed_event(
            &keys,
            Kind::Metadata,
            serde_json::to_string(&metadata).map_err(HootError::from)?,
        );
        let raw = serde_json::to_string(&metadata_event).map_err(HootError::from)?;

        assert!(process_event(&mut inner, "metadata", &raw)?);
        assert_eq!(
            inner.profile_metadata.get(&keys.public_key().to_hex()),
            Some(&ProfileOption::Some(metadata.clone()))
        );
        assert_eq!(
            inner
                .db
                .get_profile_metadata(&keys.public_key().to_hex())
                .map_err(database_error)?,
            Some(metadata)
        );
        assert!(!process_event(&mut inner, "metadata", "not json")?);

        Ok(())
    }

    #[test]
    fn apply_deletions_scopes_mail_by_author_unscopes_non_mail_and_marks_wraps() -> HootResult<()> {
        let mut inner = test_inner()?;
        let author = Keys::generate();
        let other = Keys::generate();
        let own_mail = signed_event(&author, Kind::Custom(MAIL_EVENT_KIND), "own mail");
        let other_mail = signed_event(&other, Kind::Custom(MAIL_EVENT_KIND), "other mail");
        let text_note = signed_event(&other, Kind::TextNote, "text note");
        let own_mail_id = own_mail.id.to_string();
        let other_mail_id = other_mail.id.to_string();
        let text_note_id = text_note.id.to_string();

        for event in [&own_mail, &other_mail, &text_note] {
            inner
                .db
                .store_event(event, None, None)
                .map_err(database_error)?;
        }
        inner
            .db
            .save_gift_wrap_map("wrap-own", &own_mail_id, Some("recipient"), 100)
            .map_err(database_error)?;
        inner.events = vec![own_mail.clone(), other_mail.clone(), text_note.clone()];

        assert!(apply_deletions(
            &mut inner,
            vec![
                own_mail_id.clone(),
                other_mail_id.clone(),
                text_note_id.clone()
            ],
            Some(&author.public_key().to_hex()),
            Some("delete-source"),
        )?);

        assert!(!inner.db.has_event(&own_mail_id).map_err(database_error)?);
        assert!(inner.db.has_event(&other_mail_id).map_err(database_error)?);
        assert!(!inner.db.has_event(&text_note_id).map_err(database_error)?);
        assert!(inner
            .db
            .is_deleted(&own_mail_id, Some(&author.public_key().to_hex()))
            .map_err(database_error)?);
        assert!(!inner
            .db
            .is_deleted(&other_mail_id, Some(&other.public_key().to_hex()))
            .map_err(database_error)?);
        assert!(inner
            .db
            .is_deleted(&text_note_id, None)
            .map_err(database_error)?);
        assert!(inner
            .db
            .is_deleted("wrap-own", None)
            .map_err(database_error)?);
        let remaining: Vec<String> = inner
            .events
            .iter()
            .map(|event| event.id.to_string())
            .collect();
        assert_eq!(remaining, vec![other_mail_id]);

        Ok(())
    }
}
