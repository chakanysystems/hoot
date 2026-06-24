use crate::ui;
use eframe::egui;
use hoot_backend::TableEntry;
use std::collections::HashMap;
use std::time::Instant;

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
    OnboardingRelay,
    OnboardingReady,
    Post,
    Contacts,
    Unlock,
    SearchResults,
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
    pub search: SearchState,
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

pub struct SearchState {
    pub query: String,
    pub last_executed_query: String,
    pub results: Vec<TableEntry>,
    pub last_query_time: Option<Instant>,
    pub selected_suggestion: Option<usize>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            last_executed_query: String::new(),
            results: Vec::new(),
            last_query_time: None,
            selected_suggestion: None,
        }
    }
}

impl SearchState {
    /// Whether the suggestion dropdown should be shown.
    pub fn has_suggestions(&self) -> bool {
        !self.query.is_empty() && !self.results.is_empty()
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.last_executed_query.clear();
        self.results.clear();
        self.last_query_time = None;
        self.selected_suggestion = None;
    }
}
