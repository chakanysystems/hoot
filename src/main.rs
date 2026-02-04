#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use eframe::egui::{
    self, Align2, Color32, ColorImage, FontDefinitions, FontId, Frame, Margin, RichText,
    ScrollArea, Sense, TextureHandle, TextureOptions, Vec2, Vec2b,
};
use egui::FontFamily::Proportional;
use egui_extras::{Column, TableBuilder};
use nostr::{event::Kind, EventId, SingleLetterTag, TagKind};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn, Level};

mod account_manager;
mod db;
mod error;
mod keystorage;
mod mail_event;
mod profile_metadata;
use profile_metadata::{get_profile_metadata, ProfileMetadata, ProfileOption};
mod relay;
mod ui;
// not sure if i will use this but i'm committing it for later.
// mod threaded_event;

// WE PROBABLY SHOULDN'T MAKE EVERYTHING A STRING, GRR!
#[derive(Clone, Debug)]
pub struct TableEntry {
    pub id: String,
    pub content: String,
    pub subject: String,
    pub pubkey: String,
    pub created_at: i64,
}

const CONTACT_AVATAR_SIZE: f32 = 48.0;

#[derive(Clone)]
struct Contact {
    pub pubkey: String,
    pub metadata: ProfileMetadata,
}

impl Contact {
    fn display_name(&self) -> String {
        self.metadata
            .display_name
            .clone()
            .or(self.metadata.name.clone())
            .unwrap_or_else(|| self.pubkey.clone())
    }

    fn initials(&self) -> String {
        let fallback = self
            .metadata
            .display_name
            .as_deref()
            .or(self.metadata.name.as_deref())
            .unwrap_or(&self.pubkey);

        let mut initials = fallback
            .split_whitespace()
            .filter_map(|segment| segment.chars().next())
            .map(|ch| ch.to_ascii_uppercase())
            .take(2)
            .collect::<String>();

        if initials.is_empty() {
            initials = fallback
                .chars()
                .take(2)
                .map(|ch| ch.to_ascii_uppercase())
                .collect();
        }

        initials
    }

    fn picture_url(&self) -> Option<&str> {
        self.metadata
            .picture
            .as_deref()
            .filter(|url| !url.is_empty())
    }
}

struct ContactImageMessage {
    pub pubkey: String,
    pub image: Option<ColorImage>,
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
            let _ = &cc.egui_ctx.set_visuals(egui::Visuals::light());
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
            let _ = &cc.egui_ctx.set_fonts(fonts);
            let _ = &cc
                .egui_ctx
                .style_mut(|style| style.visuals.dark_mode = false);
            Box::new(Hoot::new(cc))
        }),
    )
}

#[derive(Debug, PartialEq)]
pub enum Page {
    Inbox,
    Drafts,
    Settings,
    // TODO: fix this mess
    Onboarding,
    OnboardingNew,
    OnboardingNewShowKey,
    OnboardingReturning,
    Post,
    Contacts,
}

// for storing the state of different components and such.
#[derive(Default)]
pub struct HootState {
    pub add_account_window: HashMap<egui::Id, ui::add_account_window::AddAccountWindowState>,
    pub compose_window: HashMap<egui::Id, ui::compose_window::ComposeWindowState>,
    pub onboarding: ui::onboarding::OnboardingState,
    pub settings: ui::settings::SettingsState,
}

pub struct Hoot {
    pub page: Page,
    focused_post: String,
    status: HootStatus,
    state: HootState,
    relays: relay::RelayPool,
    events: Vec<nostr::Event>,
    account_manager: account_manager::AccountManager,
    pub active_account: Option<nostr::Keys>,
    db: db::Db,
    table_entries: Vec<TableEntry>,
    profile_metadata: HashMap<String, profile_metadata::ProfileOption>,
    contacts: Vec<Contact>,
    contact_images: HashMap<String, TextureHandle>,
    pending_contact_images: HashSet<String>,
    failed_contact_images: HashSet<String>,
    image_request_sender: Sender<ContactImageMessage>,
    image_request_receiver: Receiver<ContactImageMessage>,
}

