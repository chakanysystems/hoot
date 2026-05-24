#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use eframe::egui::{self, Color32, FontDefinitions, FontId, Frame, Margin, RichText, Sense};
use egui::FontFamily::Proportional;
use hoot_backend::profile_metadata::ProfileOption;
use hoot_backend::{
    AccountSummary, BackendEvent, DraftDto, HootBackend, InitialSnapshot, Mailbox, TableEntry,
};
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "profiling")]
use tracing::debug;
use tracing::{error, info, Level};

mod image_loader;
mod profile_metadata;
mod style;
mod types;
mod ui;
pub use types::*;
use ui::contacts::ContactsManager;

fn main() -> Result<(), eframe::Error> {
    let (non_blocking, _guard) = tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_max_level(Level::DEBUG)
        .init();

    #[cfg(feature = "profiling")]
    start_puffin_server();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 600.0])
            .with_transparent(false),
        ..Default::default()
    };

    eframe::run_native(
        "Hoot",
        options,
        Box::new(|cc| {
            style::apply_theme(&cc.egui_ctx);
            let mut fonts = FontDefinitions::default();
            fonts.font_data.insert(
                "InstrumentSans".to_owned(),
                std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                    "../assets/InstrumentSans-VariableFont_wdth,wght.ttf"
                ))),
            );
            fonts.font_data.insert(
                "Inter".to_owned(),
                std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                    "../assets/Inter.ttf"
                ))),
            );
            fonts
                .families
                .get_mut(&Proportional)
                .unwrap()
                .insert(0, "InstrumentSans".to_owned());
            fonts
                .families
                .get_mut(&Proportional)
                .unwrap()
                .insert(1, "Inter".to_owned());
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(Hoot::new(cc)))
        }),
    )
}

pub struct Hoot {
    pub page: Page,
    pub focused_post: String,
    pub show_trashed_post: bool,
    pub status: HootStatus,
    pub state: HootState,
    pub backend: Arc<HootBackend>,
    pub accounts: Vec<AccountSummary>,
    pub active_account_pubkey: Option<String>,
    pub table_entries: Vec<TableEntry>,
    pub trash_entries: Vec<TableEntry>,
    pub request_entries: Vec<TableEntry>,
    pub junk_entries: Vec<TableEntry>,
    pub profile_metadata: HashMap<String, ProfileOption>,
    pub contacts_manager: ContactsManager,
    pub drafts: Vec<DraftDto>,
}

fn get_account_display_text(app: &Hoot) -> String {
    app.active_account()
        .map(get_account_summary_display_text)
        .unwrap_or_else(|| "Select Account".to_string())
}

fn get_account_summary_display_text(account: &AccountSummary) -> String {
    if let Some(name) = &account.display_name {
        if !name.is_empty() {
            return name.clone();
        }
    }
    if !account.npub.is_empty() {
        if account.npub.len() > 16 {
            format!("{}...", &account.npub[..16])
        } else {
            account.npub.clone()
        }
    } else {
        account.pubkey_hex.clone()
    }
}

fn render_nav_item(ui: &mut egui::Ui, label: &str, is_selected: bool) -> egui::Response {
    let desired_size = egui::vec2(ui.available_width(), 30.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if is_selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(6), style::ACCENT_LIGHT);
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            Color32::from_rgba_premultiplied(149, 117, 205, 20),
        );
    }

    ui.painter().text(
        rect.left_center() + egui::vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        if is_selected {
            style::ACCENT
        } else {
            ui.visuals().text_color()
        },
    );

    response
}

