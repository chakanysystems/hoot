#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // for windows release

use eframe::egui::{self, FontDefinitions, Sense, Vec2b};
use egui::FontFamily::Proportional;
use egui_extras::{Column, TableBuilder};
use std::collections::HashMap;
use tracing::{debug, error, info, Level};
use tokio::runtime;

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
    rt: runtime::Runtime,
    pub page: Page,
    focused_post: String,
    status: HootStatus,
    state: HootState,
    relays: relay::RelayPool,
    ndb: nostrdb::Ndb,
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

        use relay::RelayMessage;
        let deserialized: RelayMessage =
            serde_json::from_str(new_val.unwrap().as_str()).expect("relay sent us bad json");

        use RelayMessage::*;
        match deserialized {
            Event {
                subscription_id,
                event,
            } => {
                app.events.push(event);
            }
            _ => {
                // who cares rn
            }
        }
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
        egui::SidePanel::left("Side Navbar").show(ctx, |ui| {
            ui.heading("Hoot");
            if ui.button("Inbox").clicked() {
                app.page = Page::Inbox;
            }
            if ui.button("Drafts").clicked() {
                app.page = Page::Drafts;
            }
            if ui.button("Settings").clicked() {
                app.page = Page::Settings;
            }
            if ui.button("Onboarding").clicked() {
                app.page = Page::Onboarding;
            }
        });

        egui::TopBottomPanel::top("Search").show(ctx, |ui| {
            ui.heading("Search");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // todo: fix
            for window_id in app.state.compose_window.clone().into_keys() {
                ui::compose_window::ComposeWindow::show(app, ui, window_id);
            }

            if app.page == Page::Inbox {
                ui.label("hello there!");
                if ui.button("Compose").clicked() {
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

                if ui.button("Send Test Event").clicked() {
                    let temp_keys = nostr::Keys::generate();
                    // todo: lmao
                    let new_event = nostr::EventBuilder::text_note("GFY!", [])
                        .to_event(&temp_keys)
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

                ui.label(format!("total events rendered: {}", app.events.len()));

                TableBuilder::new(ui)
                    .column(Column::auto())
                    .column(Column::auto())
                    .column(Column::remainder())
                    .column(Column::remainder())
                    .column(Column::remainder())
                    .striped(true)
                    .sense(Sense::click())
                    .auto_shrink(Vec2b { x: false, y: false })
                    .header(20.0, |_header| {})
                    .body(|mut body| {
                        let row_height = 30.0;
                        let events = app.events.clone();
                        body.rows(row_height, events.len(), |mut row| {
                            let row_index = row.index();
                            row.col(|ui| {
                                ui.checkbox(&mut false, "");
                            });
                            row.col(|ui| {
                                ui.checkbox(&mut false, "");
                            });
                            row.col(|ui| {
                                ui.label(events[row_index].pubkey.to_string());
                            });
                            row.col(|ui| {
                                ui.label(events[row_index].content.clone());
                            });
                            row.col(|ui| {
                                ui.label("2 minutes ago");
                            });

                            if row.response().clicked() {
                                println!("clicked: {}", events[row_index].content.clone());
                                app.focused_post = events[row_index].id().to_string();
                                app.page = Page::Post;
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

                let event_to_display = app
                    .events
                    .iter()
                    .find(|&x| x.id().to_string() == app.focused_post)
                    .expect("event id should be present inside event list");

                ui.heading("View Message");
                ui.label(format!("Content: {}", event_to_display.content));
                ui.label(format!("ID: {}", event_to_display.id().to_string()));
                ui.label(format!("Author: {}", event_to_display.pubkey.to_string()));
            }
        });
    }
}

impl Hoot {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let storage_dir = eframe::storage_dir("Hoot").unwrap();
        let mut ndb_config = nostrdb::Config::new();
        ndb_config.set_ingester_threads(3);

        let ndb = nostrdb::Ndb::new(storage_dir.to_str().unwrap(), &ndb_config)
            .expect("could not load nostrdb");
        Self {
            rt: runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
            page: Page::Inbox,
            focused_post: "".into(),
            status: HootStatus::Initalizing,
            state: Default::default(),
            relays: relay::RelayPool::new(),
            ndb,
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