#[derive(Debug, PartialEq)]
enum HootStatus {
    Initalizing,
    Ready,
}

fn update_app(app: &mut Hoot, ctx: &egui::Context) {
    #[cfg(feature = "profiling")]
    puffin::profile_function!();
    let ctx = ctx.clone();
    let wake_ctx = ctx.clone();
    let wake_up = move || {
        wake_ctx.request_repaint();
    };

    if app.status == HootStatus::Initalizing {
        info!("Initalizing Hoot...");
        match app.account_manager.load_keys() {
            Ok(..) => {}
            Err(v) => error!("something went wrong trying to load keys: {}", v),
        }

        match app.db.get_top_level_messages() {
            Ok(msgs) => app.table_entries = msgs,
            Err(e) => error!("Could not fetch table entries to display from DB: {}", e),
        }

        let _ = app
            .relays
            .add_url("wss://relay.chakany.systems".to_string(), wake_up.clone());

        let _ = app
            .relays
            .add_url("wss://talon.quest".to_string(), wake_up.clone());

        if app.account_manager.loaded_keys.len() > 0 {
            let mut gw_sub = relay::Subscription::default();

            let filter = nostr::Filter::new().kind(nostr::Kind::GiftWrap).custom_tag(
                SingleLetterTag {
                    character: nostr::Alphabet::P,
                    uppercase: false,
                },
                app.account_manager
                    .loaded_keys
                    .clone()
                    .into_iter()
                    .map(|keys| keys.public_key()),
            );
            gw_sub.filter(filter);

            // TODO: fix error handling
            let _ = app.relays.add_subscription(gw_sub);
        }

        app.status = HootStatus::Ready;
        info!("Hoot Ready");
    }

    app.relays.keepalive(wake_up);

    let new_val = app.relays.try_recv();
    if new_val.is_some() {
        info!("{:?}", new_val.clone());

        match relay::RelayMessage::from_json(&new_val.unwrap()) {
            Ok(v) => process_message(app, &v),
            Err(e) => error!("could not decode message sent from relay: {}", e),
        };
    }

    app.process_contact_image_queue(&ctx);
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

fn process_event(app: &mut Hoot, _sub_id: &str, event_json: &str) {
    #[cfg(feature = "profiling")]
    puffin::profile_function!();

    // Parse the event using the RelayMessage type which handles the ["EVENT", subscription_id, event_json] format
    if let Ok(event) = serde_json::from_str::<nostr::Event>(event_json) {
        // Verify the event signature
        if event.verify().is_ok() {
            debug!("Verified event: {:?}", event);

            // Check if we already have this event
            if let Ok(has_event) = app.db.has_event(&event.id.to_string()) {
                if has_event {
                    debug!("Skipping already stored event: {}", event.id);
                    return;
                }
            }

            if event.kind == Kind::Metadata {
                debug!("Got profile metadata");

                let deserialized_metadata: profile_metadata::ProfileMetadata =
                    serde_json::from_str(&event.content).unwrap();
                app.profile_metadata.insert(
                    event.pubkey.to_string(),
                    ProfileOption::Some(deserialized_metadata.clone()),
                );
                app.upsert_contact(event.pubkey.to_string(), deserialized_metadata.clone());
                // TODO: evaluate perf cost of clone LOL
                match app.db.update_profile_metadata(event.clone()) {
                    Ok(_) => { // wow who cares
                    }
                    Err(e) => error!("Error when saving profile metadata to DB: {}", e),
                }
            }

            // Store the event in memory
            app.events.push(event.clone());

            // Store the event in the database
            if let Err(e) = app.db.store_event(&event, &mut app.account_manager) {
                error!("Failed to store event in database: {}", e);
            } else {
                debug!("Successfully stored event with id {} in database", event.id);
            }
        } else {
            error!("Event verification failed for event: {}", event.id);
        }
    } else {
        error!("Failed to parse event JSON: {}", event_json);
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
    if let Some(profile_metadata::ProfileOption::Some(meta)) = app.profile_metadata.get(&pubkey) {
        if let Some(display_name) = &meta.display_name {
            return display_name.clone();
        }
        if let Some(name) = &meta.name {
            return name.clone();
        }
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

fn render_app(app: &mut Hoot, ctx: &egui::Context) {
    // Render add account windows
    let mut account_windows_to_remove = Vec::new();
    for window_id in app.state.add_account_window.clone().into_keys() {
        if !ui::add_account_window::AddAccountWindow::show_window(app, ctx, window_id) {
            account_windows_to_remove.push(window_id);
        }
    }
    for id in account_windows_to_remove {
        app.state.add_account_window.remove(&id);
    }

    // Render compose windows if any are open - moved outside CentralPanel
    for window_id in app.state.compose_window.clone().into_keys() {
        ui::compose_window::ComposeWindow::show_window(app, ctx, window_id);
    }

    egui::SidePanel::left("left_panel")
        .default_width(200.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(8.0);
                // App title
                ui.heading("Hoot");
                ui.add_space(16.0);

                // Compose button
                if ui
                    .add_sized(
                        [180.0, 36.0],
                        egui::Button::new("✉ Compose").fill(egui::Color32::from_rgb(149, 117, 205)),
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
                    };
                    app.state
                        .compose_window
                        .insert(egui::Id::new(rand::random::<u32>()), state);
                }

                ui.add_space(16.0);

                // Navigation items
                let nav_items = [
                    ("📥 Inbox", Page::Inbox, app.events.len()),
                    ("🔄 Requests", Page::Post, 20),
                    ("📝 Drafts", Page::Drafts, 3),
                    ("⭐ Starred", Page::Post, 0),
                    ("📁 Archived", Page::Post, 0),
                    ("🗑️ Trash", Page::Post, 0),
                    ("Contacts", Page::Contacts, 0),
                ];

                for (label, page, count) in nav_items {
                    let is_selected = app.page == page;
                    let response = ui.selectable_label(
                        is_selected,
                        format!(
                            "{} {}",
                            label,
                            if count > 0 {
                                count.to_string()
                            } else {
                                String::new()
                            }
                        ),
                    );
                    if response.clicked() {
                        app.page = page;
                    }
                }
                // Show onboarding for first-time users, or Add Account button for existing users
                if app.account_manager.loaded_keys.is_empty() {
                    if ui.button("onboarding").clicked() {
                        app.page = Page::OnboardingNew;
                    }
                } else {
                    if ui.button("+ Add Account").clicked() {
                        let state = ui::add_account_window::AddAccountWindowState::default();
                        app.state
                            .add_account_window
                            .insert(egui::Id::new(rand::random::<u32>()), state);
                    }
                }

                // Add flexible space to push profile to bottom
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(8.0);

                    // Account selector
                    if !app.account_manager.loaded_keys.is_empty() {
                        ui.label(RichText::new("Account:").size(10.0));
                        egui::ComboBox::from_id_source("sidebar_account_selector")
                            .selected_text(get_account_display_text(app))
                            .width(180.0)
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

                    // Settings button
                    if ui.add_sized([32.0, 32.0], egui::Button::new("⚙")).clicked() {
                        app.page = Page::Settings;
                    }
                });
            });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        match app.page {
            Page::Inbox => {
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

                ui.add_space(8.0);

                if app.table_entries.len() == 0 {
                    ui.label("I couldn't find any messages for you :(");
                }

                // Email list using TableBuilder
                TableBuilder::new(ui)
                    .column(Column::auto()) // Checkbox
                    .column(Column::auto()) // Star
                    .column(Column::remainder()) // Sender
                    .column(Column::remainder()) // Content
                    .column(Column::remainder()) // Time
                    .striped(true)
                    .sense(Sense::click())
                    .auto_shrink(Vec2b { x: false, y: false })
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.checkbox(&mut false, "");
                        });
                        header.col(|ui| {
                            ui.label("⭐");
                        });
                        header.col(|ui| {
                            ui.label("From");
                        });
                        header.col(|ui| {
                            ui.label("Subject");
                        });
                        header.col(|ui| {
                            ui.label("Time");
                        });
                    })
                    .body(|body| {
                        let row_height = 30.0;
                        let events: Vec<TableEntry> = app.table_entries.to_vec();
                        body.rows(row_height, events.len(), |mut row| {
                            let event = &events[row.index()];

                            row.col(|ui| {
                                ui.checkbox(&mut false, "");
                            });
                            row.col(|ui| {
                                ui.checkbox(&mut false, "");
                            });
                            row.col(|ui| {
                                match get_profile_metadata(app, event.pubkey.clone()) {
                                    ProfileOption::Waiting => {
                                        // fuck
                                        ui.label(event.pubkey.to_string());
                                    }
                                    ProfileOption::Some(meta) => {
                                        if let Some(display_name) = &meta.display_name {
                                            ui.label(display_name);
                                        } else {
                                            ui.label(event.pubkey.to_string());
                                        }
                                    }
                                }
                            });
                            row.col(|ui| {
                                // Try to get subject from tags
                                ui.label(&event.subject);
                            });
                            row.col(|ui| {
                                ui.label(event.created_at.to_string());
                            });

                            if row.response().clicked() {
                                app.focused_post = event.id.clone();
                                app.page = Page::Post;
                            }
                        });
                    });
            }
            Page::Contacts => {
                render_contacts_page(app, ui);
            }
            Page::Settings => {
                ui::settings::SettingsScreen::ui(app, ui);
            }
            Page::Post => {
                let events = app.db.get_email_thread(&app.focused_post).unwrap();

                for ev in events {
                    ui.add_space(8.0);
                    ui.heading(&ev.subject);
                    let destination_stringed = ev
                        .to
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");

                    // Metadata grid
                    egui::Grid::new(format!("email_metadata-{:?}", ev.id))
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("From");
                            match get_profile_metadata(app, ev.author.unwrap().to_string()) {
                                ProfileOption::Waiting => {
                                    // fuck
                                    ui.label(ev.author.unwrap().to_string());
                                }
                                ProfileOption::Some(meta) => {
                                    if let Some(display_name) = &meta.display_name {
                                        ui.label(display_name);
                                    } else {
                                        ui.label(ev.author.unwrap().to_string());
                                    }
                                }
                            }
                            ui.end_row();

                            ui.label("To");
                            match get_profile_metadata(app, destination_stringed.clone()) {
                                ProfileOption::Waiting => {
                                    ui.label(destination_stringed.clone());
                                }
                                ProfileOption::Some(meta) => {
                                    if let Some(display_name) = &meta.display_name {
                                        ui.label(display_name);
                                    } else {
                                        ui.label(destination_stringed.clone());
                                    }
                                }
                            }
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
                            // TODO: Handle delete
                        }
                        if ui.button("↩️ Reply").clicked() {
                            let mut parent_events: Vec<EventId> =
                                ev.parent_events.unwrap_or(Vec::new());
                            parent_events.push(ev.id.unwrap());
                            let state = ui::compose_window::ComposeWindowState {
                                subject: format!("Re: {}", ev.subject),
                                to_field: ev.author.unwrap().to_string(),
                                content: String::new(),
                                parent_events,
                                selected_account: None,
                                minimized: false,
                            };
                            app.state
                                .compose_window
                                .insert(egui::Id::new(rand::random::<u32>()), state);
                        }
                        if ui.button("↪️ Forward").clicked() {
                            // TODO: Handle forward
                        }
                        if ui.button("⭐ Star").clicked() {
                            // TODO: Handle star
                        }
                    });

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);

                    // Message content
                    ui.label(ev.content);
                }

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
                ui.heading("Drafts");
                ui.label("Your draft messages will appear here");
            }
            _ => {
                ui::onboarding::OnboardingScreen::ui(app, ui);
            }
        }
    });
}