fn render_left_panel(app: &mut Hoot, ctx: &egui::Context) {
    egui::SidePanel::left("left_panel")
        .default_width(style::SIDEBAR_WIDTH)
        .frame(
            Frame::none()
                .fill(style::SIDEBAR_BG)
                .inner_margin(Margin::symmetric(16, 12)),
        )
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Hoot")
                        .size(22.0)
                        .strong()
                        .color(style::ACCENT),
                );
                ui.add_space(16.0);

                let compose_width = ui.available_width();
                if ui
                    .add_sized(
                        [compose_width, 38.0],
                        egui::Button::new(
                            RichText::new("✉ Compose").color(Color32::WHITE).size(14.0),
                        )
                        .fill(style::ACCENT)
                        .corner_radius(8),
                    )
                    .clicked()
                {
                    let state = ui::compose_window::ComposeWindowState {
                        subject: String::new(),
                        to_field: String::new(),
                        content: String::new(),
                        parent_event_ids: Vec::new(),
                        selected_account_pubkey: app.active_account_pubkey.clone(),
                        selected_nip05: None,
                        minimized: false,
                        draft_id: None,
                        send_status: None,
                    };
                    app.state
                        .compose_window
                        .insert(egui::Id::new(rand::random::<u32>()), state);
                }

                ui.add_space(16.0);

                let nav_items: Vec<(&str, Page, usize)> = vec![
                    ("📥 Inbox", Page::Inbox, app.table_entries.len()),
                    ("📝 Drafts", Page::Drafts, app.drafts.len()),
                    ("⭐ Starred", Page::Starred, 0),
                    ("📁 Archived", Page::Archived, 0),
                    ("🗑 Trash", Page::Trash, app.trash_entries.len()),
                    ("📬 Requests", Page::Requests, app.request_entries.len()),
                    ("🚫 Junk", Page::Junk, app.junk_entries.len()),
                ];

                for (label, page, count) in &nav_items {
                    let text = if *count > 0 {
                        format!("{} {}", label, count)
                    } else {
                        label.to_string()
                    };
                    let is_selected = app.page == *page;
                    if render_nav_item(ui, &text, is_selected).clicked() {
                        app.page = page.clone();
                    }
                }

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                if render_nav_item(ui, "👤 Contacts", app.page == Page::Contacts).clicked() {
                    app.page = Page::Contacts;
                }

                ui.add_space(8.0);

                if app.accounts.is_empty() {
                    if ui.button("onboarding").clicked() {
                        app.page = Page::OnboardingNewUser;
                    }
                } else if ui.button("+ Add Account").clicked() {
                    let state = ui::add_account_window::AddAccountWindowState::default();
                    app.state
                        .add_account_window
                        .insert(egui::Id::new(rand::random::<u32>()), state);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(8.0);

                    if !app.accounts.is_empty() {
                        ui.label(
                            RichText::new("Account:")
                                .size(10.0)
                                .color(style::TEXT_MUTED),
                        );
                        egui::ComboBox::from_id_salt("sidebar_account_selector")
                            .selected_text(get_account_display_text(app))
                            .width(ui.available_width() - 8.0)
                            .show_ui(ui, |ui| {
                                for account in app.accounts.clone() {
                                    let display_text = get_account_summary_display_text(&account);
                                    let is_selected = app.active_account_pubkey.as_deref()
                                        == Some(&account.pubkey_hex);
                                    if ui.selectable_label(is_selected, display_text).clicked() {
                                        match app
                                            .backend
                                            .set_active_account(Some(account.pubkey_hex.clone()))
                                        {
                                            Ok(()) => {
                                                app.active_account_pubkey = Some(account.pubkey_hex)
                                            }
                                            Err(e) => error!("Failed to select account: {}", e),
                                        }
                                    }
                                }
                            });
                    }

                    ui.add_space(4.0);

                    if ui.add_sized([32.0, 32.0], egui::Button::new("⚙")).clicked() {
                        app.page = Page::Settings;
                    }
                });
            });
        });
}

