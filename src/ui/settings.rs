use crate::Hoot;
use eframe::egui::{self, Color32, Direction, Layout, Sense, Ui, Vec2};
use egui_tabs::Tabs;
use tracing::error;

#[derive(Default)]
pub struct SettingsState {
    pub new_relay_url: String,
}

enum Tab {
    Profile = 0,
    Relays = 1,
    Identity = 2,
}

impl From<i32> for Tab {
    fn from(value: i32) -> Self {
        match value {
            0 => Tab::Profile,
            1 => Tab::Relays,
            2 => Tab::Identity,
            _ => Tab::Profile, // Default to Profile for invalid values
        }
    }
}

impl From<Tab> for i32 {
    fn from(tab: Tab) -> Self {
        tab as i32
    }
}

pub struct SettingsScreen {}

impl SettingsScreen {
    pub fn ui(app: &mut Hoot, ui: &mut Ui) {
        let tabs_response = Tabs::new(3)
            .height(16.0)
            .selected(0)
            .layout(Layout::centered_and_justified(Direction::TopDown))
            .show(ui, |ui, state| {
                let current_tab = Tab::from(state.index());
                use Tab::*;
                let tab_label = match current_tab {
                    Profile => "My Profile",
                    Relays => "Relays",
                    Identity => "Keys",
                };
                ui.add(egui::Label::new(tab_label).selectable(false));
            });
        let current_tab: Tab = tabs_response.selected().unwrap().into();

        use Tab::*;
        match current_tab {
            Profile => Self::profile(app, ui),
            Relays => Self::relays(app, ui),
            Identity => Self::identity(app, ui),
        }
    }

    fn profile(app: &mut Hoot, ui: &mut Ui) {
        ui.label("Your profile.");
    }

    fn relays(app: &mut Hoot, ui: &mut Ui) {
        ui.heading("Relays");
        ui.small("A relay is a server that Hoot connects with to send & receive messages.");

        ui.label("Add New Relay:");
        ui.horizontal(|ui| {
            let new_relay = &mut app.state.settings.new_relay_url;
            ui.text_edit_singleline(new_relay);
            if ui.button("Add Relay").clicked() && !new_relay.is_empty() {
                let ctx = ui.ctx().clone();
                let wake_up = move || {
                    ctx.request_repaint();
                };
                app.relays.add_url(new_relay.clone(), wake_up);
                app.state.settings.new_relay_url = String::new(); // clears field
            }
        });

        ui.add_space(10.0);

        ui.label("Your Relays:");
        ui.vertical(|ui| {
            let mut relay_to_remove: Option<String> = None;
            let last_ping = app.relays.get_last_reconnect_attempt();
            for (url, relay) in app.relays.relays.iter() {
                ui.horizontal(|ui| {
                    use crate::relay::RelayStatus::*;
                    let conn_fill: Color32 = match relay.status {
                        Connecting => Color32::YELLOW,
                        Connected => Color32::LIGHT_GREEN,
                        Disconnected => Color32::RED,
                    };

                    let size = Vec2::splat(12.0);
                    let (response, painter) = ui.allocate_painter(size, Sense::hover());
                    let rect = response.rect;
                    let c = rect.center();
                    let r = rect.width() / 2.0 - 1.0;
                    painter.circle_filled(c, r, conn_fill);

                    ui.label(url);
                    // TODO: this only updates when next frame is rendered, which can be more than
                    // a few seconds between renders. Make it so it updates every second.
                    if relay.status == crate::relay::RelayStatus::Disconnected {
                        let next_ping =
                            crate::relay::RELAY_RECONNECT_SECONDS - last_ping.elapsed().as_secs();

                        ui.label(format!("(Attempting reconnect in {} seconds)", next_ping));
                    }
                    if ui.button("Remove Relay").clicked() {
                        relay_to_remove = Some(url.to_string());
                    }
                });
            }

            if relay_to_remove.is_some() {
                app.relays.remove_url(&relay_to_remove.unwrap());
            }
        });
    }

    fn identity(app: &mut Hoot, ui: &mut Ui) {
        ui.vertical(|ui| {
            use nostr::ToBech32;
            for key in app.account_manager.loaded_keys.clone() {
                ui.horizontal(|ui| {
                    ui.label(format!("Key ID: {}", key.public_key().to_bech32().unwrap()));
                    if ui.button("Remove Key").clicked() {
                        match app.account_manager.delete_key(&key) {
                            Ok(..) => {}
                            Err(v) => error!("couldn't remove key: {}", v),
                        }
                    }
                });
            }
        });
    }
}