impl Hoot {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Create storage directory if it doesn't exist
        let storage_dir = eframe::storage_dir("hoot").unwrap();
        std::fs::create_dir_all(&storage_dir).unwrap();

        // Create the database file path
        let db_path = storage_dir.join("hoot.db");

        // Initialize the database
        let db = match db::Db::new(db_path) {
            Ok(db) => {
                info!("Database initialized successfully");
                db
            }
            Err(e) => {
                error!("Failed to initialize database: {}", e);
                panic!("Database initialization failed: {}", e);
            }
        };

        let contacts_data = match db.get_contacts() {
            Ok(entries) => entries,
            Err(err) => {
                error!("Failed to load contacts from database: {}", err);
                Vec::new()
            }
        };

        let mut contacts: Vec<Contact> = contacts_data
            .into_iter()
            .map(|(pubkey, metadata)| Contact { pubkey, metadata })
            .collect();
        contacts.sort_by(|a, b| Self::contact_sort_key(a).cmp(&Self::contact_sort_key(b)));

        let mut profile_metadata_cache: HashMap<String, ProfileOption> = HashMap::new();
        for contact in &contacts {
            profile_metadata_cache.insert(
                contact.pubkey.clone(),
                ProfileOption::Some(contact.metadata.clone()),
            );
        }

