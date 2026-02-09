#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use anyhow::bail;
use eframe::egui::{
    self, Align2, Color32, ColorImage, FontDefinitions, FontId, Frame, Margin, RichText,
    ScrollArea, Sense, Stroke, TextureHandle, TextureOptions, Vec2, Vec2b,
};
use egui::FontFamily::Proportional;
use egui_extras::{Column, TableBuilder};
use nostr::{event::Kind, EventId, SingleLetterTag, TagKind};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use std::{panic, thread};
use tracing::{debug, error, info, warn, Level};

mod account_manager;
mod db;
mod error;
mod mail_event;
mod profile_metadata;
use profile_metadata::{get_profile_metadata, ProfileMetadata, ProfileOption};
mod relay;
mod style;
mod ui;

// WE PROBABLY SHOULDN'T MAKE EVERYTHING A STRING, GRR!
#[derive(Clone, Debug)]
pub struct TableEntry {
    pub id: String,
    pub content: String,
    pub subject: String,
    pub pubkey: String,
    pub created_at: i64,
}

const CONTACT_AVATAR_SIZE: f32 = style::AVATAR_SIZE;

#[derive(Clone)]
struct Contact {
    pub pubkey: String,
    pub petname: Option<String>,
    pub metadata: ProfileMetadata,
}

impl Contact {
    fn display_name(&self) -> String {
        self.petname
            .clone()
            .or(self.metadata.display_name.clone())
            .or(self.metadata.name.clone())
            .unwrap_or_else(|| self.pubkey.clone())
    }

    fn initials(&self) -> String {
        let fallback = self
            .petname
            .as_deref()
            .or(self.metadata.display_name.as_deref())
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
    drafts: Vec<db::Draft>,
}

#[derive(Debug, PartialEq)]
enum HootStatus {
    PreUnlock,
    WaitingForUnlock,
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
        // we can't do anything but wait until HootStatus is Initalizing
        app.relays.keepalive(wake_up);
        let new_val = app.relays.try_recv();
        if new_val.is_some() {
            info!("{:?}", new_val.clone());

            match relay::RelayMessage::from_json(&new_val.unwrap()) {
                Ok(v) => process_message(app, &v),
                Err(e) => error!("could not decode message sent from relay: {}", e),
            };
        }
        return;
    }

    if app.status == HootStatus::Initalizing {
        info!("Initalizing Hoot...");
        match app.account_manager.load_keys(&app.db) {
            Ok(..) => {}
            Err(v) => error!("something went wrong trying to load keys: {}", v),
        }

        match app.db.get_top_level_messages() {
            Ok(msgs) => app.table_entries = msgs,
            Err(e) => error!("Could not fetch table entries to display from DB: {}", e),
        }

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

            let contacts_data = match app.db.get_user_contacts() {
                Ok(entries) => entries,
                Err(err) => {
                    error!("Failed to load contacts from database: {}", err);
                    Vec::new()
                }
            };

            app.contacts = contacts_data
                .into_iter()
                .map(|(pubkey, petname, metadata)| Contact {
                    pubkey,
                    petname,
                    metadata,
                })
                .collect();
            app.contacts
                .sort_by(|a, b| Hoot::contact_sort_key(a).cmp(&Hoot::contact_sort_key(b)));

            for contact in &app.contacts {
                app.profile_metadata.insert(
                    contact.pubkey.clone(),
                    ProfileOption::Some(contact.metadata.clone()),
                );
            }
        }

