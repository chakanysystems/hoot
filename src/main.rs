#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use eframe::egui::{self, FontDefinitions, Sense, Vec2b};
use egui::FontFamily::Proportional;
use egui_extras::{Column, TableBuilder};
use std::collections::HashMap;
use tracing::{debug, error, info, Level};

fn truncate_string(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut truncated: String = s.chars().take(max_chars - 3).collect();
        truncated.push_str("...");
        truncated
    }
}

mod account_manager;
mod error;
mod keystorage;
mod mail_event;
mod relay;
mod ui;

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
            let mut style = egui::Style::default();
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.spacing.window_margin = egui::Margin::same(16.0);
            style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.active.rounding = egui::Rounding::same(8.0);
            style.visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
            style.visuals.window_rounding = egui::Rounding::same(10.0);
            
            // Custom colors
            let accent_color = egui::Color32::from_rgb(79, 70, 229); // Indigo
            style.visuals.selection.bg_fill = accent_color;
            style.visuals.widgets.active.bg_fill = accent_color;
            style.visuals.widgets.active.weak_bg_fill = accent_color.linear_multiply(0.3);
            style.visuals.widgets.hovered.bg_fill = accent_color.linear_multiply(0.8);
            style.visuals.hyperlink_color = accent_color;
            
            let _ = &cc.egui_ctx.set_style(style);
            
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

            let filter = nostr::Filter::new().kind(nostr::Kind::Custom(mail_event::MAIL_EVENT_KIND)).custom_tag(nostr::SingleLetterTag { character: nostr::Alphabet::P, uppercase: false }, app.account_manager.loaded_keys.clone().into_iter().map(|keys| keys.public_key()));
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
        // Verify the event signature
        if event.verify().is_ok() {
            debug!("Verified event: {:?}", event);
            app.events.push(event);
        } else {
            error!("Event verification failed");
        }
    } else {
        error!("Failed to parse event JSON: {}", event_json);
    }
}

