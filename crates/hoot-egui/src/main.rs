#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use eframe::egui::{self, Color32, FontDefinitions, FontId, Frame, Margin, RichText, Sense};
use egui::FontFamily::Proportional;
use hoot_backend::{
    AccountSummary, BackendEvent, DraftDto, HootBackend, InitialSnapshot, Mailbox, ProfileOption,
    TableEntry,
};
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "profiling")]
use tracing::debug;
use tracing::{error, info, Level};

mod app_controller;
mod image_loader;
mod profile_metadata;
mod style;
mod types;
mod ui;
pub use app_controller::{sender_decision_refresh_plan, SenderDecision};
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
            .with_min_inner_size([600.0, 400.0])
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

fn render_nav_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    is_selected: bool,
) -> egui::Response {
    render_nav_item_with_badge(ui, icon, label, is_selected, 0)
}

fn render_nav_item_with_badge(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    is_selected: bool,
    badge_count: usize,
) -> egui::Response {
    let desired_size = egui::vec2(ui.available_width(), 30.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if is_selected {
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + 4.0),
            egui::vec2(3.0, rect.height() - 8.0),
        );
        ui.painter()
            .rect_filled(bar_rect, egui::CornerRadius::same(2), style::ACCENT);
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), style::accent_soft());
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), style::SURFACE2);
    }

    let text_color = if is_selected {
        style::ACCENT
    } else if response.hovered() {
        style::TEXT
    } else {
        style::TEXT2
    };

    let badge_reserved = if badge_count > 0 { 34.0 } else { 0.0 };
    let icon_x = rect.left() + 10.0;
    ui.painter().text(
        egui::Pos2::new(icon_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        FontId::proportional(13.0),
        text_color,
    );

    let text_x = icon_x + 15.0 + 6.0;
    let max_text_w = (rect.right() - badge_reserved - text_x - 4.0).max(0.0);
    let galley = ui.painter().layout(
        label.to_string(),
        FontId::proportional(13.5),
        text_color,
        max_text_w,
    );
    ui.painter().galley(
        egui::Pos2::new(text_x, rect.center().y - galley.size().y / 2.0),
        galley,
        text_color,
    );

    if badge_count > 0 {
        let badge_text = if badge_count > 99 {
            "99+".to_string()
        } else {
            badge_count.to_string()
        };
        let badge_font = FontId::proportional(10.0);
        let bgalley =
            ui.painter()
                .layout_no_wrap(badge_text.clone(), badge_font.clone(), Color32::WHITE);
        let badge_w = (bgalley.size().x + 8.0).max(18.0);
        let badge_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - badge_w / 2.0 - 4.0, rect.center().y),
            egui::vec2(badge_w, 16.0),
        );
        ui.painter()
            .rect_filled(badge_rect, egui::CornerRadius::same(8), style::ACCENT);
        ui.painter().text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            badge_text,
            badge_font,
            Color32::WHITE,
        );
    }

    response
}