        let (image_request_sender, image_request_receiver) = std::sync::mpsc::channel();

        Self {
            page: Page::Inbox,
            focused_post: "".into(),
            status: HootStatus::Initalizing,
            state: Default::default(),
            relays: relay::RelayPool::new(),
            events: Vec::new(),
            account_manager: account_manager::AccountManager::new(),
            active_account: None,
            db,
            table_entries: Vec::new(),
            profile_metadata: profile_metadata_cache,
            contacts,
            contact_images: HashMap::new(),
            pending_contact_images: HashSet::new(),
            failed_contact_images: HashSet::new(),
            image_request_sender,
            image_request_receiver,
        }
    }

    fn contact_sort_key(contact: &Contact) -> String {
        contact
            .metadata
            .display_name
            .clone()
            .or(contact.metadata.name.clone())
            .unwrap_or_else(|| contact.pubkey.clone())
            .to_lowercase()
    }

    fn ensure_contact_image_request(&mut self, contact: &Contact) {
        let Some(url) = contact.picture_url() else {
            return;
        };

        if self.contact_images.contains_key(&contact.pubkey)
            || self.pending_contact_images.contains(&contact.pubkey)
            || self.failed_contact_images.contains(&contact.pubkey)
        {
            return;
        }

        let sender = self.image_request_sender.clone();
        let pubkey = contact.pubkey.clone();
        let url = url.to_string();

        self.pending_contact_images.insert(pubkey.clone());

        thread::spawn(move || {
            let image = fetch_profile_image(&url);
            if sender.send(ContactImageMessage { pubkey, image }).is_err() {
                debug!("Contact image receiver dropped before image arrived");
            }
        });
    }

    fn process_contact_image_queue(&mut self, ctx: &egui::Context) {
        let mut updated = false;

        while let Ok(message) = self.image_request_receiver.try_recv() {
            self.pending_contact_images.remove(&message.pubkey);

            if let Some(image) = message.image {
                let texture = ctx.load_texture(
                    format!("contact-{}", message.pubkey),
                    image,
                    TextureOptions::LINEAR,
                );
                self.contact_images.insert(message.pubkey, texture);
            } else {
                self.failed_contact_images.insert(message.pubkey);
            }

            updated = true;
        }

        if updated {
            ctx.request_repaint();
        }
    }

    fn upsert_contact(&mut self, pubkey: String, metadata: ProfileMetadata) {
        let mut updated = false;

        if let Some(existing) = self.contacts.iter_mut().find(|c| c.pubkey == pubkey) {
            let previous_picture = existing.metadata.picture.clone();
            existing.metadata = metadata.clone();
            if previous_picture != existing.metadata.picture {
                self.contact_images.remove(&existing.pubkey);
                self.pending_contact_images.remove(&existing.pubkey);
                self.failed_contact_images.remove(&existing.pubkey);
            }
            updated = true;
        }

        if !updated {
            self.contacts.push(Contact {
                pubkey: pubkey.clone(),
                metadata: metadata.clone(),
            });
        }

        self.contacts
            .sort_by(|a, b| Self::contact_sort_key(a).cmp(&Self::contact_sort_key(b)));

        self.profile_metadata
            .insert(pubkey, ProfileOption::Some(metadata));
    }
}

