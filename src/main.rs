#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use eframe::egui::{self, FontDefinitions, Sense, Vec2b};
use egui::FontFamily::Proportional;
use egui_extras::{Column, TableBuilder};
use relay::RelayMessage;
use std::collections::HashMap;
use tracing::{debug, error, info, Level};

mod account_manager;
mod db;
mod error;
mod keystorage;
mod mail_event;
mod relay;
mod ui;
// not sure if i will use this but i'm committing it for later.
// mod threaded_event;

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
}

// for storing the state of different components and such.
#[derive(Default)]
pub struct HootState {
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
    db: db::Db,
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
    let wake_up = move || {
        ctx.request_repaint();
    };

    if app.status == HootStatus::Initalizing {
        info!("Initalizing Hoot...");
        match app.account_manager.load_keys() {
            Ok(..) => {}
            Err(v) => error!("something went wrong trying to load keys: {}", v),
        }
        let _ = app
            .relays
            .add_url("wss://relay.chakany.systems".to_string(), wake_up.clone());

        if app.account_manager.loaded_keys.len() > 0 {
            let mut gw_sub = relay::Subscription::default();

            let filter = nostr::Filter::new().kind(nostr::Kind::GiftWrap).custom_tag(
                nostr::SingleLetterTag {
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
        // Check if we already have this event
        if let Ok(has_event) = app.db.has_event(&event.id.to_string()) {
            if has_event {
                debug!("Skipping already stored event: {}", event.id);
                return;
            }
        }

        // Verify the event signature
        if event.verify().is_ok() {
            debug!("Verified event: {:?}", event);

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

fn render_app(app: &mut Hoot, ctx: &egui::Context) {
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
                if ui.button("onboarding").clicked() {
                    app.page = Page::OnboardingNew;
                }

                // Add flexible space to push profile to bottom
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    // Profile section
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized([32.0, 32.0], egui::Button::new("👤"))
                            .clicked()
                        {
                            app.page = Page::Settings;
                        }
                        if let Some(key) = app.account_manager.loaded_keys.first() {
                            ui.label(&key.public_key().to_string()[..8]);
                        }
                    });
                });
            });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        match app.page {
            Page::Inbox => {
                // Top bar with search
                ui.horizontal(|ui| {
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
                            ui.label("Content");
                        });
                        header.col(|ui| {
                            ui.label("Time");
                        });
                    })
                    .body(|mut body| {
                        let row_height = 30.0;
                        let events = app.events.clone();
                        body.rows(row_height, events.len(), |mut row| {
                            let event = &events[row.index()];

                            row.col(|ui| {
                                ui.checkbox(&mut false, "");
                            });
                            row.col(|ui| {
                                ui.checkbox(&mut false, "");
                            });
                            row.col(|ui| {
                                ui.label(event.pubkey.to_string());
                            });
                            row.col(|ui| {
                                // Try to get subject from tags
                                let subject = match &event.tags.find(nostr::TagKind::Subject) {
                                    Some(s) => match s.content() {
                                        Some(c) => format!("{}: {}", c.to_string(), event.content),
                                        None => event.content.clone(),
                                    },
                                    None => event.content.clone(),
                                };
                                ui.label(subject);
                            });
                            row.col(|ui| {
                                ui.label("2 minutes ago");
                            });

                            if row.response().clicked() {
                                app.focused_post = event.id.to_string();
                                app.page = Page::Post;
                            }
                        });
                    });
            }
            Page::Settings => {
                ui::settings::SettingsScreen::ui(app, ui);
            }
            Page::Post => {
                if let Some(event) = app
                    .events
                    .iter()
                    .find(|e| e.id.to_string() == app.focused_post)
                {
                    if let Ok(unwrapped) = app.account_manager.unwrap_gift_wrap(event) {
                        // Message header section
                        ui.add_space(8.0);
                        ui.heading(
                            &unwrapped
                                .rumor
                                .tags
                                .find(nostr::TagKind::Subject)
                                .and_then(|s| s.content())
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "No Subject".to_string()),
                        );

                        // Metadata grid
                        egui::Grid::new("email_metadata")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("From");
                                ui.label(unwrapped.sender.to_string());
                                ui.end_row();

                                ui.label("To");
                                ui.label(
                                    unwrapped
                                        .rumor
                                        .tags
                                        .iter()
                                        .filter_map(|tag| tag.content())
                                        .next()
                                        .unwrap_or_else(|| "Unknown")
                                        .to_string(),
                                );
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
                                // TODO: Handle reply
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
                        ui.label(&unwrapped.rumor.content);
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
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
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

        Self {
            page: Page::Inbox,
            focused_post: "".into(),
            status: HootStatus::Initalizing,
            state: Default::default(),
            relays: relay::RelayPool::new(),
            events: Vec::new(),
            account_manager: account_manager::AccountManager::new(),
            db,
        }
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