fn render_left_panel(app: &mut Hoot, ctx: &egui::Context) {
    egui::SidePanel::left("left_panel")
        .exact_width(style::SIDEBAR_WIDTH)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(style::SURFACE)
                .inner_margin(Margin::same(0)),
        )
        .show(ctx, |ui| {
            ui.set_min_height(ui.available_height());
            let panel_rect = ui.max_rect();

            let top_frame = Frame::new()
                .inner_margin(Margin {
                    left: 16,
                    right: 16,
                    top: 18,
                    bottom: 14,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Hoot").size(17.0).strong().color(style::TEXT));
                });
            let bottom_y = top_frame.response.rect.bottom();
            ui.painter().line_segment(
                [
                    egui::pos2(panel_rect.left(), bottom_y),
                    egui::pos2(panel_rect.right(), bottom_y),
                ],
                egui::Stroke::new(1.0, style::border()),
            );

            egui::TopBottomPanel::bottom("sidebar_footer")
                .exact_height(50.0)
                .frame(
                    Frame::new()
                        .fill(style::SURFACE)
                        .inner_margin(Margin::same(0)),
                )
                .show_inside(ui, |ui| {
                    let r = ui.max_rect();
                    ui.painter().line_segment(
                        [r.left_top(), r.right_top()],
                        egui::Stroke::new(1.0, style::border()),
                    );
                    let content_rect = egui::Rect::from_min_size(
                        egui::Pos2::new(r.left() + 12.0, r.top() + 10.0),
                        egui::Vec2::new(r.width() - 24.0, 30.0),
                    );
                    let mut footer_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
                    footer_ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let (dot_rect, _) =
                            ui.allocate_exact_size(egui::vec2(7.0, 7.0), Sense::hover());
                        ui.painter()
                            .circle_filled(dot_rect.center(), 3.5, style::GREEN);

                        let chip_w = (ui.available_width() - 30.0 - 8.0).max(20.0);
                        let (chip_rect, chip_resp) =
                            ui.allocate_exact_size(egui::Vec2::new(chip_w, 30.0), Sense::click());
                        let chip_resp = chip_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                        ui.painter().rect_filled(
                            chip_rect,
                            egui::CornerRadius::same(7),
                            style::SURFACE2,
                        );
                        ui.painter().rect_stroke(
                            chip_rect,
                            egui::CornerRadius::same(7),
                            egui::Stroke::new(1.0, style::border_strong()),
                            egui::StrokeKind::Inside,
                        );
                        let account_text = get_account_display_text(app);
                        let galley = ui.painter().layout(
                            account_text,
                            FontId::proportional(12.0),
                            style::TEXT2,
                            (chip_w - 16.0).max(0.0),
                        );
                        ui.painter().galley(
                            egui::Pos2::new(
                                chip_rect.left() + 8.0,
                                chip_rect.center().y - galley.size().y / 2.0,
                            ),
                            galley,
                            style::TEXT2,
                        );
                        if chip_resp.clicked() && !app.accounts.is_empty() {
                            let next_pubkey = match app.active_account_pubkey.as_deref() {
                                Some(current) => app
                                    .accounts
                                    .iter()
                                    .position(|account| account.pubkey_hex == current)
                                    .map(|idx| {
                                        app.accounts[(idx + 1) % app.accounts.len()]
                                            .pubkey_hex
                                            .clone()
                                    }),
                                None => app
                                    .accounts
                                    .first()
                                    .map(|account| account.pubkey_hex.clone()),
                            };
                            if let Some(pubkey) = next_pubkey {
                                app.select_account(pubkey);
                            }
                        }

                        if style::pointer(
                            ui.add(
                                egui::Button::new(
                                    RichText::new("⚙").size(14.0).color(style::TEXT2),
                                )
                                .fill(style::SURFACE2)
                                .stroke(egui::Stroke::new(1.0, style::border_strong()))
                                .corner_radius(egui::CornerRadius::same(7))
                                .min_size(egui::Vec2::new(30.0, 30.0)),
                            ),
                        )
                        .clicked()
                        {
                            app.page = Page::Settings;
                        }
                    });
                });

            egui::ScrollArea::vertical()
                .id_salt("sidebar_scroll")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    Frame::new()
                        .inner_margin(Margin {
                            left: 8,
                            right: 8,
                            top: 0,
                            bottom: 0,
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            ui.add_space(12.0);
                            let compose_resp = style::pointer(
                                ui.add_sized(
                                    [ui.available_width(), 38.0],
                                    egui::Button::new(
                                        RichText::new("  ✏  Compose")
                                            .color(Color32::WHITE)
                                            .size(13.5),
                                    )
                                    .fill(style::ACCENT)
                                    .corner_radius(egui::CornerRadius::same(10)),
                                ),
                            );
                            if compose_resp.clicked() {
                                let state = ui::compose_window::ComposeWindowState {
                                    subject: String::new(),
                                    to_input: String::new(),
                                    recipients: Vec::new(),
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
                            ui.add_space(4.0);

                            if render_nav_item_with_badge(
                                ui,
                                "✉",
                                "Inbox",
                                app.page == Page::Inbox,
                                app.table_entries.len(),
                            )
                            .clicked()
                            {
                                app.page = Page::Inbox;
                            }
                            if render_nav_item_with_badge(
                                ui,
                                "✏",
                                "Drafts",
                                app.page == Page::Drafts,
                                app.drafts.len(),
                            )
                            .clicked()
                            {
                                app.page = Page::Drafts;
                            }
                            if render_nav_item(ui, "★", "Starred", app.page == Page::Starred)
                                .clicked()
                            {
                                app.page = Page::Starred;
                            }
                            if render_nav_item(ui, "☰", "Archived", app.page == Page::Archived)
                                .clicked()
                            {
                                app.page = Page::Archived;
                            }
                            if render_nav_item_with_badge(
                                ui,
                                "⊗",
                                "Trash",
                                app.page == Page::Trash,
                                app.trash_entries.len(),
                            )
                            .clicked()
                            {
                                app.page = Page::Trash;
                            }

                            ui.add_space(4.0);
                            let (div_rect, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 1.0),
                                Sense::hover(),
                            );
                            ui.painter().line_segment(
                                [div_rect.left_center(), div_rect.right_center()],
                                egui::Stroke::new(1.0, style::border()),
                            );
                            ui.add_space(4.0);

                            if render_nav_item_with_badge(
                                ui,
                                "◌",
                                "Requests",
                                app.page == Page::Requests,
                                app.request_entries.len(),
                            )
                            .clicked()
                            {
                                app.page = Page::Requests;
                            }
                            if render_nav_item_with_badge(
                                ui,
                                "⊘",
                                "Junk",
                                app.page == Page::Junk,
                                app.junk_entries.len(),
                            )
                            .clicked()
                            {
                                app.page = Page::Junk;
                            }
                            if render_nav_item(ui, "◎", "Contacts", app.page == Page::Contacts)
                                .clicked()
                            {
                                app.page = Page::Contacts;
                            }
                            if render_nav_item(ui, "⚙", "Settings", app.page == Page::Settings)
                                .clicked()
                            {
                                app.page = Page::Settings;
                            }
                        });
                });

            let divider_x = panel_rect.right() - 0.5;
            ui.painter().line_segment(
                [
                    egui::pos2(divider_x, panel_rect.top()),
                    egui::pos2(divider_x, panel_rect.bottom()),
                ],
                egui::Stroke::new(1.0, style::border()),
            );
        });
}