fn render_contacts_page(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.heading("Contacts");
    ui.add_space(8.0);

    if app.contacts.is_empty() {
        ui.label("No contacts yet. Profile metadata will appear here when available.");
        return;
    }

    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let total = app.contacts.len();

            for index in 0..total {
                let contact = { app.contacts[index].clone() };
                app.ensure_contact_image_request(&contact);

                Frame::group(ui.style())
                    .inner_margin(Margin::symmetric(12.0, 8.0))
                    .rounding(CONTACT_AVATAR_SIZE / 2.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            draw_contact_avatar(app, ui, &contact);
                            ui.add_space(12.0);

                            let display_name = contact.display_name();

                            ui.vertical(|ui| {
                                ui.label(RichText::new(display_name.clone()).strong());

                                if let Some(name) = contact.metadata.name.as_ref() {
                                    if contact.metadata.display_name.as_ref() != Some(name) {
                                        ui.label(name);
                                    }
                                }

                                ui.label(RichText::new(&contact.pubkey).monospace());
                            });
                        });
                    });

                ui.add_space(8.0);
            }
        });
}

fn draw_contact_avatar(app: &Hoot, ui: &mut egui::Ui, contact: &Contact) {
    let size = Vec2::splat(CONTACT_AVATAR_SIZE);

    if let Some(texture) = app.contact_images.get(&contact.pubkey) {
        ui.add(egui::Image::new((texture.id(), size)).maintain_aspect_ratio(true));
        return;
    }

    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.circle_filled(
        rect.center(),
        CONTACT_AVATAR_SIZE / 2.0,
        Color32::from_rgb(149, 117, 205),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        contact.initials(),
        FontId::proportional(18.0),
        Color32::WHITE,
    );
}

