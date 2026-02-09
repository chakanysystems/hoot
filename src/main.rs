#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use crate::mail_event::MAIL_EVENT_KIND;
use eframe::egui::{
    self, Color32, FontDefinitions, FontId, Frame, Margin, RichText, ScrollArea, Sense, Stroke,
    Vec2b,
};
use egui::FontFamily::Proportional;
use egui_extras::{Column, TableBuilder};
use nostr::{event::Kind, EventId, TagKind};
use std::collections::{HashMap, HashSet};
use std::panic;
use tracing::{debug, error, info, warn, Level};

mod account_manager;
mod db;
mod error;
mod image_loader;
mod mail_event;
mod profile_metadata;
use profile_metadata::{get_profile_metadata, ProfileMetadata, ProfileOption};
mod relay;
mod style;
mod ui;
use ui::contacts::ContactsManager;

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
                egui::FontData::from_static(include_bytes!("../fonts/Inter.ttf")),
            );
            fonts
                .families
                .get_mut(&Proportional)
                .unwrap()
                .insert(0, "Inter".to_owned());
            cc.egui_ctx.set_fonts(fonts);
            Box::new(Hoot::new(cc))
        }),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Inbox,
    Drafts,
    Starred,
    Archived,
    Trash,
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
    profile_metadata: HashMap<String, profile_metadata::ProfileOption>,
    pub contacts_manager: ContactsManager,
    drafts: Vec<db::Draft>,
}

#[derive(Debug, PartialEq)]
enum HootStatus {
    PreUnlock,
    WaitingForUnlock,
    Initializing,
    Ready,
}

fn try_recv_relay_message(app: &mut Hoot) {
    if let Some(raw) = app.relays.try_recv() {
        info!("{:?}", &raw);
        match relay::RelayMessage::from_json(&raw) {
            Ok(v) => process_message(app, &v),
            Err(e) => error!("could not decode message sent from relay: {}", e),
        }
    }
}

fn update_app(app: &mut Hoot, ctx: &egui::Context) {
    #[cfg(feature = "profiling")]
    puffin::profile_function!();
    let ctx = ctx.clone();
    let wake_ctx = ctx.clone();
    let wake_up = move || {
        wake_ctx.request_repaint();
    };

    if app.status == HootStatus::PreUnlock {
        info!("Requesting Database Unlock before proceeding.");
        app.status = HootStatus::WaitingForUnlock;
        let _ = app
            .relays
            .add_url("wss://relay.chakany.systems".to_string(), wake_up.clone());

        let _ = app
            .relays
            .add_url("wss://talon.quest".to_string(), wake_up.clone());

        app.relays.keepalive(wake_up);
        return;
    } else if app.status == HootStatus::WaitingForUnlock {
        // the unlock happens in the render_app function
        // we can't do anything but wait until HootStatus is Initializing
        app.relays.keepalive(wake_up);
        try_recv_relay_message(app);
        return;
    }

    if app.status == HootStatus::Initializing {
        info!("Initializing Hoot...");
        if let Err(e) = app.account_manager.load_keys(&app.db) {
            error!("something went wrong trying to load keys: {}", e);
        }

        if let Err(e) = app.db.purge_deleted_events() {
            error!("Failed to purge deleted events: {}", e);
        }

        let now = chrono::Utc::now().timestamp();
        if let Err(e) = app.db.purge_expired_trash(now) {
            error!("Failed to purge expired trash: {}", e);
        }

        match app.db.get_top_level_messages() {
            Ok(msgs) => app.table_entries = msgs,
            Err(e) => error!("Could not fetch table entries to display from DB: {}", e),
        }

        app.refresh_trash();

        if !app.account_manager.loaded_keys.is_empty() {
            app.update_gift_wrap_subscription();

            if let Err(e) = app
                .contacts_manager
                .load_from_db(&app.db, &mut app.profile_metadata)
            {
                error!("Failed to load contacts: {}", e);
            }
        }

        app.refresh_drafts();

        app.status = HootStatus::Ready;
        info!("Hoot Ready");
    }

    app.relays.keepalive(wake_up);
    try_recv_relay_message(app);
    app.contacts_manager.process_image_queue(&ctx);
}

