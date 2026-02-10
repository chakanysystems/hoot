use crate::ui;
use eframe::egui;
use std::collections::HashMap;

// WE PROBABLY SHOULDN'T MAKE EVERYTHING A STRING, GRR!
#[derive(Clone, Debug)]
pub struct TableEntry {
    pub id: String,
    pub content: String,
    pub subject: String,
    pub pubkey: String,
    pub created_at: i64,
    pub thread_count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Inbox,
    Drafts,
    Starred,
    Archived,
    Trash,
    Requests,
    Junk,
    Settings,
    // TODO: fix this mess
    Onboarding,
    OnboardingNewUser,
    OnboardingNewShowKey,
    OnboardingReturning,
    Post,
    Contacts,
    Unlock,
}

// for storing the state of different components and such.
#[derive(Default)]
pub struct HootState {
    pub add_account_window: HashMap<egui::Id, ui::add_account_window::AddAccountWindowState>,
    pub compose_window: HashMap<egui::Id, ui::compose_window::ComposeWindowState>,
    pub onboarding: ui::onboarding::OnboardingState,
    pub settings: ui::settings::SettingsState,
    pub unlock_database: ui::unlock_database::UnlockDatabaseState,
    pub contacts: ContactsPageState,
    pub requests: RequestsPageState,
}

#[derive(Default)]
pub struct ContactsPageState {
    pub add_pubkey_input: String,
    pub add_petname_input: String,
    pub show_add_form: bool,
    pub editing_pubkey: Option<String>,
    pub editing_petname_buf: String,
    pub add_error: Option<String>,
}

#[derive(Default)]
pub struct RequestsPageState {
    pub add_to_contacts: bool,
}

#[derive(Debug, PartialEq)]
pub enum HootStatus {
    PreUnlock,
    WaitingForUnlock,
    Initializing,
    Ready,
}
