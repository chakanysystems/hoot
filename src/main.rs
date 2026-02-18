#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use eframe::egui::{self, Color32, FontDefinitions, FontId, Frame, Margin, RichText, Sense};
use egui::FontFamily::Proportional;
use std::collections::HashMap;
use tracing::{debug, error, info, Level};

mod account_manager;
mod db;
mod error;
mod event_processing;
mod image_loader;
mod mail_event;
mod profile_metadata;
use profile_metadata::ProfileOption;
mod relay;
mod style;
mod types;
mod ui;
pub use types::*;
use ui::contacts::ContactsManager;

fn main() -> Result<(), eframe::Error> {
    let (non_blocking, _guard) = tracing_appender::non_blocking(std::io::stdout()); // add log files in prod one day
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_max_level(Level::DEBUG)
        .init();

    #[cfg(feature = "profiling")]
    start_puffin_server();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Hoot",
        options,
        Box::new(|cc| {
            style::apply_theme(&cc.egui_ctx);
            let mut fonts = FontDefinitions::default();
            fonts.font_data.insert(
                "Inter".to_owned(),
                std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/Inter.ttf"))),
            );
            fonts
                .families
                .get_mut(&Proportional)
                .unwrap()
                .insert(0, "Inter".to_owned());
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(Hoot::new(cc)))
        }),
    )
}

pub struct Hoot {
    pub page: Page,
    focused_post: String,
    show_trashed_post: bool,
    status: HootStatus,
    state: HootState,
    relays: relay::RelayPool,
    events: Vec<nostr::Event>,
    account_manager: account_manager::AccountManager,
    pub active_account: Option<nostr::Keys>,
    db: db::Db,
    table_entries: Vec<TableEntry>,
    trash_entries: Vec<TableEntry>,
    request_entries: Vec<TableEntry>,
    junk_entries: Vec<TableEntry>,
    profile_metadata: HashMap<String, profile_metadata::ProfileOption>,
    pub contacts_manager: ContactsManager,
    drafts: Vec<db::Draft>,
}

fn get_account_display_text(app: &Hoot) -> String {
    if let Some(key) = &app.active_account {
        get_key_display_text(app, key)
    } else {
        "Select Account".to_string()
    }
}

