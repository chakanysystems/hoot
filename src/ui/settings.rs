use crate::{
    profile_metadata::{ProfileMetadata, ProfileOption},
    Hoot,
};
use eframe::egui::{Color32, Sense, Ui, Vec2};
use std::cell::RefCell;
use std::collections::HashMap;
use tracing::error;

#[derive(Debug, Default)]
pub struct ProfileMetadataEditingStatus {
    display_name: String,
    editing: bool,
}

#[derive(Debug, Default)]
pub struct Nip05EditingState {
    pub new_nip05: String,
    pub verification_error: Option<String>,
}

#[derive(Debug, Default)]
pub struct SettingsState {
    pub new_relay_url: String,
    pub editing_display_name: bool,
    pub new_display_name: String,
    pub metadata_state: HashMap<String, RefCell<ProfileMetadataEditingStatus>>,
    pub nip05_state: HashMap<String, RefCell<Nip05EditingState>>,
}

pub struct SettingsScreen {}

impl SettingsScreen {
    pub fn ui(app: &mut Hoot, ui: &mut Ui) {
        ui.vertical(|ui| {
            Self::profile(app, ui);
            ui.add_space(15.0);
            Self::relays(app, ui);
        });
    }

    fn profile(app: &mut Hoot, ui: &mut Ui) {
        ui.heading("Your profiles");
        use nostr::ToBech32;
        for key in app.account_manager.loaded_keys.clone() {
            // Get metadata about key
            let pk_hex = key.public_key().to_hex();
            if !app.state.settings.metadata_state.contains_key(&pk_hex) {
                app.state.settings.metadata_state.insert(
                    pk_hex.clone(),
                    RefCell::new(ProfileMetadataEditingStatus::default()),
                );
            }

            ui.horizontal(|ui| {
                ui.label(format!("Key ID: {}", key.public_key().to_bech32().unwrap()));
                if ui.button("Remove Key").clicked() {
                    match app.account_manager.delete_key(&app.db, &key) {
                        Ok(..) => {}
                        Err(v) => error!("couldn't remove key: {}", v),
                    }
                }
            });

            let profile_metadata =
                crate::profile_metadata::get_profile_metadata(app, pk_hex.clone()).clone();

            ui.horizontal(|ui| {
                let key_meta_state = app
                    .state
                    .settings
                    .metadata_state
                    .get(&pk_hex)
                    .expect("This should have been created already");

                // Track button actions and new name outside the borrow scope.
                let mut save_clicked = false;
                let mut cancel_clicked = false;
                let mut edit_clicked = false;
                let mut new_name_to_save: Option<String> = None;

                {
                    // Single mutable borrow of the RefCell; ends before we call functions needing &mut app.
                    let mut meta_state = key_meta_state.borrow_mut();
                    let is_editing = meta_state.editing;

                    match profile_metadata.clone() {
                        ProfileOption::Some(meta) => {
                            if let Some(display_name) = &meta.display_name {
                                if is_editing {
                                    ui.label("Display Name: ");
                                    ui.text_edit_singleline(&mut meta_state.display_name);
                                    if ui.button("Cancel").clicked() {
                                        cancel_clicked = true;
                                    }
                                    if ui.button("Save").clicked() {
                                        save_clicked = true;
                                        new_name_to_save = Some(meta_state.display_name.clone());
                                    }
                                } else {
                                    ui.label(format!("Display Name: {}", display_name));
                                }
                            } else {
                                ui.label("Display Name: Not Found");
                            }
                        }
                        ProfileOption::Waiting => {
                            if is_editing {
                                ui.label("Display Name: ");
                                ui.text_edit_singleline(&mut meta_state.display_name);
                                if ui.button("Cancel").clicked() {
                                    cancel_clicked = true;
                                }
                                if ui.button("Save").clicked() {
                                    save_clicked = true;
                                    new_name_to_save = Some(meta_state.display_name.clone());
                                }
                            } else {
                                ui.label("Display Name: Not Found");
                            }
                        }
                    }

                    if !is_editing {
                        if ui.button("Edit").clicked() {
                            edit_clicked = true;
                        }
                    }

                    if edit_clicked {
                        meta_state.editing = true;
                    }
                    if cancel_clicked {
                        meta_state.editing = false;
                    }
                    if save_clicked {
                        meta_state.editing = false;
                    }
                } // borrow ends here

                if save_clicked {
                    if let Some(new_name) = new_name_to_save {
                        let mut new_meta = match &profile_metadata {
                            ProfileOption::Some(meta) => meta.to_owned(),
                            ProfileOption::Waiting => ProfileMetadata::default(),
                        };
                        new_meta.display_name = Some(new_name);
                        match crate::profile_metadata::update_logged_in_profile_metadata(
                            app,
                            key.public_key(),
                            new_meta,
                        ) {
                            Ok(()) => (),
                            Err(e) => error!("Couldn't update logged in profile metadata: {}", e),
                        }
                    }
                }
            });

            // NIP-05 Management Section
            ui.add_space(4.0);
            Self::nip05_management(app, ui, &pk_hex);

            ui.add_space(8.0);
        }
    }