fn fetch_profile_image(url: &str) -> Option<ColorImage> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        debug!("Skipping unsupported contact image URL: {}", url);
        return None;
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            debug!("Failed to build HTTP client for contact image: {}", err);
            return None;
        }
    };

    match client.get(url).send() {
        Ok(response) => {
            if !response.status().is_success() {
                warn!(
                    "Contact image request returned status {} for {}",
                    response.status(),
                    url
                );
                return None;
            }

            match response.bytes() {
                Ok(bytes) => decode_image(bytes.as_ref()),
                Err(err) => {
                    debug!("Failed to read contact image bytes: {}", err);
                    None
                }
            }
        }
        Err(err) => {
            debug!("Failed to fetch contact image {}: {}", url, err);
            None
        }
    }
}

fn decode_image(bytes: &[u8]) -> Option<ColorImage> {
    let mut rgba = match image::load_from_memory(bytes) {
        Ok(img) => img.to_rgba8(),
        Err(err) => {
            debug!("Failed to decode contact image: {}", err);
            return None;
        }
    };

    if rgba.width() > 256 || rgba.height() > 256 {
        rgba = image::imageops::resize(&rgba, 256, 256, image::imageops::FilterType::Triangle);
    }

    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = rgba
        .as_raw()
        .chunks_exact(4)
        .map(|chunk| Color32::from_rgba_unmultiplied(chunk[0], chunk[1], chunk[2], chunk[3]))
        .collect::<Vec<_>>();

    Some(ColorImage { size, pixels })
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
