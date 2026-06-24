use crate::account_manager::{self, AccountManager};
use crate::db::sender_status::SenderStatus;
use crate::db::{self, Db};
use crate::dto::*;
use crate::error::{HootError, HootResult};
use crate::mail_event::{MailMessage, MAIL_EVENT_KIND};
use crate::nip05::{Nip05Resolution, Nip05Resolver, Nip05Verifier};
use crate::profile_metadata::ProfileOption;
use crate::relay::{self, ClientMessage, RelayStatus};
use ewebsock::WsMessage;
use nostr::{Alphabet, Event, EventBuilder, EventId, Filter, FromBech32, Keys, Kind, PublicKey};
use nostr::{SingleLetterTag, TagKind, ToBech32};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

pub type WakeCallback = Arc<dyn Fn() + Send + Sync + 'static>;

#[derive(uniffi::Object)]
pub struct HootBackend {
    inner: Mutex<BackendInner>,
}

struct BackendInner {
    storage_dir: PathBuf,
    db: Db,
    relays: relay::RelayPool,
    events: Vec<Event>,
    account_manager: AccountManager,
    active_account_pubkey: Option<String>,
    profile_metadata: HashMap<String, ProfileOption>,
    nip05_verifier: Nip05Verifier,
    nip05_resolver: Nip05Resolver,
    wake_up: WakeCallback,
}

impl HootBackend {
    pub fn set_wake_callback(&self, wake_up: WakeCallback) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        inner.wake_up = wake_up;
        Ok(())
    }

    fn lock_inner(&self) -> HootResult<std::sync::MutexGuard<'_, BackendInner>> {
        self.inner.lock().map_err(|_| HootError::Internal {
            message: "backend lock poisoned".to_string(),
        })
    }
}

#[uniffi::export]
impl HootBackend {
    #[uniffi::constructor]
    pub fn open(storage_dir: String) -> HootResult<Arc<Self>> {
        let storage_dir = PathBuf::from(storage_dir);
        std::fs::create_dir_all(&storage_dir).map_err(|e| HootError::Database {
            message: e.to_string(),
        })?;
        let db_path = storage_dir.join("hoot.db");
        let db = Db::new(db_path).map_err(database_error)?;
        Ok(Arc::new(Self {
            inner: Mutex::new(BackendInner {
                storage_dir,
                db,
                relays: relay::RelayPool::new(),
                events: Vec::new(),
                account_manager: AccountManager::new(),
                active_account_pubkey: None,
                profile_metadata: HashMap::new(),
                nip05_verifier: Nip05Verifier::new(),
                nip05_resolver: Nip05Resolver::new(),
                wake_up: Arc::new(|| {}),
            }),
        }))
    }

    pub fn onboarding_complete(&self) -> HootResult<bool> {
        let inner = self.lock_inner()?;
        Ok(inner.storage_dir.join("done").exists())
    }

    pub fn mark_onboarding_complete(&self) -> HootResult<()> {
        let inner = self.lock_inner()?;
        std::fs::write(inner.storage_dir.join("done"), []).map_err(|e| HootError::Database {
            message: e.to_string(),
        })
    }

    pub fn db_file_has_password(&self) -> HootResult<bool> {
        let inner = self.lock_inner()?;
        let db_path = inner.storage_dir.join("hoot.db");
        Ok(std::fs::metadata(&db_path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false))
    }

