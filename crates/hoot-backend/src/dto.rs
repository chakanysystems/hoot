#[derive(Clone, Debug, uniffi::Record)]
pub struct TableEntry {
    pub id: String,
    pub content: String,
    pub subject: String,
    pub pubkey: String,
    pub created_at: i64,
    pub thread_count: i64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DraftDto {
    pub id: i64,
    pub subject: String,
    pub to_field: String,
    pub content: String,
    pub parent_events: Vec<String>,
    pub selected_account: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DraftInput {
    pub subject: String,
    pub to_field: String,
    pub content: String,
    pub parent_events: Vec<String>,
    pub selected_account: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq, uniffi::Record)]
pub struct ProfileMetadata {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub picture: Option<String>,
    pub nip05: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ContactDto {
    pub pubkey: String,
    pub petname: Option<String>,
    pub metadata: ProfileMetadata,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AccountSummary {
    pub pubkey_hex: String,
    pub npub: String,
    pub display_name: Option<String>,
    pub is_active: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct MailMessageDto {
    pub id: Option<String>,
    pub created_at: Option<i64>,
    pub author_pubkey: Option<String>,
    pub to_pubkeys: Vec<String>,
    pub cc_pubkeys: Vec<String>,
    pub bcc_pubkeys: Vec<String>,
    pub parent_event_ids: Vec<String>,
    pub subject: String,
    pub content: String,
    pub sender_nip05: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ComposeMessageInput {
    pub subject: String,
    pub content: String,
    pub to_field: String,
    pub parent_event_ids: Vec<String>,
    pub selected_account_pubkey: Option<String>,
    pub selected_nip05: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SendMessageResult {
    pub sent_count: u32,
    pub pending_nip05: Vec<String>,
    pub failed_nip05: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Nip05Entry {
    pub id: i64,
    pub pubkey: String,
    pub nip05: String,
    pub is_own: bool,
    pub first_seen: i64,
    pub last_verified: Option<i64>,
    pub last_checked: Option<i64>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RelayStatusDto {
    pub url: String,
    pub status: RelayConnectionStatus,
    pub authenticated_pubkeys: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct InitialSnapshot {
    pub accounts: Vec<AccountSummary>,
    pub inbox: Vec<TableEntry>,
    pub trash: Vec<TableEntry>,
    pub requests: Vec<TableEntry>,
    pub junk: Vec<TableEntry>,
    pub drafts: Vec<DraftDto>,
    pub contacts: Vec<ContactDto>,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Mailbox {
    Inbox,
    Trash,
    Requests,
    Junk,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SenderStatusDto {
    Allowed,
    Junked,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum RelayConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum BackendEvent {
    MailboxesChanged,
    RelayStatusesChanged,
    NIp05ResultsChanged,
    AccountsChanged,
}