fn sync_unlock_window_state(state: &mut HootState, page: &Page, id: egui::Id) -> bool {
    if page == &Page::Unlock {
        state.unlock_window.entry(id).or_default();
        true
    } else {
        state.unlock_window.remove(&id);
        false
    }
}

fn render_app(app: &mut Hoot, ctx: &egui::Context) {
    let unlock_window_id = egui::Id::new(ui::unlock_window::UNLOCK_WINDOW_ID);
    if sync_unlock_window_state(&mut app.state, &app.page, unlock_window_id)
        && !ui::unlock_window::UnlockWindow::show_window(app, ctx, unlock_window_id)
    {
        app.state.unlock_window.remove(&unlock_window_id);
    }

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

    if app.page.shows_left_panel() {
        render_left_panel(app, ctx);
    }

    if app.page.shows_global_search() {
        ui::search::render_global_search_bar(app, ctx);
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
        Page::Unlock => {
            ui.allocate_space(ui.available_size());
        }
        Page::Onboarding
        | Page::OnboardingNewUser
        | Page::OnboardingNewShowKey
        | Page::OnboardingRelay
        | Page::OnboardingReady => ui::onboarding::OnboardingScreen::ui(app, ui),
        _ => {
            ui.heading("This hasn't been implemented yet.");
        }
    });
}

impl Hoot {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
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
        self.active_account_pubkey = active_account_pubkey(&self.accounts);
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
                for relay_url in hoot_backend::default_relay_urls() {
                    self.add_relay_url((*relay_url).to_string());
                }
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
                self.active_account_pubkey = active_account_pubkey(&self.accounts);
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

fn active_account_pubkey(accounts: &[AccountSummary]) -> Option<String> {
    accounts
        .iter()
        .find(|account| account.is_active)
        .map(|account| account.pubkey_hex.clone())
        .or_else(|| accounts.first().map(|account| account.pubkey_hex.clone()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepting_sender_refreshes_requests_then_inbox() {
        let plan = sender_decision_refresh_plan(SenderDecision::Accept);

        assert_eq!(plan.len(), 2);
        assert!(matches!(plan[0], Mailbox::Requests));
        assert!(matches!(plan[1], Mailbox::Inbox));
    }

    #[test]
    fn rejecting_sender_refreshes_requests_then_junk() {
        let plan = sender_decision_refresh_plan(SenderDecision::Reject);

        assert_eq!(plan.len(), 2);
        assert!(matches!(plan[0], Mailbox::Requests));
        assert!(matches!(plan[1], Mailbox::Junk));
    }

    #[test]
    fn unlock_window_rendering_is_gated_by_unlock_page() {
        let id = egui::Id::new(ui::unlock_window::UNLOCK_WINDOW_ID);
        let mut state = HootState::default();

        assert!(!sync_unlock_window_state(&mut state, &Page::Inbox, id));
        assert!(!state.unlock_window.contains_key(&id));

        assert!(sync_unlock_window_state(&mut state, &Page::Unlock, id));
        assert!(state.unlock_window.contains_key(&id));

        assert!(!sync_unlock_window_state(&mut state, &Page::Inbox, id));
        assert!(!state.unlock_window.contains_key(&id));
    }
}