    pub fn unlock_database(&self, password: String) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        inner.db.unlock_with_password(password).map_err(|e| {
            let formatted = db::format_unlock_error(&e);
            if formatted == "Wrong password" {
                HootError::WrongPassword
            } else {
                HootError::Database { message: formatted }
            }
        })
    }

    pub fn is_database_initialized(&self) -> HootResult<bool> {
        let inner = self.lock_inner()?;
        Ok(inner.db.is_initialized())
    }

    pub fn initialize(&self) -> HootResult<InitialSnapshot> {
        let mut inner = self.lock_inner()?;
        load_account_keys(&mut inner)?;
        if inner.active_account_pubkey.is_none() {
            inner.active_account_pubkey = inner
                .account_manager
                .loaded_keys
                .first()
                .map(|keys| keys.public_key().to_hex());
        }

        inner.db.purge_deleted_events().map_err(database_error)?;
        let expired_ids = inner
            .db
            .purge_expired_trash(chrono::Utc::now().timestamp())
            .map_err(database_error)?;
        if !expired_ids.is_empty() {
            let expired: HashSet<String> = expired_ids.into_iter().collect();
            inner
                .events
                .retain(|event| !expired.contains(&event.id.to_string()));
        }

        if !inner.account_manager.loaded_keys.is_empty() {
            update_gift_wrap_subscription(&mut inner)?;
        }
        load_profile_metadata_cache(&mut inner)?;

        Ok(InitialSnapshot {
            accounts: account_summaries(&inner)?,
            inbox: inner.db.get_top_level_messages().map_err(database_error)?,
            trash: inner.db.get_trash_messages().map_err(database_error)?,
            requests: inner.db.get_request_messages().map_err(database_error)?,
            junk: inner.db.get_junk_messages().map_err(database_error)?,
            drafts: draft_dtos(inner.db.get_drafts().map_err(database_error)?),
            contacts: contact_dtos(inner.db.get_user_contacts().map_err(database_error)?)?,
        })
    }

    pub fn tick(&self) -> HootResult<Vec<BackendEvent>> {
        let mut inner = self.lock_inner()?;
        let mut events = Vec::new();

        let wake_up = inner.wake_up.clone();
        inner.relays.keepalive(move || (wake_up)());
        if !inner.relays.relays.is_empty() {
            events.push(BackendEvent::RelayStatusesChanged);
        }

        if let Some((relay_url, raw)) = inner.relays.try_recv() {
            match relay::RelayMessage::from_json(&raw) {
                Ok(message) => {
                    if process_message(&mut inner, &relay_url, &message)? {
                        events.push(BackendEvent::MailboxesChanged);
                    }
                }
                Err(err) => warn!("could not decode message sent from relay: {}", err),
            }
        }

        if process_verification_queue(&mut inner) {
            events.push(BackendEvent::NIp05ResultsChanged);
        }
        if process_resolution_queue(&mut inner) {
            events.push(BackendEvent::NIp05ResultsChanged);
        }

        Ok(dedup_events(events))
    }

    pub fn list_accounts(&self) -> HootResult<Vec<AccountSummary>> {
        let inner = self.lock_inner()?;
        account_summaries(&inner)
    }

    pub fn validate_nsec(&self, input: String) -> HootResult<AccountSummary> {
        let keys = account_manager::validate_nsec(&input)
            .map_err(|message| HootError::InvalidNsec { message })?;
        Ok(account_summary_for_keys(&keys, None, false))
    }

    pub fn generate_account(&self) -> HootResult<AccountSummary> {
        let mut inner = self.lock_inner()?;
        let keys = generate_new_keys(&mut inner)?;
        if inner.active_account_pubkey.is_none() {
            inner.active_account_pubkey = Some(keys.public_key().to_hex());
        }
        update_gift_wrap_subscription(&mut inner)?;
        account_summary(&inner, &keys)
    }

    /// Generate a new keypair without saving it. Returns the account summary and the nsec.
    pub fn generate_account_preview(&self) -> HootResult<AccountPreview> {
        let keys = Keys::generate();
        let nsec = keys
            .secret_key()
            .to_bech32()
            .map_err(|e| HootError::Nostr {
                message: e.to_string(),
            })?;
        let summary = account_summary_for_keys(&keys, None, false);
        Ok(AccountPreview { summary, nsec })
    }

    pub fn import_account(&self, nsec: String) -> HootResult<AccountSummary> {
        let keys = account_manager::validate_nsec(&nsec)
            .map_err(|message| HootError::InvalidNsec { message })?;
        let mut inner = self.lock_inner()?;
        save_account_keys(&mut inner, &keys)?;
        if inner.active_account_pubkey.is_none() {
            inner.active_account_pubkey = Some(keys.public_key().to_hex());
        }
        update_gift_wrap_subscription(&mut inner)?;
        account_summary(&inner, &keys)
    }

    pub fn delete_account(&self, pubkey_hex: String) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        let key = inner
            .account_manager
            .loaded_keys
            .iter()
            .find(|keys| keys.public_key().to_hex() == pubkey_hex)
            .cloned()
            .ok_or_else(|| HootError::NotFound {
                entity: "account".to_string(),
                id: pubkey_hex.clone(),
            })?;
        delete_account_key(&mut inner, &key)?;
        if inner.active_account_pubkey.as_deref() == Some(pubkey_hex.as_str()) {
            inner.active_account_pubkey = inner
                .account_manager
                .loaded_keys
                .first()
                .map(|keys| keys.public_key().to_hex());
        }
        update_gift_wrap_subscription(&mut inner)?;
        Ok(())
    }

    pub fn set_active_account(&self, pubkey_hex: Option<String>) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        if let Some(pubkey) = &pubkey_hex {
            let exists = inner
                .account_manager
                .loaded_keys
                .iter()
                .any(|keys| keys.public_key().to_hex() == *pubkey);
            if !exists {
                return Err(HootError::NotFound {
                    entity: "account".to_string(),
                    id: pubkey.clone(),
                });
            }
        }
        inner.active_account_pubkey = pubkey_hex;
        Ok(())
    }

    pub fn list_messages(&self, mailbox: Mailbox) -> HootResult<Vec<TableEntry>> {
        let inner = self.lock_inner()?;
        match mailbox {
            Mailbox::Inbox => inner.db.get_top_level_messages(),
            Mailbox::Trash => inner.db.get_trash_messages(),
            Mailbox::Requests => inner.db.get_request_messages(),
            Mailbox::Junk => inner.db.get_junk_messages(),
        }
        .map_err(database_error)
    }

    pub fn get_thread(
        &self,
        event_id: String,
        include_trash: bool,
    ) -> HootResult<Vec<MailMessageDto>> {
        let inner = self.lock_inner()?;
        let thread = if include_trash {
            inner.db.get_email_thread_including_trash(&event_id)
        } else {
            inner.db.get_email_thread(&event_id)
        }
        .map_err(database_error)?;
        Ok(thread.into_iter().map(mail_message_dto).collect())
    }

    pub fn search_messages(&self, query: String) -> HootResult<Vec<TableEntry>> {
        let inner = self.lock_inner()?;
        inner.db.search_messages(&query).map_err(database_error)
    }

    pub fn list_drafts(&self) -> HootResult<Vec<DraftDto>> {
        let inner = self.lock_inner()?;
        Ok(draft_dtos(inner.db.get_drafts().map_err(database_error)?))
    }

    pub fn save_draft(&self, draft: DraftInput) -> HootResult<i64> {
        let inner = self.lock_inner()?;
        inner
            .db
            .save_draft(
                &draft.subject,
                &draft.to_field,
                &draft.content,
                &draft.parent_events,
                draft.selected_account.as_deref(),
                draft.selected_nip05.as_deref(),
            )
            .map_err(database_error)
    }

    pub fn update_draft(&self, draft: DraftDto) -> HootResult<()> {
        let inner = self.lock_inner()?;
        inner
            .db
            .update_draft(
                draft.id,
                &draft.subject,
                &draft.to_field,
                &draft.content,
                &draft.parent_events,
                draft.selected_account.as_deref(),
                draft.selected_nip05.as_deref(),
            )
            .map_err(database_error)
    }

    pub fn delete_draft(&self, id: i64) -> HootResult<()> {
        let inner = self.lock_inner()?;
        inner.db.delete_draft(id).map_err(database_error)
    }

    pub fn list_contacts(&self) -> HootResult<Vec<ContactDto>> {
        let inner = self.lock_inner()?;
        contact_dtos(inner.db.get_user_contacts().map_err(database_error)?)
    }

    pub fn save_contact(&self, pubkey: String, petname: Option<String>) -> HootResult<()> {
        let inner = self.lock_inner()?;
        let parsed_pubkey = parse_public_key(&pubkey)?;
        let pubkey_hex = parsed_pubkey.to_hex();
        inner
            .db
            .save_contact(&pubkey_hex, petname.as_deref())
            .map_err(database_error)
    }

    pub fn update_contact_petname(
        &self,
        pubkey: String,
        petname: Option<String>,
    ) -> HootResult<()> {
        let inner = self.lock_inner()?;
        inner
            .db
            .update_contact_petname(&pubkey, petname.as_deref())
            .map_err(database_error)
    }

    pub fn delete_contact(&self, pubkey: String) -> HootResult<()> {
        let inner = self.lock_inner()?;
        inner.db.delete_contact(&pubkey).map_err(database_error)
    }

    pub fn set_sender_status(&self, pubkey: String, status: SenderStatusDto) -> HootResult<()> {
        let inner = self.lock_inner()?;
        inner
            .db
            .set_sender_status(&pubkey, &sender_status(status))
            .map_err(database_error)
    }

    pub fn remove_sender_status(&self, pubkey: String) -> HootResult<()> {
        let inner = self.lock_inner()?;
        inner
            .db
            .remove_sender_status(&pubkey)
            .map_err(database_error)
    }

    pub fn get_sender_status(&self, pubkey: String) -> HootResult<Option<SenderStatusDto>> {
        let inner = self.lock_inner()?;
        Ok(inner
            .db
            .get_sender_status(&pubkey)
            .map_err(database_error)?
            .map(sender_status_dto))
    }

    pub fn get_cached_nip05(&self, pubkey: String) -> HootResult<Option<Nip05Entry>> {
        let inner = self.lock_inner()?;
        inner.db.get_cached_nip05(&pubkey).map_err(database_error)
    }

    pub fn get_nip05s_for_pubkey(&self, pubkey: String) -> HootResult<Vec<Nip05Entry>> {
        let inner = self.lock_inner()?;
        inner
            .db
            .get_nip05s_for_pubkey(&pubkey)
            .map_err(database_error)
    }

    pub fn add_nip05(&self, pubkey: String, nip05: String, is_own: bool) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        inner
            .db
            .add_nip05(&pubkey, &nip05, is_own)
            .map_err(database_error)?;
        inner.nip05_verifier.request(nip05, pubkey);
        Ok(())
    }

    pub fn delete_nip05(&self, pubkey: String, nip05: String) -> HootResult<()> {
        let inner = self.lock_inner()?;
        inner
            .db
            .delete_nip05(&pubkey, &nip05)
            .map_err(database_error)
    }

    pub fn request_nip05_resolution(&self, nip05: String) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        inner.nip05_resolver.request(nip05);
        Ok(())
    }

    pub fn get_profile_metadata(&self, pubkey_hex: String) -> HootResult<Option<ProfileMetadata>> {
        let mut inner = self.lock_inner()?;
        get_profile_metadata(&mut inner, pubkey_hex)
    }

    pub fn update_profile_metadata(
        &self,
        pubkey_hex: String,
        metadata: ProfileMetadata,
    ) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        let keys = inner
            .account_manager
            .loaded_keys
            .iter()
            .find(|keys| keys.public_key().to_hex() == pubkey_hex)
            .cloned()
            .ok_or_else(|| HootError::NotFound {
                entity: "account".to_string(),
                id: pubkey_hex.clone(),
            })?;

        let serialized = serde_json::to_string(&metadata)?;
        let event = EventBuilder::new(Kind::Metadata, serialized)
            .sign_with_keys(&keys)
            .map_err(nostr_error)?;
        inner
            .db
            .write_profile_metadata(event.clone())
            .map_err(database_error)?;
        inner
            .profile_metadata
            .insert(pubkey_hex, ProfileOption::Some(metadata));
        send_client_message(&mut inner, ClientMessage::Event { event })
    }

    pub fn send_message(&self, input: ComposeMessageInput) -> HootResult<SendMessageResult> {
        let mut inner = self.lock_inner()?;
        let account_pubkey = input
            .selected_account_pubkey
            .clone()
            .or_else(|| inner.active_account_pubkey.clone())
            .ok_or_else(|| HootError::NotFound {
                entity: "account".to_string(),
                id: "<none>".to_string(),
            })?;
        let sending_keys = inner
            .account_manager
            .loaded_keys
            .iter()
            .find(|keys| keys.public_key().to_hex() == account_pubkey)
            .cloned()
            .ok_or_else(|| HootError::NotFound {
                entity: "account".to_string(),
                id: account_pubkey.clone(),
            })?;

        let recipients = resolve_recipients(&mut inner, &input.to_field)?;
        if !recipients.pending_nip05.is_empty() || !recipients.failed_nip05.is_empty() {
            return Ok(SendMessageResult {
                sent_count: 0,
                pending_nip05: recipients.pending_nip05,
                failed_nip05: recipients.failed_nip05,
            });
        }
        if recipients.public_keys.is_empty() {
            return Err(HootError::Nostr {
                message: "No valid recipients".to_string(),
            });
        }

        let parent_events = parse_event_ids(input.parent_event_ids)?;
        let mut message = MailMessage {
            id: None,
            created_at: None,
            author: None,
            to: recipients.public_keys,
            cc: Vec::new(),
            bcc: Vec::new(),
            parent_events: Some(parent_events),
            subject: input.subject,
            content: input.content,
            sender_nip05: input.selected_nip05,
        };
        let events_to_send = message
            .try_to_events(&sending_keys)
            .map_err(|message| HootError::Nostr { message })?;
        let sent_count = events_to_send.len() as u32;
        for event in events_to_send.into_values() {
            send_client_message(&mut inner, ClientMessage::Event { event })?;
        }
        Ok(SendMessageResult {
            sent_count,
            pending_nip05: Vec::new(),
            failed_nip05: Vec::new(),
        })
    }

    pub fn move_to_trash(&self, event_id: String, purge_after: i64) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        inner
            .db
            .record_trash(&[event_id], purge_after)
            .map_err(database_error)
    }

    pub fn restore_from_trash(&self, event_id: String) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        inner
            .db
            .restore_from_trash(&event_id)
            .map_err(database_error)
    }

    pub fn delete_messages_permanently(&self, event_ids: Vec<String>) -> HootResult<()> {
        if event_ids.is_empty() {
            return Ok(());
        }
        let mut inner = self.lock_inner()?;
        apply_deletions(&mut inner, event_ids, None, None).map(|_| ())
    }

    pub fn add_relay(&self, url: String) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        let wake_up = inner.wake_up.clone();
        inner
            .relays
            .add_url(url, move || (wake_up)())
            .map_err(HootError::from)
    }

    pub fn remove_relay(&self, url: String) -> HootResult<()> {
        let mut inner = self.lock_inner()?;
        inner.relays.remove_url(&url);
        Ok(())
    }

    pub fn relay_statuses(&self) -> HootResult<Vec<RelayStatusDto>> {
        let inner = self.lock_inner()?;
        let mut statuses: Vec<RelayStatusDto> = inner
            .relays
            .relays
            .values()
            .map(|relay| {
                let mut authenticated_pubkeys: Vec<String> = relay
                    .auth_state
                    .authenticated_keys
                    .iter()
                    .cloned()
                    .collect();
                authenticated_pubkeys.sort();
                RelayStatusDto {
                    url: relay.url.clone(),
                    status: relay_connection_status(relay.status),
                    authenticated_pubkeys,
                }
            })
            .collect();
        statuses.sort_by(|a, b| a.url.cmp(&b.url));
        Ok(statuses)
    }
}