fn render_app(app: &mut Hoot, ctx: &egui::Context) {
    #[cfg(feature = "profiling")]
    puffin::profile_function!();

    if app.page == Page::Onboarding
        || app.page == Page::OnboardingNew
        || app.page == Page::OnboardingNewShowKey
        || app.page == Page::OnboardingReturning
    {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui::onboarding::OnboardingScreen::ui(app, ui);
        });
    } else {
        egui::SidePanel::left("Side Navbar")
            .resizable(false)
            .default_width(200.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    ui.heading("🦉 Hoot");
                    ui.add_space(16.0);
                });
                
                ui.separator();
                ui.add_space(8.0);

                let nav_button = |ui: &mut egui::Ui, text: &str, icon: &str, selected: bool| {
                    let response = ui.add(egui::Button::new(
                        egui::RichText::new(format!("{} {}", icon, text))
                            .size(16.0)
                    ).fill(if selected {
                        ui.style().visuals.widgets.active.weak_bg_fill
                    } else {
                        ui.style().visuals.widgets.inactive.bg_fill
                    }));
                    response.clicked()
                };

                if nav_button(ui, "Inbox", "📥", app.page == Page::Inbox) {
                    app.page = Page::Inbox;
                }
                if nav_button(ui, "Drafts", "📝", app.page == Page::Drafts) {
                    app.page = Page::Drafts;
                }
                
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
                
                if nav_button(ui, "Settings", "⚙️", app.page == Page::Settings) {
                    app.page = Page::Settings;
                }
                if nav_button(ui, "Onboarding", "🚀", app.page == Page::Onboarding) {
                    app.page = Page::Onboarding;
                }
            });

        egui::TopBottomPanel::top("Search")
            .exact_height(60.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    let search_width = ui.available_width() - 16.0;
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut String::new())
                            .hint_text("Search messages...")
                            .desired_width(search_width)
                    );
                    if search.gained_focus() {
                        // TODO: Handle search focus
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            // todo: fix
            for window_id in app.state.compose_window.clone().into_keys() {
                ui::compose_window::ComposeWindow::show(app, ui, window_id);
            }

            if app.page == Page::Inbox {
                ui.horizontal(|ui| {
                    ui.heading("Inbox");
                    ui.add_space(ui.available_width() - 120.0); // Push compose to right
                    if ui.add(egui::Button::new(
                        egui::RichText::new("✏️ Compose")
                            .size(16.0)
                    ).min_size(egui::vec2(100.0, 32.0))).clicked() {
                        let state = ui::compose_window::ComposeWindowState {
                            subject: String::new(),
                            to_field: String::new(),
                            content: String::new(),
                            selected_account: None,
                        };
                        app.state
                            .compose_window
                            .insert(egui::Id::new(rand::random::<u32>()), state);
                    }
                });
                
                ui.add_space(8.0);

                // Debug buttons in collapsing section
                if ui.collapsing("Debug Controls", |ui| {
                    if ui.button("Send Test Event").clicked() {
                        let temp_keys = nostr::Keys::generate();
                        let new_event = nostr::EventBuilder::text_note("GFY!")
                            .sign_with_keys(&temp_keys)
                            .unwrap();
                        let event_json = crate::relay::ClientMessage::Event { event: new_event };
                        let _ = &app
                            .relays
                            .send(ewebsock::WsMessage::Text(
                                serde_json::to_string(&event_json).unwrap(),
                            ))
                            .unwrap();
                    }

                    if ui.button("Get kind 1 notes").clicked() {
                        let mut filter = nostr::types::Filter::new();
                        filter = filter.kind(nostr::Kind::TextNote);
                        let mut sub = crate::relay::Subscription::default();
                        sub.filter(filter);
                        let c_msg = crate::relay::ClientMessage::from(sub);

                        let _ = &app
                            .relays
                            .send(ewebsock::WsMessage::Text(
                                serde_json::to_string(&c_msg).unwrap(),
                            ))
                            .unwrap();
                    }
                    ui.label(format!("Total events: {}", app.events.len()));
                }).body_returned {
                    ui.add_space(8.0);
                }

                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(30.0)) // Select
                    .column(Column::auto().at_least(30.0)) // Star
                    .column(Column::remainder().at_least(200.0)) // From
                    .column(Column::remainder().at_least(300.0)) // Content
                    .column(Column::remainder().at_least(120.0)) // Time
                    .header(24.0, |mut header| {
                        header.col(|ui| {
                            ui.checkbox(&mut false, "");
                        });
                        header.col(|ui| {
                            ui.label("⭐");
                        });
                        header.col(|ui| {
                            ui.strong("From");
                        });
                        header.col(|ui| {
                            ui.strong("Content");
                        });
                        header.col(|ui| {
                            ui.strong("Time");
                        });
                    });

                table.body(|mut body| {
                    let row_height = 40.0;
                    let events = app.events.clone();
                    body.rows(row_height, events.len(), |mut row| {
                        let event = &events[row.index()];
                        let is_selected = app.focused_post == event.id.to_string();
                        
                        let row_response = row.row_ui(|mut row| {
                            row.col(|ui| {
                                ui.checkbox(&mut false, "");
                            });
                            row.col(|ui| {
                                ui.label("☆");
                            });
                            row.col(|ui| {
                                ui.label(egui::RichText::new(truncate_string(&event.pubkey.to_string(), 20))
                                    .strong(is_selected));
                            });
                            row.col(|ui| {
                                ui.label(egui::RichText::new(truncate_string(&event.content, 50))
                                    .strong(is_selected));
                            });
                            row.col(|ui| {
                                ui.label(egui::RichText::new("2 minutes ago")
                                    .strong(is_selected));
                            });
                        });

                        if row_response.response.clicked() {
                            app.focused_post = event.id.to_string();
                            app.page = Page::Post;
                        }

                        // Highlight on hover
                        if row_response.response.hovered() {
                            row_response.response.highlight();
                        }
                    });
                });
            } else if app.page == Page::Settings {
                ui.heading("Settings");
                ui::settings::SettingsScreen::ui(app, ui);
            } else if app.page == Page::Post {
                assert!(
                    !app.focused_post.is_empty(),
                    "focused_post should not be empty when Page::Post"
                );

                let gift_wrapped_event = app
                    .events
                    .iter()
                    .find(|&x| x.id.to_string() == app.focused_post)
                    .expect("event id should be present inside event list");

                let event_to_display = app.account_manager.unwrap_gift_wrap(gift_wrapped_event).expect("we should be able to unwrap an event we recieved");

                ui.heading("View Message");
                ui.label(format!("Content: {}", event_to_display.rumor.content));
                ui.label(match &event_to_display.rumor.tags.find(nostr::TagKind::Subject) {
                    Some(s) => match s.content() {
                        Some(c) => format!("Subject: {}", c.to_string()),
                        None => "Subject: None".to_string(),
                    },
                    None => "Subject: None".to_string(),
                });

                ui.label(match &event_to_display.rumor.id {
                    Some(id) => format!("ID: {}", id.to_string()),
                    None => "ID: None".to_string(),
                });

                ui.label(format!("Author: {}", event_to_display.sender.to_string()));
            }
        });
    }
}

impl Hoot {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let storage_dir = eframe::storage_dir("Hoot").unwrap();
        Self {
            page: Page::Inbox,
            focused_post: "".into(),
            status: HootStatus::Initalizing,
            state: Default::default(),
            relays: relay::RelayPool::new(),
            events: Vec::new(),
            account_manager: account_manager::AccountManager::new(),
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
