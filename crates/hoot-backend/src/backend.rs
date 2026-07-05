use crate::account_manager::{self, AccountManager};
use crate::account_ops::*;
use crate::conversions::*;
use crate::db::{self, Db};
use crate::dto::*;
use crate::error::{HootError, HootResult};
use crate::event_processing::*;
use crate::mail_event::MailMessage;
use crate::nip05::{Nip05Resolution, Nip05Resolver, Nip05Verifier};
use crate::profile_metadata::ProfileOption;
use crate::relay::{self, ClientMessage};
use ewebsock::WsMessage;
use nostr::{Alphabet, Event, EventBuilder, Filter, FromBech32, Keys, Kind, PublicKey};
use nostr::{SingleLetterTag, ToBech32};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

pub type WakeCallback = Arc<dyn Fn() + Send + Sync + 'static>;

#[derive(uniffi::Object)]
pub struct HootBackend {
    inner: Mutex<BackendInner>,
}

pub(crate) struct BackendInner {
    pub(crate) storage_dir: PathBuf,
    pub(crate) db: Db,
    pub(crate) relays: relay::RelayPool,
    pub(crate) events: Vec<Event>,
    pub(crate) account_manager: AccountManager,
    pub(crate) active_account_pubkey: Option<String>,
    pub(crate) profile_metadata: HashMap<String, ProfileOption>,
    pub(crate) nip05_verifier: Nip05Verifier,
    pub(crate) nip05_resolver: Nip05Resolver,
    pub(crate) wake_up: WakeCallback,
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
            .update_draft(db::DraftUpdate {
                id: draft.id,
                subject: &draft.subject,
                to_field: &draft.to_field,
                content: &draft.content,
                parent_events: &draft.parent_events,
                selected_account: draft.selected_account.as_deref(),
                selected_nip05: draft.selected_nip05.as_deref(),
            })
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
        Ok(inner
            .nip05_resolver
            .get(&nip05)
            .map(|resolution| match resolution {
                Nip05Resolution::Pending => Nip05ResolutionDto::Pending,
                Nip05Resolution::Resolved(pubkey_hex) => Nip05ResolutionDto::Resolved {
                    pubkey_hex: pubkey_hex.clone(),
                },
                Nip05Resolution::Failed => Nip05ResolutionDto::Failed,
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
}
