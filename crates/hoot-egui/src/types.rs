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
    Onboarding,
    OnboardingNewUser,
    OnboardingNewShowKey,
    OnboardingRelay,
    OnboardingReady,
    Post,
    Contacts,
    Unlock,
    SearchResults,
}

impl Page {
    /// Returns true for app pages that show the sidebar + search bar.
    /// Onboarding and unlock are full-screen flows without chrome.
    pub fn shows_chrome(&self) -> bool {
        matches!(
            self,
            Page::Inbox
                | Page::Drafts
                | Page::Starred
                | Page::Archived
                | Page::Trash
                | Page::Requests
                | Page::Junk
                | Page::Contacts
                | Page::Settings
                | Page::Post
                | Page::SearchResults
        )
    }
}

// for storing the state of different components and such.
#[derive(Default)]
pub struct HootState {
    pub add_account_window: HashMap<egui::Id, ui::add_account_window::AddAccountWindowState>,
    pub compose_window: HashMap<egui::Id, ui::compose_window::ComposeWindowState>,
    pub onboarding: ui::onboarding::OnboardingState,
    pub settings: ui::settings::SettingsState,
    pub unlock_window: HashMap<egui::Id, ui::unlock_window::UnlockWindowState>,
    pub contacts: ContactsPageState,
    pub requests: RequestsPageState,
    pub search: SearchState,
    pub thread_view: ThreadViewState,
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

#[derive(Default)]
pub struct ThreadViewState {
    pub reply_post_id: String,
    pub reply_compose: Option<ui::compose_window::ComposeWindowState>,
}

#[derive(Debug, PartialEq)]
pub enum HootStatus {
    PreUnlock,
    WaitingForUnlock,
    Initializing,
    Ready,
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub last_executed_query: String,
    pub results: Vec<TableEntry>,
    pub last_query_time: Option<Instant>,
    pub selected_suggestion: Option<usize>,
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