        app.refresh_drafts();

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
                    ("🔄 Requests", Page::Post, 20),
                    ("📝 Drafts", Page::Drafts, app.drafts.len()),
                    ("⭐ Starred", Page::Post, 0),
                    ("📁 Archived", Page::Post, 0),
                    ("🗑️ Trash", Page::Post, 0),
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
    let mut compose_windows_to_remove = Vec::new();
    for window_id in app.state.compose_window.clone().into_keys() {
        if !ui::compose_window::ComposeWindow::show_window(app, ctx, window_id) {
            compose_windows_to_remove.push(window_id);
        }
    }
    for id in compose_windows_to_remove {
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
                                    ui.label(&event.subject);
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
                                }
                            });
                        });
                } // else (has table entries)
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

                    Frame::none()
                        .fill(style::CARD_BG)
                        .stroke(Stroke::new(1.0, style::CARD_STROKE))
                        .inner_margin(Margin::same(16.0))
                        .rounding(8.0)
                        .show(ui, |ui| {
                            ui.heading(&ev.subject);
                            ui.add_space(4.0);

                            // Metadata grid
                            egui::Grid::new(format!("email_metadata-{:?}", ev.id))
                                .num_columns(2)
                                .spacing([8.0, 4.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new("From").color(style::TEXT_MUTED));
                                    let author_pk = ev.author.unwrap().to_string();
                                    let _ = get_profile_metadata(app, author_pk.clone());
                                    let from_label =
                                        app.resolve_name(&author_pk).unwrap_or_else(|| author_pk);
                                    ui.label(RichText::new(from_label).strong());
                                    ui.end_row();

                                    ui.label(RichText::new("To").color(style::TEXT_MUTED));
                                    let to_labels: Vec<String> = ev
                                        .to
                                        .iter()
                                        .map(|pk| {
                                            let pk_str = pk.to_string();
                                            let _ = get_profile_metadata(app, pk_str.clone());
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
                                        draft_id: None,
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

                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(12.0);

                            // Message content
                            ui.label(ev.content);
                        });
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
                ui.heading("Something has gone seriously wrong! Restart Hoot.");
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

        let (image_request_sender, image_request_receiver) = std::sync::mpsc::channel();

        // check if this is our first time loading
        let page = match std::fs::exists(storage_dir.join("done")) {
            Ok(v) => {
                if v {
                    Page::Unlock
                } else {
                    Page::Onboarding
                }
            }
            Err(e) => {
                panic!("Couldn't check if we have already setup: {}", e);
            }
        };

        Self {
            page: page,
            focused_post: "".into(),
            status: HootStatus::PreUnlock,
            state: Default::default(),
            relays: relay::RelayPool::new(),
            events: Vec::new(),
            account_manager: account_manager::AccountManager::new(),
            active_account: None,
            db,
            table_entries: Vec::new(),
            profile_metadata: HashMap::new(),
            contacts: Vec::new(),
            contact_images: HashMap::new(),
            pending_contact_images: HashSet::new(),
            failed_contact_images: HashSet::new(),
            image_request_sender,
            image_request_receiver,
            drafts: Vec::new(),
        }
    }

    fn refresh_drafts(&mut self) {
        match self.db.get_drafts() {
            Ok(drafts) => self.drafts = drafts,
            Err(e) => error!("Failed to load drafts: {}", e),
        }
    }

    /// Resolve the best display name for a pubkey: petname > display_name > name > pubkey.
    fn resolve_name(&self, pubkey: &str) -> Option<String> {
        // Check contacts for petname first
        if let Some(contact) = self.contacts.iter().find(|c| c.pubkey == pubkey) {
            if contact.petname.is_some() {
                return contact.petname.clone();
            }
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

    fn contact_sort_key(contact: &Contact) -> String {
        contact
            .petname
            .clone()
            .or(contact.metadata.display_name.clone())
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
        // Only update metadata for contacts that already exist in our list.
        // New contacts are added explicitly by the user via the UI.
        if let Some(existing) = self.contacts.iter_mut().find(|c| c.pubkey == pubkey) {
            let previous_picture = existing.metadata.picture.clone();
            existing.metadata = metadata.clone();
            if previous_picture != existing.metadata.picture {
                self.contact_images.remove(&existing.pubkey);
                self.pending_contact_images.remove(&existing.pubkey);
                self.failed_contact_images.remove(&existing.pubkey);
            }
            self.contacts
                .sort_by(|a, b| Self::contact_sort_key(a).cmp(&Self::contact_sort_key(b)));
        }

        self.profile_metadata
            .insert(pubkey, ProfileOption::Some(metadata));
    }

    fn add_contact(&mut self, pubkey: String, petname: Option<String>, metadata: ProfileMetadata) {
        if self.contacts.iter().any(|c| c.pubkey == pubkey) {
            return;
        }

        if let Err(e) = self.db.save_contact(&pubkey, petname.as_deref()) {
            error!("Failed to save contact to database: {}", e);
            return;
        }

        self.contacts.push(Contact {
            pubkey: pubkey.clone(),
            petname,
            metadata: metadata.clone(),
        });
        self.contacts
            .sort_by(|a, b| Self::contact_sort_key(a).cmp(&Self::contact_sort_key(b)));

        self.profile_metadata
            .insert(pubkey, ProfileOption::Some(metadata));
    }

    fn remove_contact(&mut self, pubkey: &str) {
        if let Err(e) = self.db.delete_contact(pubkey) {
            error!("Failed to delete contact from database: {}", e);
            return;
        }

        self.contacts.retain(|c| c.pubkey != pubkey);
        self.contact_images.remove(pubkey);
        self.pending_contact_images.remove(pubkey);
        self.failed_contact_images.remove(pubkey);
    }

    fn update_contact_petname(&mut self, pubkey: &str, petname: Option<String>) {
        if let Err(e) = self.db.update_contact_petname(pubkey, petname.as_deref()) {
            error!("Failed to update contact petname in database: {}", e);
            return;
        }

        if let Some(contact) = self.contacts.iter_mut().find(|c| c.pubkey == pubkey) {
            contact.petname = petname;
        }
        self.contacts
            .sort_by(|a, b| Self::contact_sort_key(a).cmp(&Self::contact_sort_key(b)));
    }
}

fn render_contacts_page(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Contacts");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("+ Add Contact").clicked() {
                app.state.contacts.show_add_form = !app.state.contacts.show_add_form;
                app.state.contacts.add_error = None;
            }
        });
    });

    ui.add_space(8.0);

    // Add contact form
    if app.state.contacts.show_add_form {
        Frame::none()
            .fill(style::CARD_BG)
            .stroke(Stroke::new(1.0, style::CARD_STROKE))
            .inner_margin(Margin::symmetric(16.0, 12.0))
            .rounding(8.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Add New Contact").strong());
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label("Public Key:");
                    ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(&mut app.state.contacts.add_pubkey_input)
                            .hint_text("npub1... or hex pubkey"),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Petname:     ");
                    ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(&mut app.state.contacts.add_petname_input)
                            .hint_text("Optional nickname for this contact"),
                    );
                });

                if let Some(err) = &app.state.contacts.add_error {
                    ui.colored_label(Color32::RED, err.clone());
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        let raw = app.state.contacts.add_pubkey_input.trim().to_string();
                        let petname_raw = app.state.contacts.add_petname_input.trim().to_string();
                        let petname = if petname_raw.is_empty() {
                            None
                        } else {
                            Some(petname_raw)
                        };

                        // Try parsing the pubkey
                        use nostr::FromBech32;
                        let parsed = nostr::PublicKey::from_bech32(&raw)
                            .or_else(|_| nostr::PublicKey::from_hex(&raw));

                        match parsed {
                            Ok(pk) => {
                                let pk_hex = pk.to_string();
                                if app.contacts.iter().any(|c| c.pubkey == pk_hex) {
                                    app.state.contacts.add_error =
                                        Some("Contact already exists.".to_string());
                                } else {
                                    // Grab whatever metadata we already have cached
                                    let metadata = app
                                        .profile_metadata
                                        .get(&pk_hex)
                                        .and_then(|opt| match opt {
                                            ProfileOption::Some(m) => Some(m.clone()),
                                            _ => None,
                                        })
                                        .unwrap_or_default();

                                    app.add_contact(pk_hex, petname, metadata);

                                    // Reset form
                                    app.state.contacts.add_pubkey_input.clear();
                                    app.state.contacts.add_petname_input.clear();
                                    app.state.contacts.show_add_form = false;
                                    app.state.contacts.add_error = None;
                                }
                            }
                            Err(_) => {
                                app.state.contacts.add_error = Some(
                                    "Invalid public key. Use npub1... or 64-char hex.".to_string(),
                                );
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        app.state.contacts.show_add_form = false;
                        app.state.contacts.add_pubkey_input.clear();
                        app.state.contacts.add_petname_input.clear();
                        app.state.contacts.add_error = None;
                    }
                });
            });
        ui.add_space(8.0);
    }

    if app.contacts.is_empty() {
        ui.label("No contacts yet. Add one above!");
        return;
    }

    // Track actions to apply after the loop (can't mutate app while iterating)
    let mut contact_to_remove: Option<String> = None;
    let mut petname_to_save: Option<(String, Option<String>)> = None;

    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let total = app.contacts.len();

            for index in 0..total {
                let contact = app.contacts[index].clone();
                app.ensure_contact_image_request(&contact);

                let is_editing =
                    app.state.contacts.editing_pubkey.as_ref() == Some(&contact.pubkey);

                Frame::none()
                    .fill(style::CARD_BG)
                    .stroke(Stroke::new(1.0, style::CARD_STROKE))
                    .inner_margin(Margin::symmetric(16.0, 12.0))
                    .rounding(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            draw_contact_avatar(app, ui, &contact);
                            ui.add_space(12.0);

                            ui.vertical(|ui| {
                                // Petname row (editable)
                                if is_editing {
                                    ui.horizontal(|ui| {
                                        ui.label("Petname:");
                                        ui.text_edit_singleline(
                                            &mut app.state.contacts.editing_petname_buf,
                                        );
                                        if ui.button("Save").clicked() {
                                            let new_petname = app
                                                .state
                                                .contacts
                                                .editing_petname_buf
                                                .trim()
                                                .to_string();
                                            let petname = if new_petname.is_empty() {
                                                None
                                            } else {
                                                Some(new_petname)
                                            };
                                            petname_to_save =
                                                Some((contact.pubkey.clone(), petname));
                                            app.state.contacts.editing_pubkey = None;
                                        }
                                        if ui.button("Cancel").clicked() {
                                            app.state.contacts.editing_pubkey = None;
                                        }
                                    });
                                } else {
                                    let display = contact.display_name();
                                    ui.label(RichText::new(&display).strong());

                                    if let Some(petname) = &contact.petname {
                                        // Show the nostr name underneath the petname
                                        if let Some(nostr_name) = contact
                                            .metadata
                                            .display_name
                                            .as_ref()
                                            .or(contact.metadata.name.as_ref())
                                        {
                                            if nostr_name != petname {
                                                ui.label(
                                                    RichText::new(format!("({})", nostr_name))
                                                        .small()
                                                        .color(style::TEXT_MUTED),
                                                );
                                            }
                                        }
                                    } else if let Some(name) = contact.metadata.name.as_ref() {
                                        if contact.metadata.display_name.as_ref() != Some(name) {
                                            ui.label(name);
                                        }
                                    }

                                    ui.label(
                                        RichText::new(&contact.pubkey)
                                            .monospace()
                                            .small()
                                            .color(style::TEXT_MUTED),
                                    );
                                }
                            });

                            // Action buttons pushed to right
                            if !is_editing {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button(RichText::new("X").color(Color32::RED))
                                            .on_hover_text("Remove contact")
                                            .clicked()
                                        {
                                            contact_to_remove = Some(contact.pubkey.clone());
                                        }

                                        if ui.button("Edit").on_hover_text("Edit petname").clicked()
                                        {
                                            app.state.contacts.editing_pubkey =
                                                Some(contact.pubkey.clone());
                                            app.state.contacts.editing_petname_buf =
                                                contact.petname.clone().unwrap_or_default();
                                        }
                                    },
                                );
                            }
                        });
                    });

                ui.add_space(4.0);
            }
        });

    // Apply deferred mutations
    if let Some(pubkey) = contact_to_remove {
        app.remove_contact(&pubkey);
    }
    if let Some((pubkey, petname)) = petname_to_save {
        app.update_contact_petname(&pubkey, petname);
    }
}

fn draw_contact_avatar(app: &Hoot, ui: &mut egui::Ui, contact: &Contact) {
    let size = Vec2::splat(CONTACT_AVATAR_SIZE);

    if let Some(texture) = app.contact_images.get(&contact.pubkey) {
        ui.add(egui::Image::new((texture.id(), size)).maintain_aspect_ratio(true));
        return;
    }

    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.circle_filled(rect.center(), CONTACT_AVATAR_SIZE / 2.0, style::ACCENT);
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