    fn nip05_management(app: &mut Hoot, ui: &mut Ui, pk_hex: &str) {
        // Ensure NIP-05 state exists for this key
        if !app.state.settings.nip05_state.contains_key(pk_hex) {
            app.state.settings.nip05_state.insert(
                pk_hex.to_string(),
                RefCell::new(Nip05EditingState::default()),
            );
        }

        // Get existing NIP-05s for this key
        let nip05s = match app.db.get_nip05s_for_pubkey(pk_hex) {
            Ok(entries) => entries,
            Err(e) => {
                error!("Failed to get NIP-05s for key: {}", e);
                Vec::new()
            }
        };

        ui.label("NIP-05 Identifiers:");

        // Display existing NIP-05s
        for entry in &nip05s {
            ui.horizontal(|ui| {
                let (icon, color, _tooltip) = entry.status_display();

                ui.colored_label(color, icon);
                ui.label(&entry.nip05);

                if entry.is_own {
                    ui.small("(own)");
                }

                if ui.button("Remove").clicked() {
                    if let Err(e) = app.db.delete_nip05(pk_hex, &entry.nip05) {
                        error!("Failed to delete NIP-05: {}", e);
                    }
                }

                if ui.button("Verify").clicked() {
                    app.nip05_verifier
                        .request(entry.nip05.clone(), pk_hex.to_string());
                }
            });
        }

        if nip05s.is_empty() {
            ui.small("No NIP-05 identifiers added yet.");
        }

        // Add new NIP-05 form
        ui.horizontal(|ui| {
            let nip05_state = app
                .state
                .settings
                .nip05_state
                .get(pk_hex)
                .expect("State should exist");

            let mut verify_clicked = false;
            let new_nip05_value;

            {
                let mut state = nip05_state.borrow_mut();
                ui.text_edit_singleline(&mut state.new_nip05);
                new_nip05_value = state.new_nip05.clone();

                if ui.button("Verify & Add").clicked() {
                    verify_clicked = true;
                }

                // Show any verification error
                if let Some(ref error) = state.verification_error {
                    ui.colored_label(Color32::RED, error);
                }
            }

            if verify_clicked && !new_nip05_value.is_empty() {
                let mut state = nip05_state.borrow_mut();

                // Validate the NIP-05 format
                if crate::nip05::parse_nip05(&new_nip05_value).is_none() {
                    state.verification_error =
                        Some("Invalid NIP-05 format. Use: user@domain.com".to_string());
                } else {
                    // Save to DB and enqueue background verification
                    if let Err(e) = app.db.add_nip05(pk_hex, &new_nip05_value, true) {
                        state.verification_error = Some(format!("Failed to save: {}", e));
                    } else {
                        app.nip05_verifier
                            .request(new_nip05_value, pk_hex.to_string());
                        state.new_nip05.clear();
                        state.verification_error = None;
                    }
                }
            }
        });
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
}