struct RecipientResolution {
    public_keys: Vec<PublicKey>,
    pending_nip05: Vec<String>,
    failed_nip05: Vec<String>,
}

fn database_error(value: anyhow::Error) -> HootError {
    HootError::Database {
        message: value.to_string(),
    }
}

fn keyring_error(value: anyhow::Error) -> HootError {
    HootError::Keyring {
        message: value.to_string(),
    }
}

fn load_account_keys(inner: &mut BackendInner) -> HootResult<Vec<Keys>> {
    inner
        .account_manager
        .load_keys(&inner.db)
        .map_err(keyring_error)
}

fn generate_new_keys(inner: &mut BackendInner) -> HootResult<Keys> {
    inner
        .account_manager
        .generate_new_keys_and_save(&inner.db)
        .map_err(keyring_error)
}

fn save_account_keys(inner: &mut BackendInner, keys: &Keys) -> HootResult<()> {
    inner
        .account_manager
        .save_keys(&inner.db, keys)
        .map_err(keyring_error)
}

fn delete_account_key(inner: &mut BackendInner, keys: &Keys) -> HootResult<()> {
    inner
        .account_manager
        .delete_key(&inner.db, keys)
        .map_err(keyring_error)
}

fn process_verification_queue(inner: &mut BackendInner) -> bool {
    inner.nip05_verifier.process_queue(&inner.db)
}