fn render_app(app: &mut Hoot, ctx: &egui::Context) {
    let closed_account_windows: Vec<egui::Id> = app
        .state
        .add_account_window
        .keys()
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .filter(|&id| !ui::add_account_window::AddAccountWindow::show_window(app, ctx, id))
        .collect();
    for id in closed_account_windows {
        app.state.add_account_window.remove(&id);
    }

    let closed_compose_windows: Vec<egui::Id> = app
        .state
        .compose_window
        .keys()
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .filter(|&id| !ui::compose_window::ComposeWindow::show_window(app, ctx, id))
        .collect();
    for id in closed_compose_windows {
        app.state.compose_window.remove(&id);
    }

    match app.page {
        Page::Unlock => {}
        Page::Onboarding
        | Page::OnboardingNewUser
        | Page::OnboardingNewShowKey
        | Page::OnboardingReturning => {}
        _ => render_left_panel(app, ctx),
    }

    match app.page {
        Page::Unlock
        | Page::Onboarding
        | Page::OnboardingNewUser
        | Page::OnboardingNewShowKey
        | Page::OnboardingReturning => {}
        _ => ui::search::render_global_search_bar(app, ctx),
    }

    egui::CentralPanel::default().show(ctx, |ui| match app.page {
        Page::Inbox => ui::inbox::render(app, ui),
        Page::Post => ui::thread_view::render(app, ui),
        Page::Drafts => ui::drafts_page::render(app, ui),
        Page::Trash => ui::trash::render(app, ui),
        Page::Requests => ui::requests::render(app, ui),
        Page::Junk => ui::junk::render(app, ui),
        Page::Contacts => ui::contacts::render_contacts_page(app, ui),
        Page::Settings => ui::settings::SettingsScreen::ui(app, ui),
        Page::SearchResults => ui::search::render_search_results(app, ui),
        Page::Unlock => ui::unlock_database::UnlockDatabase::ui(app, ui),
        Page::Onboarding
        | Page::OnboardingNewUser
        | Page::OnboardingNewShowKey
        | Page::OnboardingReturning => ui::onboarding::OnboardingScreen::ui(app, ui),
        _ => {
            ui.heading("This hasn't been implemented yet.");
        }
    });
}

impl Hoot {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let storage_dir = eframe::storage_dir(hoot_backend::STORAGE_NAME).unwrap();
        let backend = HootBackend::open(storage_dir.to_string_lossy().to_string())
            .unwrap_or_else(|e| panic!("Backend initialization failed: {}", e));
        let wake_ctx = cc.egui_ctx.clone();
        if let Err(e) = backend.set_wake_callback(Arc::new(move || wake_ctx.request_repaint())) {
            error!("Failed to install backend wake callback: {}", e);
        }

        let page = if backend.onboarding_complete().unwrap_or(false) {
            Page::Unlock
        } else {
            Page::Onboarding
        };