fn process_message(app: &mut Hoot, msg: &relay::RelayMessage) {
    use relay::RelayMessage::*;
    match msg {
        Event(sub_id, event) => process_event(app, sub_id, event),
        Notice(msg) => debug!("Relay notice: {}", msg),
        OK(result) => debug!("Command result: {:?}", result),
        Eose(sub_id) => debug!("End of stored events for subscription {}", sub_id),
        Closed(sub_id, msg) => debug!("Subscription {} closed: {}", sub_id, msg),
    }
}

fn apply_deletions(
    app: &mut Hoot,
    event_ids: Vec<String>,
    author_pubkey: Option<&str>,
    source_event_id: Option<&str>,
) -> Result<(), anyhow::Error> {
    if event_ids.is_empty() {
        return Ok(());
    }

    let mut scoped_event_ids: Vec<String> = Vec::new();
    let mut unscoped_event_ids: Vec<String> = Vec::new();
    for event_id in event_ids {
        match app.db.get_event_kind_pubkey(&event_id) {
            Ok(Some((kind, pubkey))) => {
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
            Ok(None) => {}
            Err(e) => error!("Failed to load event {} metadata: {}", event_id, e),
        }
    }

    let mut apply_event_ids: Vec<String> = Vec::new();
    apply_event_ids.extend(scoped_event_ids.iter().cloned());
    apply_event_ids.extend(unscoped_event_ids.iter().cloned());

    if apply_event_ids.is_empty() {
        return Ok(());
    }

    if !scoped_event_ids.is_empty() {
        app.db
            .record_deletions(&scoped_event_ids, author_pubkey, source_event_id)?;
    }
    if !unscoped_event_ids.is_empty() {
        app.db
            .record_deletions(&unscoped_event_ids, None, source_event_id)?;
    }

    if let Err(e) = app.db.delete_from_trash(&apply_event_ids) {
        error!("Failed to remove deleted events from trash: {}", e);
    }

    let mut wrap_ids: Vec<String> = Vec::new();
    for event_id in &apply_event_ids {
        match app.db.get_wrap_ids_for_inner(event_id) {
            Ok(ids) => wrap_ids.extend(ids),
            Err(e) => error!("Failed to load gift wrap ids for {}: {}", event_id, e),
        }
    }
    if !wrap_ids.is_empty() {
        app.db
            .record_deletion_markers(&wrap_ids, source_event_id)?;
    }

    let mut removed_ids: HashSet<String> = apply_event_ids.into_iter().collect();
    for wrap_id in wrap_ids {
        removed_ids.insert(wrap_id);
    }
    if !removed_ids.is_empty() {
        app.events
            .retain(|ev| !removed_ids.contains(&ev.id.to_string()));
        if removed_ids.contains(&app.focused_post) {
            app.page = Page::Inbox;
            app.focused_post.clear();
            app.show_trashed_post = false;
        }
        match app.db.get_top_level_messages() {
            Ok(msgs) => app.table_entries = msgs,
            Err(e) => error!("Could not fetch table entries to display from DB: {}", e),
        }
        app.refresh_trash();
    }
    Ok(())
}

fn process_event(app: &mut Hoot, _sub_id: &str, event_json: &str) {
    #[cfg(feature = "profiling")]
    puffin::profile_function!();

    let event = match serde_json::from_str::<nostr::Event>(event_json) {
        Ok(event) => event,
        Err(_) => {
            error!("Failed to parse event JSON: {}", event_json);
            return;
        }
    };

    if event.verify().is_err() {
        error!("Event verification failed for event: {}", event.id);
        return;
    }
    debug!("Verified event: {:?}", event);

    if event.kind == Kind::EventDeletion {
        let event_ids: Vec<String> = event.tags.event_ids().map(|id| id.to_hex()).collect();
        if !event_ids.is_empty() {
            let author_pubkey = event.pubkey.to_string();
            let deletion_id = event.id.to_string();
            if let Err(e) = apply_deletions(
                app,
                event_ids,
                Some(author_pubkey.as_str()),
                Some(deletion_id.as_str()),
            ) {
                error!("Failed to apply deletion event {}: {}", event.id, e);
            }
        }
        return;
    }

    let event_id = event.id.to_string();
    let event_author = event.pubkey.to_string();
    if let Ok(true) = app.db.is_deleted(&event_id, Some(event_author.as_str())) {
        debug!("Skipping deleted event: {}", event.id);
        return;
    }
    if let Ok(true) = app.db.is_trashed(&event_id) {
        debug!("Skipping trashed event: {}", event.id);
        return;
    }

    if event.kind == Kind::Metadata {
        debug!("Got profile metadata");

        let deserialized_metadata: profile_metadata::ProfileMetadata =
            match serde_json::from_str(&event.content) {
                Ok(meta) => meta,
                Err(e) => {
                    error!("Invalid metadata event {}: {}", event.id, e);
                    return;
                }
            };
        app.profile_metadata.insert(
            event.pubkey.to_string(),
            ProfileOption::Some(deserialized_metadata.clone()),
        );
        app.contacts_manager
            .upsert_metadata(event.pubkey.to_string(), deserialized_metadata.clone());
        // TODO: evaluate perf cost of clone LOL
        match app.db.update_profile_metadata(event.clone()) {
            Ok(_) => { // wow who cares
            }
            Err(e) => error!("Error when saving profile metadata to DB: {}", e),
        }
        return;
    }

    if event.kind == Kind::GiftWrap {
        if let Ok(true) = app.db.gift_wrap_exists(&event.id.to_string()) {
            debug!("Skipping already stored gift wrap: {}", event.id);
            return;
        }
        if let Ok(true) = app.db.is_deleted(&event.id.to_string(), None) {
            debug!("Skipping deleted gift wrap: {}", event.id);
            return;
        }
        match app.account_manager.unwrap_gift_wrap(&event) {
            Ok(unwrapped) => {
                if unwrapped.sender != unwrapped.rumor.pubkey {
                    warn!("Gift wrap seal signer mismatch for event {}", event.id);
                    return;
                }

                let mut rumor = unwrapped.rumor.clone();
                rumor.ensure_id();
                if let Err(e) = rumor.verify_id() {
                    error!("Invalid rumor id for gift wrap {}: {}", event.id, e);
                    return;
                }
                let rumor_id = rumor
                    .id
                    .expect("Invalid Gift Wrapped Event: There is no ID!")
                    .to_hex();
                let author_pubkey = rumor.pubkey.to_string();
                if let Ok(true) = app.db.is_deleted(&rumor_id, Some(author_pubkey.as_str())) {
                    if let Err(e) = app.db.record_deletion_markers(
                        &[event.id.to_string()],
                        None,
                    ) {
                        error!("Failed to record gift wrap deletion {}: {}", event.id, e);
                    }
                    return;
                }
                if let Ok(true) = app.db.is_trashed(&rumor_id) {
                    let recipient = event
                        .tags
                        .find(TagKind::p())
                        .and_then(|tag| tag.content())
                        .map(|val| val.to_string());
                    if let Err(e) = app.db.save_gift_wrap_map(
                        &event.id.to_string(),
                        &rumor_id,
                        recipient.as_deref(),
                        event.created_at.as_u64() as i64,
                    ) {
                        error!("Failed to save gift wrap map for trashed rumor: {}", e);
                    }
                    return;
                }

                let recipient = event
                    .tags
                    .find(TagKind::p())
                    .and_then(|tag| tag.content())
                    .map(|val| val.to_string());

                app.events.push(event.clone());

                if let Err(e) = app
                    .db
                    .store_event(&event, Some(&unwrapped), recipient.as_deref())
                {
                    error!("Failed to store event in database: {}", e);
                } else {
                    debug!("Successfully stored event with id {} in database", event.id);
                }
            }
            Err(e) => {
                error!("Failed to unwrap gift wrap {}: {}", event.id, e);
            }
        }
        return;
    }

    if let Ok(true) = app.db.has_event(&event.id.to_string()) {
        debug!("Skipping already stored event: {}", event.id);
        return;
    }

    app.events.push(event.clone());

    if let Err(e) = app.db.store_event(&event, None, None) {
        error!("Failed to store event in database: {}", e);
    } else {
        debug!("Successfully stored event with id {} in database", event.id);
    }
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
            .rect_filled(rect, egui::Rounding::same(6.0), style::ACCENT_LIGHT);
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::same(6.0),
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
                .inner_margin(Margin::symmetric(16.0, 12.0)),
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
                        .rounding(8.0),
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
                        egui::ComboBox::from_id_source("sidebar_account_selector")
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

    egui::CentralPanel::default().show(ctx, |ui| {
        match app.page {
            Page::Inbox => {
                ui.add_space(8.0);

                // Top bar with search
                ui.horizontal(|ui| {
                    if ui.button("Refresh").clicked() {
                        match app.db.get_top_level_messages() {
                            Ok(msgs) => app.table_entries = msgs,
                            Err(e) => {
                                error!("Could not fetch table entries to display from DB: {}", e)
                            }
                        }
                    }
                    ui.add_space(16.0);
                    let search_width = ui.available_width() - 100.0;
                    ui.add_sized(
                        [search_width, 32.0],
                        egui::TextEdit::singleline(&mut String::new())
                            .hint_text("Search")
                            .margin(egui::vec2(8.0, 4.0)),
                    );
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                if app.table_entries.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No messages yet")
                                .size(16.0)
                                .color(style::TEXT_MUTED),
                        );
                    });
                } else {
                    // Email list using TableBuilder
                    TableBuilder::new(ui)
                        .column(Column::auto()) // Checkbox
                        .column(Column::auto()) // Star
                        .column(Column::initial(160.0).at_least(100.0)) // Sender
                        .column(Column::remainder()) // Subject
                        .column(Column::initial(100.0).at_least(70.0)) // Time
                        .striped(true)
                        .sense(Sense::click())
                        .auto_shrink(Vec2b { x: false, y: false })
                        .header(28.0, |mut header| {
                            header.col(|ui| {
                                ui.checkbox(&mut false, "");
                            });
                            header.col(|ui| {
                                ui.label(RichText::new("⭐").size(12.0));
                            });
                            header.col(|ui| {
                                ui.label(RichText::new("From").small().color(style::TEXT_MUTED));
                            });
                            header.col(|ui| {
                                ui.label(RichText::new("Subject").small().color(style::TEXT_MUTED));
                            });
                            header.col(|ui| {
                                ui.label(RichText::new("Date").small().color(style::TEXT_MUTED));
                            });
                        })
                        .body(|body| {
                            let events: Vec<TableEntry> = app.table_entries.to_vec();
                            body.rows(style::INBOX_ROW_HEIGHT, events.len(), |mut row| {
                                let event = &events[row.index()];

                                row.col(|ui| {
                                    ui.checkbox(&mut false, "");
                                });
                                row.col(|ui| {
                                    ui.checkbox(&mut false, "");
                                });
                                row.col(|ui| {
                                    let _ = get_profile_metadata(app, event.pubkey.clone());
                                    let label = app
                                        .resolve_name(&event.pubkey)
                                        .unwrap_or_else(|| event.pubkey.to_string());
                                    ui.label(RichText::new(label).strong());
                                });
                                row.col(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(&event.subject);
                                        if event.thread_count > 1 {
                                            ui.label(
                                                RichText::new(format!("{}", event.thread_count))
                                                    .small()
                                                    .color(style::TEXT_MUTED),
                                            );
                                        }
                                    });
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(style::format_timestamp(event.created_at))
                                            .color(style::TEXT_MUTED)
                                            .small(),
                                    );
                                });

                                if row.response().clicked() {
                                    app.focused_post = event.id.clone();
                                    app.page = Page::Post;
                                    app.show_trashed_post = false;
                                }
                            });
                        });
                } // else (has table entries)
            }
            Page::Contacts => {
                ui::contacts::render_contacts_page(app, ui);
            }
            Page::Settings => {
                ui::settings::SettingsScreen::ui(app, ui);
            }
            Page::Post => {
                let events = if app.show_trashed_post {
                    app.db.get_email_thread_including_trash(&app.focused_post)
                } else {
                    app.db.get_email_thread(&app.focused_post)
                };
                let events = match events {
                    Ok(events) => events,
                    Err(e) => {
                        error!("Failed to load thread for {}: {}", app.focused_post, e);
                        app.page = Page::Inbox;
                        app.focused_post.clear();
                        return;
                    }
                };

                let mut event_ids: Vec<String> = Vec::new();
                for ev in &events {
                    if let Some(event_id) = ev.id.as_ref() {
                        event_ids.push(event_id.to_hex());
                    }
                }
                let trashed_ids = match app.db.get_trashed_event_ids(&event_ids) {
                    Ok(ids) => ids,
                    Err(e) => {
                        error!("Failed to load trashed event ids: {}", e);
                        Default::default()
                    }
                };

                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        for ev in events {
                            ui.add_space(8.0);

                            let event_id = ev.id;
                            let author = ev.author;

                            Frame::none()
                                .fill(style::CARD_BG)
                                .stroke(Stroke::new(1.0, style::CARD_STROKE))
                                .inner_margin(Margin::same(16.0))
                                .rounding(8.0)
                                .show(ui, |ui| {
                                    if event_id.is_none() || author.is_none() {
                                        ui.label(
                                            RichText::new("Error: malformed message (missing ID or author)")
                                                .color(Color32::RED),
                                        );
                                        if !ev.subject.is_empty() {
                                            ui.label(format!("Subject: {}", ev.subject));
                                        }
                                        return;
                                    }
                                    let event_id = event_id.unwrap();
                                    let author = author.unwrap();

                                    if trashed_ids.contains(&event_id.to_hex()) {
                                        ui.label(
                                            RichText::new("This message is in Trash")
                                                .small()
                                                .color(style::TEXT_MUTED),
                                        );
                                        ui.add_space(6.0);
                                    }
                                    ui.heading(&ev.subject);
                                    ui.add_space(4.0);

                                    // Metadata grid
                                    let author_pk = author.to_string();
                                    egui::Grid::new(format!("email_metadata-{}", event_id.to_hex()))
                                        .num_columns(2)
                                        .spacing([8.0, 4.0])
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new("From").color(style::TEXT_MUTED),
                                            );
                                            let _ = get_profile_metadata(app, author_pk.clone());
                                            let from_label = app
                                                .resolve_name(&author_pk)
                                                .unwrap_or_else(|| author_pk.clone());
                                            ui.label(RichText::new(from_label).strong());
                                            ui.end_row();

                                            ui.label(RichText::new("To").color(style::TEXT_MUTED));
                                            let to_labels: Vec<String> = ev
                                                .to
                                                .iter()
                                                .map(|pk| {
                                                    let pk_str = pk.to_string();
                                                    let _ =
                                                        get_profile_metadata(app, pk_str.clone());
                                                    app.resolve_name(&pk_str).unwrap_or(pk_str)
                                                })
                                                .collect();
                                            ui.label(to_labels.join(", "));
                                            ui.end_row();
                                        });

                                    ui.add_space(8.0);

                                    // Action buttons
                                    ui.horizontal(|ui| {
                                        if ui.button("📎 Attach").clicked() {
                                            // TODO: Handle attachment
                                        }
                                        if ui.button("📝 Edit").clicked() {
                                            // TODO: Handle edit
                                        }
                                        if ui.button("🗑️ Delete").clicked() {
                                            // TODO: broadcast NIP-09 EventDeletion to relays
                                            let now = chrono::Utc::now().timestamp();
                                            let purge_after = now + 30 * 24 * 60 * 60;
                                            let event_id_hex = event_id.to_hex();
                                            if let Err(e) = app
                                                .db
                                                .record_trash(&[event_id_hex.clone()], purge_after)
                                            {
                                                error!("Failed to move event to trash: {}", e);
                                            } else {
                                                app.events
                                                    .retain(|ev| ev.id.to_string() != event_id_hex);
                                                if app.focused_post == event_id_hex {
                                                    app.page = Page::Inbox;
                                                    app.focused_post.clear();
                                                    app.show_trashed_post = false;
                                                }
                                                match app.db.get_top_level_messages() {
                                                    Ok(msgs) => app.table_entries = msgs,
                                                    Err(e) => error!(
                                                        "Could not fetch table entries to display from DB: {}",
                                                        e
                                                    ),
                                                }
                                                app.refresh_trash();
                                            }
                                        }
                                        if ui.button("↩️ Reply").clicked() {
                                            let mut parent_events: Vec<EventId> =
                                                ev.parent_events.unwrap_or(Vec::new());
                                            parent_events.push(event_id);
                                            let state = ui::compose_window::ComposeWindowState {
                                                subject: format!("Re: {}", ev.subject),
                                                to_field: author.to_string(),
                                                content: String::new(),
                                                parent_events,
                                                selected_account: None,
                                                minimized: false,
                                                draft_id: None,
                                            };
                                            app.state.compose_window.insert(
                                                egui::Id::new(rand::random::<u32>()),
                                                state,
                                            );
                                        }
                                        if ui.button("↪️ Forward").clicked() {
                                            // TODO: Handle forward
                                        }
                                        if ui.button("⭐ Star").clicked() {
                                            // TODO: Handle star
                                        }
                                    });

                                    ui.add_space(12.0);
                                    ui.separator();
                                    ui.add_space(12.0);

                                    // Message content
                                    ui.label(ev.content);
                                });
                        }
                    });

                if let Some(event) = app
                    .events
                    .iter()
                    .find(|e| e.id.to_string() == app.focused_post)
                {
                    if let Ok(unwrapped) = app.account_manager.unwrap_gift_wrap(event) {
                        let _subject = &unwrapped
                            .rumor
                            .tags
                            .find(TagKind::Subject)
                            .and_then(|s| s.content())
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "No Subject".to_string());
                        // Message header section
                    }
                }
            }
            Page::Drafts => {
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.heading("Drafts");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Refresh").clicked() {
                            app.refresh_drafts();
                        }
                    });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                if app.drafts.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No drafts")
                                .size(16.0)
                                .color(style::TEXT_MUTED),
                        );
                    });
                } else {
                    let mut draft_to_delete: Option<i64> = None;
                    let mut draft_to_open: Option<db::Draft> = None;

                    TableBuilder::new(ui)
                        .column(Column::initial(200.0).at_least(100.0)) // Subject
                        .column(Column::initial(200.0).at_least(100.0)) // To
                        .column(Column::initial(120.0).at_least(80.0)) // Last Modified
                        .column(Column::initial(60.0).at_least(60.0)) // Actions
                        .striped(true)
                        .auto_shrink(Vec2b { x: false, y: false })
                        .header(28.0, |mut header| {
                            header.col(|ui| {
                                ui.label(RichText::new("Subject").small().color(style::TEXT_MUTED));
                            });
                            header.col(|ui| {
                                ui.label(RichText::new("To").small().color(style::TEXT_MUTED));
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("Last Modified")
                                        .small()
                                        .color(style::TEXT_MUTED),
                                );
                            });
                            header.col(|ui| {
                                ui.label(RichText::new("").small());
                            });
                        })
                        .body(|body| {
                            let drafts: Vec<db::Draft> = app.drafts.clone();
                            body.rows(style::INBOX_ROW_HEIGHT, drafts.len(), |mut row| {
                                let draft = &drafts[row.index()];

                                row.col(|ui| {
                                    let subject = if draft.subject.is_empty() {
                                        "(No Subject)"
                                    } else {
                                        &draft.subject
                                    };
                                    if ui.link(RichText::new(subject).strong()).clicked() {
                                        draft_to_open = Some(draft.clone());
                                    }
                                });
                                row.col(|ui| {
                                    let to = if draft.to_field.is_empty() {
                                        "(No Recipient)"
                                    } else {
                                        &draft.to_field
                                    };
                                    ui.label(RichText::new(to).color(style::TEXT_MUTED));
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(style::format_timestamp(draft.updated_at))
                                            .color(style::TEXT_MUTED)
                                            .small(),
                                    );
                                });
                                row.col(|ui| {
                                    if ui
                                        .button(RichText::new("X").color(Color32::RED))
                                        .on_hover_text("Delete draft")
                                        .clicked()
                                    {
                                        draft_to_delete = Some(draft.id);
                                    }
                                });
                            });
                        });

                    if let Some(draft) = draft_to_open {
                        let parent_events: Vec<EventId> = draft
                            .parent_events
                            .iter()
                            .filter_map(|s| EventId::parse(s).ok())
                            .collect();
                        let selected_account = draft.selected_account.as_ref().and_then(|pk_str| {
                            app.account_manager
                                .loaded_keys
                                .iter()
                                .find(|k| k.public_key().to_string() == *pk_str)
                                .cloned()
                        });
                        let state = ui::compose_window::ComposeWindowState {
                            subject: draft.subject,
                            to_field: draft.to_field,
                            content: draft.content,
                            parent_events,
                            selected_account,
                            minimized: false,
                            draft_id: Some(draft.id),
                        };
                        app.state
                            .compose_window
                            .insert(egui::Id::new(rand::random::<u32>()), state);
                    }

                    if let Some(id) = draft_to_delete {
                        if let Err(e) = app.db.delete_draft(id) {
                            error!("Failed to delete draft: {}", e);
                        }
                        app.refresh_drafts();
                    }
                }
            }
            Page::Trash => {
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.heading("Trash");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Refresh").clicked() {
                            app.refresh_trash();
                        }
                    });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                if app.trash_entries.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("Trash is empty")
                                .size(16.0)
                                .color(style::TEXT_MUTED),
                        );
                    });
                } else {
                    let mut to_restore: Option<String> = None;
                    let mut to_delete: Option<String> = None;

                    TableBuilder::new(ui)
                        .column(Column::initial(160.0).at_least(100.0)) // Sender
                        .column(Column::remainder()) // Subject
                        .column(Column::initial(100.0).at_least(70.0)) // Time
                        .column(Column::initial(140.0).at_least(120.0)) // Actions
                        .striped(true)
                        .sense(Sense::click())
                        .auto_shrink(Vec2b { x: false, y: false })
                        .header(28.0, |mut header| {
                            header.col(|ui| {
                                ui.label(RichText::new("From").small().color(style::TEXT_MUTED));
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("Subject").small().color(style::TEXT_MUTED),
                                );
                            });
                            header.col(|ui| {
                                ui.label(RichText::new("Date").small().color(style::TEXT_MUTED));
                            });
                            header.col(|ui| {
                                ui.label(RichText::new("Actions").small().color(style::TEXT_MUTED));
                            });
                        })
                        .body(|body| {
                            let events: Vec<TableEntry> = app.trash_entries.to_vec();
                            body.rows(style::INBOX_ROW_HEIGHT, events.len(), |mut row| {
                                let event = &events[row.index()];

                                row.col(|ui| {
                                    let _ = get_profile_metadata(app, event.pubkey.clone());
                                    let label = app
                                        .resolve_name(&event.pubkey)
                                        .unwrap_or_else(|| event.pubkey.to_string());
                                    ui.label(RichText::new(label).strong());
                                });
                                row.col(|ui| {
                                    ui.label(&event.subject);
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(style::format_timestamp(event.created_at))
                                            .color(style::TEXT_MUTED)
                                            .small(),
                                    );
                                });
                                row.col(|ui| {
                                    ui.horizontal(|ui| {
                                        if ui.button("Restore").clicked() {
                                            to_restore = Some(event.id.clone());
                                        }
                                        if ui.button("Delete now").clicked() {
                                            // TODO: broadcast NIP-09 EventDeletion to relays
                                            to_delete = Some(event.id.clone());
                                        }
                                    });
                                });

                                if row.response().clicked() {
                                    app.focused_post = event.id.clone();
                                    app.page = Page::Post;
                                    app.show_trashed_post = true;
                                }
                            });
                        });

                    if let Some(event_id) = to_restore {
                        if let Err(e) = app.db.restore_from_trash(&event_id) {
                            error!("Failed to restore from trash: {}", e);
                        } else {
                            match app.db.get_top_level_messages() {
                                Ok(msgs) => app.table_entries = msgs,
                                Err(e) => error!(
                                    "Could not fetch table entries to display from DB: {}",
                                    e
                                ),
                            }
                            app.refresh_trash();
                        }
                    }

                    if let Some(event_id) = to_delete {
                        if let Err(e) = apply_deletions(app, vec![event_id.clone()], None, None) {
                            error!("Failed to delete trashed event: {}", e);
                        } else {
                            app.refresh_trash();
                        }
                    }
                }
            }
            Page::Unlock => {
                ui::unlock_database::UnlockDatabase::ui(app, ui);
            }
            Page::Onboarding
            | Page::OnboardingNewUser
            | Page::OnboardingNewShowKey
            | Page::OnboardingReturning => {
                ui::onboarding::OnboardingScreen::ui(app, ui);
            }
            _ => {
                ui.heading("This hasn't been implemented yet.");
            }
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
        update_app(self, ctx);
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