fn process_resolution_queue(inner: &mut BackendInner) -> bool {
    inner.nip05_resolver.process_queue()
}

fn nostr_error(value: impl std::fmt::Display) -> HootError {
    HootError::Nostr {
        message: value.to_string(),
    }
}

fn parse_public_key(pubkey: &str) -> HootResult<PublicKey> {
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

fn parse_event_ids(event_ids: Vec<String>) -> HootResult<Vec<EventId>> {
    event_ids
        .into_iter()
        .map(|event_id| {
            EventId::parse(&event_id).map_err(|e| HootError::Nostr {
                message: format!("invalid event id `{}`: {}", event_id, e),
            })
        })
        .collect()
}

fn account_summaries(inner: &BackendInner) -> HootResult<Vec<AccountSummary>> {
    inner
        .account_manager
        .loaded_keys
        .iter()
        .map(|keys| account_summary(inner, keys))
        .collect()
}

fn account_summary(inner: &BackendInner, keys: &Keys) -> HootResult<AccountSummary> {
    let pubkey_hex = keys.public_key().to_hex();
    let metadata = inner
        .db
        .get_profile_metadata(&pubkey_hex)
        .map_err(database_error)?;
    Ok(account_summary_for_keys(
        keys,
        metadata,
        inner.active_account_pubkey.as_deref() == Some(pubkey_hex.as_str()),
    ))
}

fn account_summary_for_keys(
    keys: &Keys,
    metadata: Option<ProfileMetadata>,
    is_active: bool,
) -> AccountSummary {
    let pubkey_hex = keys.public_key().to_hex();
    let npub = keys
        .public_key()
        .to_bech32()
        .unwrap_or_else(|_| pubkey_hex.clone());
    let display_name = metadata.and_then(|metadata| metadata.display_name.or(metadata.name));
    AccountSummary {
        pubkey_hex,
        npub,
        display_name,
        is_active,
    }
}

fn draft_dtos(drafts: Vec<db::Draft>) -> Vec<DraftDto> {
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

fn contact_dtos(
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

fn mail_message_dto(message: MailMessage) -> MailMessageDto {
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

fn sender_status(status: SenderStatusDto) -> SenderStatus {
    match status {
        SenderStatusDto::Allowed => SenderStatus::Allowed,
        SenderStatusDto::Junked => SenderStatus::Junked,
    }
}

fn sender_status_dto(status: SenderStatus) -> SenderStatusDto {
    match status {
        SenderStatus::Allowed => SenderStatusDto::Allowed,
        SenderStatus::Junked => SenderStatusDto::Junked,
    }
}

fn relay_connection_status(status: RelayStatus) -> RelayConnectionStatus {
    match status {
        RelayStatus::Connecting => RelayConnectionStatus::Connecting,
        RelayStatus::Connected => RelayConnectionStatus::Connected,
        RelayStatus::Disconnected => RelayConnectionStatus::Disconnected,
    }
}

fn dedup_events(events: Vec<BackendEvent>) -> Vec<BackendEvent> {
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

fn load_profile_metadata_cache(inner: &mut BackendInner) -> HootResult<()> {
    for (pubkey, metadata) in inner.db.get_contacts().map_err(database_error)? {
        inner
            .profile_metadata
            .insert(pubkey, ProfileOption::Some(metadata));
    }
    Ok(())
}

fn update_gift_wrap_subscription(inner: &mut BackendInner) -> HootResult<()> {
    if inner.account_manager.loaded_keys.is_empty() {
        return Ok(());
    }

    let public_keys: Vec<PublicKey> = inner
        .account_manager
        .loaded_keys
        .iter()
        .map(|keys| keys.public_key())
        .collect();
    let filter = Filter::new().kind(Kind::GiftWrap).custom_tag(
        SingleLetterTag {
            character: Alphabet::P,
            uppercase: false,
        },
        public_keys,
    );
    let mut subscription = relay::Subscription::default();
    subscription.filter(filter);
    inner
        .relays
        .add_subscription(subscription)
        .map_err(HootError::from)
}

fn get_profile_metadata(
    inner: &mut BackendInner,
    pubkey_hex: String,
) -> HootResult<Option<ProfileMetadata>> {
    match inner.profile_metadata.get(&pubkey_hex) {
        Some(ProfileOption::Some(metadata)) => return Ok(Some(metadata.clone())),
        Some(ProfileOption::Waiting) => return Ok(None),
        None => {}
    }

    if let Some(metadata) = inner
        .db
        .get_profile_metadata(&pubkey_hex)
        .map_err(database_error)?
    {
        inner
            .profile_metadata
            .insert(pubkey_hex, ProfileOption::Some(metadata.clone()));
        return Ok(Some(metadata));
    }

    let public_key = parse_public_key(&pubkey_hex)?;
    let filter = Filter::new().kind(Kind::Metadata).author(public_key);
    let mut subscription = relay::Subscription::default();
    subscription.filter(filter);
    inner
        .relays
        .add_subscription(subscription)
        .map_err(HootError::from)?;
    inner
        .profile_metadata
        .insert(pubkey_hex, ProfileOption::Waiting);
    Ok(None)
}

fn resolve_recipients(inner: &mut BackendInner, to_field: &str) -> HootResult<RecipientResolution> {
    let mut public_keys = Vec::new();
    let mut pending_nip05 = Vec::new();
    let mut failed_nip05 = Vec::new();

    for raw_recipient in to_field.split_whitespace() {
        if raw_recipient.contains('@') {
            match inner.nip05_resolver.get(raw_recipient) {
                Some(Nip05Resolution::Resolved(pubkey_hex)) => {
                    match PublicKey::from_hex(pubkey_hex) {
                        Ok(pubkey) => public_keys.push(pubkey),
                        Err(_) => failed_nip05.push(raw_recipient.to_string()),
                    }
                }
                Some(Nip05Resolution::Pending) => pending_nip05.push(raw_recipient.to_string()),
                Some(Nip05Resolution::Failed) => failed_nip05.push(raw_recipient.to_string()),
                None => {
                    inner.nip05_resolver.request(raw_recipient.to_string());
                    pending_nip05.push(raw_recipient.to_string());
                }
            }
            continue;
        }

        if let Ok(pubkey) = PublicKey::from_bech32(raw_recipient) {
            public_keys.push(pubkey);
        } else if let Ok(pubkey) = PublicKey::from_hex(raw_recipient) {
            public_keys.push(pubkey);
        } else {
            debug!("could not parse public key recipient: {}", raw_recipient);
        }
    }

    Ok(RecipientResolution {
        public_keys,
        pending_nip05,
        failed_nip05,
    })
}

fn send_client_message(inner: &mut BackendInner, message: ClientMessage) -> HootResult<()> {
    let payload = serde_json::to_string(&message)?;
    inner
        .relays
        .send(WsMessage::Text(payload))
        .map_err(HootError::from)
}

fn process_message(
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

fn perform_auth(inner: &mut BackendInner, relay_url: &str) -> HootResult<()> {
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

fn cache_and_verify_nip05(
    inner: &mut BackendInner,
    nip05: &str,
    pubkey_hex: &str,
    is_own: bool,
) -> HootResult<()> {
    inner
        .db
        .add_nip05(pubkey_hex, nip05, is_own)
        .map_err(database_error)?;
    inner
        .nip05_verifier
        .request(nip05.to_string(), pubkey_hex.to_string());
    Ok(())
}

fn process_event(
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

fn apply_deletions(
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
        match inner
            .db
            .get_event_kind_pubkey(&event_id)
            .map_err(database_error)?
        {
            Some((kind, pubkey)) => {
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
            None => {}
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