        Self {
            page,
            focused_post: String::new(),
            show_trashed_post: false,
            status: HootStatus::PreUnlock,
            state: Default::default(),
            backend,
            accounts: Vec::new(),
            active_account_pubkey: None,
            table_entries: Vec::new(),
            trash_entries: Vec::new(),
            request_entries: Vec::new(),
            junk_entries: Vec::new(),
            profile_metadata: HashMap::new(),
            contacts_manager: ContactsManager::new(),
            drafts: Vec::new(),
        }
    }

    fn apply_snapshot(&mut self, snapshot: InitialSnapshot) {
        self.accounts = snapshot.accounts;
        self.active_account_pubkey = self
            .accounts
            .iter()
            .find(|account| account.is_active)
            .map(|account| account.pubkey_hex.clone())
            .or_else(|| {
                self.accounts
                    .first()
                    .map(|account| account.pubkey_hex.clone())
            });
        self.table_entries = snapshot.inbox;
        self.trash_entries = snapshot.trash;
        self.request_entries = snapshot.requests;
        self.junk_entries = snapshot.junk;
        self.drafts = snapshot.drafts;
        self.contacts_manager
            .set_contacts(snapshot.contacts, &mut self.profile_metadata);
    }

    fn update_backend(&mut self) {
        match self.status {
            HootStatus::PreUnlock => {
                info!("Requesting Database Unlock before proceeding.");
                self.status = HootStatus::WaitingForUnlock;
                let _ = self
                    .backend
                    .add_relay("wss://relay.chakany.systems".to_string());
                let _ = self.backend.add_relay("wss://talon.quest".to_string());
            }
            HootStatus::WaitingForUnlock => {
                let _ = self.backend.tick();
            }
            HootStatus::Initializing => {
                info!("Initializing Hoot...");
                match self.backend.initialize() {
                    Ok(snapshot) => {
                        self.apply_snapshot(snapshot);
                        self.status = HootStatus::Ready;
                        info!("Hoot Ready");
                    }
                    Err(e) => error!("Failed to initialize backend: {}", e),
                }
            }
            HootStatus::Ready => match self.backend.tick() {
                Ok(events) => self.handle_backend_events(events),
                Err(e) => error!("Backend tick failed: {}", e),
            },
        }
    }

    fn handle_backend_events(&mut self, events: Vec<BackendEvent>) {
        for event in events {
            match event {
                BackendEvent::MailboxesChanged => {
                    self.refresh_inbox();
                    self.refresh_trash();
                    self.refresh_requests();
                    self.refresh_junk();
                }
                BackendEvent::AccountsChanged => self.refresh_accounts(),
                BackendEvent::NIp05ResultsChanged => {}
                BackendEvent::RelayStatusesChanged => {}
            }
        }
    }

    pub fn refresh_accounts(&mut self) {
        match self.backend.list_accounts() {
            Ok(accounts) => {
                self.accounts = accounts;
                self.active_account_pubkey = self
                    .accounts
                    .iter()
                    .find(|account| account.is_active)
                    .map(|account| account.pubkey_hex.clone())
                    .or_else(|| {
                        self.accounts
                            .first()
                            .map(|account| account.pubkey_hex.clone())
                    });
            }
            Err(e) => error!("Failed to load accounts: {}", e),
        }
    }

    pub fn refresh_drafts(&mut self) {
        match self.backend.list_drafts() {
            Ok(drafts) => self.drafts = drafts,
            Err(e) => error!("Failed to load drafts: {}", e),
        }
    }

    pub fn refresh_inbox(&mut self) {
        match self.backend.list_messages(Mailbox::Inbox) {
            Ok(entries) => self.table_entries = entries,
            Err(e) => error!("Could not refresh inbox: {}", e),
        }
    }

    pub fn refresh_trash(&mut self) {
        match self.backend.list_messages(Mailbox::Trash) {
            Ok(entries) => self.trash_entries = entries,
            Err(e) => error!("Failed to load trash entries: {}", e),
        }
    }

    pub fn refresh_requests(&mut self) {
        match self.backend.list_messages(Mailbox::Requests) {
            Ok(entries) => self.request_entries = entries,
            Err(e) => error!("Failed to load request entries: {}", e),
        }
    }

    pub fn refresh_junk(&mut self) {
        match self.backend.list_messages(Mailbox::Junk) {
            Ok(entries) => self.junk_entries = entries,
            Err(e) => error!("Failed to load junk entries: {}", e),
        }
    }

    pub fn refresh_contacts(&mut self) {
        match self.backend.list_contacts() {
            Ok(contacts) => self
                .contacts_manager
                .set_contacts(contacts, &mut self.profile_metadata),
            Err(e) => error!("Failed to load contacts: {}", e),
        }
    }

    pub fn active_account(&self) -> Option<&AccountSummary> {
        self.active_account_pubkey.as_ref().and_then(|pubkey| {
            self.accounts
                .iter()
                .find(|account| &account.pubkey_hex == pubkey)
        })
    }

    pub fn resolve_name(&self, pubkey: &str) -> Option<String> {
        if let Some(petname) = self.contacts_manager.find_petname(pubkey) {
            return Some(petname.to_string());
        }
        if let Some(ProfileOption::Some(meta)) = self.profile_metadata.get(pubkey) {
            if let Some(display_name) = &meta.display_name {
                return Some(display_name.clone());
            }
            if let Some(name) = &meta.name {
                return Some(name.clone());
            }
        }
        None
    }
}

impl eframe::App for Hoot {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_backend();
        self.contacts_manager.process_image_queue(ctx);
        render_app(self, ctx);
    }
}

#[cfg(feature = "profiling")]
fn start_puffin_server() {
    puffin::set_scopes_on(true);

    match puffin_http::Server::new("127.0.0.1:8585") {
        Ok(puffin_server) => {
            debug!("Run: cargo install puffin_viewer && puffin_viewer --url 127.0.0.1:8585");

            std::process::Command::new("puffin_viewer")
                .arg("--url")
                .arg("127.0.0.1:8585")
                .spawn()
                .ok();

            #[allow(clippy::mem_forget)]
            std::mem::forget(puffin_server);
        }
        Err(err) => {
            error!("Failed to start puffin server: {}", err);
        }
    };
}