fn get_key_display_text(app: &Hoot, key: &nostr::Keys) -> String {
    let pubkey = key.public_key().to_string();
    if let Some(name) = app.resolve_name(&pubkey) {
        return name;
    }
    // Fallback: truncated npub
    use nostr::ToBech32;
    let npub = key
        .public_key()
        .to_bech32()
        .unwrap_or_else(|_| pubkey.clone());
    if npub.len() > 16 {
        format!("{}...", &npub[..16])
    } else {
        npub
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

                // Compose button — full width, accent fill, white text
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
                        parent_events: Vec::new(),
                        selected_account: None,
                        minimized: false,
                        draft_id: None,
                    };
                    app.state
                        .compose_window
                        .insert(egui::Id::new(rand::random::<u32>()), state);
                }

                ui.add_space(16.0);

                // Navigation items
                let nav_items: Vec<(&str, Page, usize)> = vec![
                    ("📥 Inbox", Page::Inbox, app.events.len()),
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

                // Contacts
                if render_nav_item(ui, "👤 Contacts", app.page == Page::Contacts).clicked() {
                    app.page = Page::Contacts;
                }

                ui.add_space(8.0);

                // Show onboarding for first-time users, or Add Account button for existing users
                if app.account_manager.loaded_keys.is_empty() {
                    if ui.button("onboarding").clicked() {
                        app.page = Page::OnboardingNewUser;
                    }
                } else {
                    if ui.button("+ Add Account").clicked() {
                        let state = ui::add_account_window::AddAccountWindowState::default();
                        app.state
                            .add_account_window
                            .insert(egui::Id::new(rand::random::<u32>()), state);
                    }
                }

                // Push account selector + settings to bottom
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(8.0);

                    if !app.account_manager.loaded_keys.is_empty() {
                        ui.label(
                            RichText::new("Account:")
                                .size(10.0)
                                .color(style::TEXT_MUTED),
                        );
                        egui::ComboBox::from_id_salt("sidebar_account_selector")
                            .selected_text(get_account_display_text(app))
                            .width(ui.available_width() - 8.0)
                            .show_ui(ui, |ui| {
                                for key in &app.account_manager.loaded_keys.clone() {
                                    let display_text = get_key_display_text(app, key);
                                    let is_selected =
                                        app.active_account.as_ref().map(|k| k.public_key())
                                            == Some(key.public_key());
                                    if ui.selectable_label(is_selected, display_text).clicked() {
                                        app.active_account = Some(key.clone());
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
    // Render add account windows, collecting closed ones for removal
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

    // Render compose windows, collecting closed ones for removal
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

    egui::CentralPanel::default().show(ctx, |ui| match app.page {
        Page::Inbox => ui::inbox::render(app, ui),
        Page::Post => ui::thread_view::render(app, ui),
        Page::Drafts => ui::drafts_page::render(app, ui),
        Page::Trash => ui::trash::render(app, ui),
        Page::Requests => ui::requests::render(app, ui),
        Page::Junk => ui::junk::render(app, ui),
        Page::Contacts => ui::contacts::render_contacts_page(app, ui),
        Page::Settings => ui::settings::SettingsScreen::ui(app, ui),
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

// it's just to determine where to store files and also for keystorage paths and such
// y'know?????
#[cfg(debug_assertions)]
pub const STORAGE_NAME: &'static str = "systems.chakany.hoot-dev";
#[cfg(not(debug_assertions))]
pub const STORAGE_NAME: &'static str = "systems.chakany.hoot";

impl Hoot {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Create storage directory if it doesn't exist
        let storage_dir = eframe::storage_dir(STORAGE_NAME).unwrap();
        std::fs::create_dir_all(&storage_dir).unwrap();

        // Create the database file path
        let db_path = storage_dir.join("hoot.db");

        // Initialize the database
        let db = match db::Db::new(db_path.clone()) {
            Ok(db) => {
                info!("Database initialized successfully");
                db
            }
            Err(e) => {
                error!("Failed to initialize database: {}", e);
                panic!("Database initialization failed: {}", e);
            }
        };

        // check if this is our first time loading
        let page = match std::fs::exists(storage_dir.join("done")) {
            Ok(true) => Page::Unlock,
            Ok(false) => Page::Onboarding,
            Err(e) => panic!("Couldn't check if we have already setup: {}", e),
        };

        Self {
            page,
            focused_post: String::new(),
            show_trashed_post: false,
            status: HootStatus::PreUnlock,
            state: Default::default(),
            relays: relay::RelayPool::new(),
            events: Vec::new(),
            account_manager: account_manager::AccountManager::new(),
            active_account: None,
            db,
            table_entries: Vec::new(),
            trash_entries: Vec::new(),
            request_entries: Vec::new(),
            junk_entries: Vec::new(),
            profile_metadata: HashMap::new(),
            contacts_manager: ContactsManager::new(),
            drafts: Vec::new(),
        }
    }

    fn refresh_drafts(&mut self) {
        match self.db.get_drafts() {
            Ok(drafts) => self.drafts = drafts,
            Err(e) => error!("Failed to load drafts: {}", e),
        }
    }

    fn refresh_trash(&mut self) {
        match self.db.get_trash_messages() {
            Ok(entries) => self.trash_entries = entries,
            Err(e) => error!("Failed to load trash entries: {}", e),
        }
    }

    fn refresh_requests(&mut self) {
        match self.db.get_request_messages() {
            Ok(entries) => self.request_entries = entries,
            Err(e) => error!("Failed to load request entries: {}", e),
        }
    }

    fn refresh_junk(&mut self) {
        match self.db.get_junk_messages() {
            Ok(entries) => self.junk_entries = entries,
            Err(e) => error!("Failed to load junk entries: {}", e),
        }
    }

    /// Update the gift-wrap subscription to include all loaded accounts.
    pub fn update_gift_wrap_subscription(&mut self) {
        if self.account_manager.loaded_keys.is_empty() {
            return;
        }

        let public_keys: Vec<nostr::PublicKey> = self
            .account_manager
            .loaded_keys
            .iter()
            .map(|k| k.public_key())
            .collect();

        let filter = nostr::Filter::new().kind(nostr::Kind::GiftWrap).custom_tag(
            nostr::SingleLetterTag {
                character: nostr::Alphabet::P,
                uppercase: false,
            },
            public_keys,
        );

        let mut gw_sub = relay::Subscription::default();
        gw_sub.filter(filter);

        match self.relays.add_subscription(gw_sub) {
            Ok(_) => debug!("Updated gift-wrap subscription"),
            Err(e) => error!("Failed to update gift-wrap subscription: {}", e),
        }
    }

    /// Resolve the best display name for a pubkey: petname > display_name > name > pubkey.
    fn resolve_name(&self, pubkey: &str) -> Option<String> {
        // Check contacts for petname first
        if let Some(petname) = self.contacts_manager.find_petname(pubkey) {
            return Some(petname.to_string());
        }
        // Fall back to profile metadata
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
        event_processing::update_app(self, ctx);
        render_app(self, ctx);
    }
}

#[cfg(feature = "profiling")]
fn start_puffin_server() {
    puffin::set_scopes_on(true); // tell puffin to collect data

    match puffin_http::Server::new("127.0.0.1:8585") {
        Ok(puffin_server) => {
            debug!("Run: cargo install puffin_viewer && puffin_viewer --url 127.0.0.1:8585");

            std::process::Command::new("puffin_viewer")
                .arg("--url")
                .arg("127.0.0.1:8585")
                .spawn()
                .ok();

            // We can store the server if we want, but in this case we just want
            // it to keep running. Dropping it closes the server, so let's not drop it!
            #[allow(clippy::mem_forget)]
            std::mem::forget(puffin_server);
        }
        Err(err) => {
            error!("Failed to start puffin server: {}", err);
        }
    };
}
