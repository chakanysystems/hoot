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

    pub fn senders_by_status(&self, status: SenderStatusDto) -> HootResult<Vec<SenderStatusEntry>> {
        let inner = self.lock_inner()?;
        inner
            .db
            .get_senders_by_status(&sender_status(status))
            .map(|senders| {
                senders
                    .into_iter()
                    .map(
                        |(pubkey, name, display_name, picture, created_at)| SenderStatusEntry {
                            pubkey,
                            name,
                            display_name,
                            picture,
                            created_at,
                        },
                    )
                    .collect()
            })
            .map_err(database_error)
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
        let wake_up = inner.wake_up.clone();
        inner.nip05_verifier.request(nip05, pubkey, wake_up);
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
        let wake_up = inner.wake_up.clone();
        inner.nip05_resolver.request(nip05, wake_up);
        Ok(())
    }

    pub fn get_nip05_resolution(&self, nip05: String) -> HootResult<Option<Nip05ResolutionDto>> {
        let inner = self.lock_inner()?;
        let Some(resolution) = inner.nip05_resolver.get(&nip05) else {
            return Ok(None);
        };
        Ok(Some(match resolution {
            Nip05Resolution::Pending => Nip05ResolutionDto {
                status: Nip05ResolutionStatusDto::Pending,
                pubkey_hex: None,
            },
            Nip05Resolution::Resolved(pubkey_hex) => Nip05ResolutionDto {
                status: Nip05ResolutionStatusDto::Resolved,
                pubkey_hex: Some(pubkey_hex.clone()),
            },
            Nip05Resolution::Failed => Nip05ResolutionDto {
                status: Nip05ResolutionStatusDto::Failed,
                pubkey_hex: None,
            },
        }))
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
                    let wake_up = inner.wake_up.clone();
                    inner
                        .nip05_resolver
                        .request(raw_recipient.to_string(), wake_up);
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
    let wake_up = inner.wake_up.clone();
    inner
        .nip05_verifier
        .request(nip05.to_string(), pubkey_hex.to_string(), wake_up);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_storage_dir(name: &str) -> HootResult<PathBuf> {
        let dir = std::env::temp_dir().join(format!(
            "hoot-backend-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|err| HootError::Database {
                    message: err.to_string(),
                })?
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).map_err(|err| HootError::Database {
            message: err.to_string(),
        })?;
        Ok(dir)
    }

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

    fn test_backend_with_inner(inner: BackendInner) -> HootBackend {
        HootBackend {
            inner: Mutex::new(inner),
        }
    }

    fn compose_input(
        to_field: impl Into<String>,
        selected_account_pubkey: Option<String>,
    ) -> ComposeMessageInput {
        ComposeMessageInput {
            subject: "Subject".to_string(),
            content: "Body".to_string(),
            to_field: to_field.into(),
            parent_event_ids: Vec::new(),
            selected_account_pubkey,
            selected_nip05: None,
        }
    }

    fn signed_event(keys: &Keys, kind: Kind, content: impl Into<String>) -> Event {
        EventBuilder::new(kind, content)
            .sign_with_keys(keys)
            .expect("event should sign")
    }

    #[test]
    fn backend_onboarding_and_database_password_state_are_file_backed() -> HootResult<()> {
        let dir = temp_storage_dir("state")?;
        let backend = HootBackend::open(dir.to_string_lossy().into_owned())?;

        assert!(!backend.onboarding_complete()?);
        assert!(!backend.db_file_has_password()?);
        assert!(!backend.is_database_initialized()?);

        backend.mark_onboarding_complete()?;
        assert!(backend.onboarding_complete()?);

        backend.unlock_database("correct-password".to_string())?;
        assert!(backend.db_file_has_password()?);
        assert!(backend.is_database_initialized()?);

        std::fs::remove_dir_all(dir).map_err(|err| HootError::Database {
            message: err.to_string(),
        })?;
        Ok(())
    }

    #[test]
    fn backend_maps_wrong_database_password_to_domain_error() -> HootResult<()> {
        let dir = temp_storage_dir("wrong-password")?;
        {
            let backend = HootBackend::open(dir.to_string_lossy().into_owned())?;
            backend.unlock_database("correct-password".to_string())?;
        }
        let backend = HootBackend::open(dir.to_string_lossy().into_owned())?;

        let err = backend
            .unlock_database("wrong-password".to_string())
            .unwrap_err();

        assert!(matches!(err, HootError::WrongPassword));
        std::fs::remove_dir_all(dir).map_err(|err| HootError::Database {
            message: err.to_string(),
        })?;
        Ok(())
    }

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
    fn account_summary_uses_profile_display_name_precedence_and_active_marker() {
        let keys = Keys::generate();
        let with_display_name = account_summary_for_keys(
            &keys,
            Some(ProfileMetadata {
                name: Some("alice".to_string()),
                display_name: Some("Alice Display".to_string()),
                picture: None,
                nip05: None,
            }),
            true,
        );
        assert_eq!(with_display_name.pubkey_hex, keys.public_key().to_hex());
        assert_eq!(
            with_display_name.display_name.as_deref(),
            Some("Alice Display")
        );
        assert!(with_display_name.is_active);

        let with_name_only = account_summary_for_keys(
            &keys,
            Some(ProfileMetadata {
                name: Some("alice".to_string()),
                display_name: None,
                picture: None,
                nip05: None,
            }),
            false,
        );
        assert_eq!(with_name_only.display_name.as_deref(), Some("alice"));
        assert!(!with_name_only.is_active);
    }

    #[test]
    fn account_preview_nsec_validates_to_the_preview_summary_without_persisting() -> HootResult<()>
    {
        let backend = test_backend_with_inner(test_inner()?);

        let preview = backend.generate_account_preview()?;
        let validated = account_manager::validate_nsec(&preview.nsec)
            .map_err(|message| HootError::InvalidNsec { message })?;

        assert_eq!(preview.summary.pubkey_hex, validated.public_key().to_hex());
        assert_eq!(
            preview.summary.npub,
            validated.public_key().to_bech32().map_err(nostr_error)?
        );
        assert!(backend.list_accounts()?.is_empty());
        Ok(())
    }

    #[test]
    fn set_active_account_rejects_unknown_pubkey_without_changing_existing_active_account(
    ) -> HootResult<()> {
        let mut inner = test_inner()?;
        let existing = Keys::generate();
        let existing_pubkey = existing.public_key().to_hex();
        let missing_pubkey = Keys::generate().public_key().to_hex();
        inner.account_manager.loaded_keys = vec![existing];
        inner.active_account_pubkey = Some(existing_pubkey.clone());
        let backend = test_backend_with_inner(inner);

        let error = backend
            .set_active_account(Some(missing_pubkey.clone()))
            .unwrap_err();

        assert!(matches!(
            error,
            HootError::NotFound { entity, id }
                if entity == "account" && id == missing_pubkey
        ));
        let accounts = backend.list_accounts()?;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].pubkey_hex, existing_pubkey);
        assert!(accounts[0].is_active);
        Ok(())
    }

    #[test]
    fn send_message_reports_missing_account_before_attempting_recipient_resolution(
    ) -> HootResult<()> {
        let backend = test_backend_with_inner(test_inner()?);

        let no_active_error = backend
            .send_message(compose_input("not-a-recipient", None))
            .unwrap_err();
        assert!(matches!(
            no_active_error,
            HootError::NotFound { entity, id }
                if entity == "account" && id == "<none>"
        ));

        let missing_pubkey = Keys::generate().public_key().to_hex();
        let selected_missing_error = backend
            .send_message(compose_input(
                "not-a-recipient",
                Some(missing_pubkey.clone()),
            ))
            .unwrap_err();
        assert!(matches!(
            selected_missing_error,
            HootError::NotFound { entity, id }
                if entity == "account" && id == missing_pubkey
        ));
        Ok(())
    }

    #[test]
    fn send_message_rejects_empty_resolved_recipient_set_without_touching_relays() -> HootResult<()>
    {
        let mut inner = test_inner()?;
        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();
        inner.account_manager.loaded_keys = vec![keys];
        inner.active_account_pubkey = Some(pubkey);
        let backend = test_backend_with_inner(inner);

        let error = backend
            .send_message(compose_input("not-a-public-key", None))
            .unwrap_err();

        assert!(matches!(
            error,
            HootError::Nostr { message } if message == "No valid recipients"
        ));
        Ok(())
    }

    #[test]
    fn send_message_returns_pending_then_failed_for_unresolved_nip05_without_network(
    ) -> HootResult<()> {
        let mut inner = test_inner()?;
        let keys = Keys::generate();
        let account_pubkey = keys.public_key().to_hex();
        let unresolved = "bob@@example.com";
        let (wake_sender, wake_receiver) = std::sync::mpsc::channel();
        inner.account_manager.loaded_keys = vec![keys];
        inner.active_account_pubkey = Some(account_pubkey);
        inner.wake_up = Arc::new(move || {
            let _ = wake_sender.send(());
        });
        let backend = test_backend_with_inner(inner);

        let pending = backend.send_message(compose_input(unresolved, None))?;
        assert_eq!(pending.sent_count, 0);
        assert_eq!(pending.pending_nip05, vec![unresolved.to_string()]);
        assert!(pending.failed_nip05.is_empty());

        wake_receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("invalid NIP-05 resolution should complete without network");
        assert!(backend
            .tick()?
            .iter()
            .any(|event| matches!(event, BackendEvent::NIp05ResultsChanged)));

        let failed = backend.send_message(compose_input(unresolved, None))?;
        assert_eq!(failed.sent_count, 0);
        assert!(failed.pending_nip05.is_empty());
        assert_eq!(failed.failed_nip05, vec![unresolved.to_string()]);
        Ok(())
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
