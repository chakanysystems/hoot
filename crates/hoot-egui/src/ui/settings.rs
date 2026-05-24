use crate::{
    profile_metadata::{ProfileMetadata, ProfileOption},
    Hoot,
};
use eframe::egui::{Color32, Sense, Ui, Vec2};
use hoot_backend::{AccountSummary, RelayConnectionStatus};
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
        if app.accounts.is_empty() {
            ui.small("No accounts added yet.");
            return;
        }

        for account in app.accounts.clone() {
            Self::profile_account(app, ui, account);
            ui.add_space(8.0);
        }
    }

    fn profile_account(app: &mut Hoot, ui: &mut Ui, account: AccountSummary) {
        let pk_hex = account.pubkey_hex.clone();
        app.state
            .settings
            .metadata_state
            .entry(pk_hex.clone())
            .or_default();

        ui.horizontal(|ui| {
            let active_marker = if account.is_active { " (active)" } else { "" };
            ui.label(format!("Key ID: {}{}", account.npub, active_marker));
            if !account.is_active && ui.button("Make Active").clicked() {
                if let Err(e) = app.backend.set_active_account(Some(pk_hex.clone())) {
                    error!("Could not set active account: {}", e);
                }
                app.refresh_accounts();
            }
            if ui.button("Remove Key").clicked() {
                if let Err(e) = app.backend.delete_account(pk_hex.clone()) {
                    error!("Could not remove key: {}", e);
                }
                app.refresh_accounts();
            }
        });

        let profile_metadata = app
            .profile_metadata
            .get(&pk_hex)
            .cloned()
            .unwrap_or_else(|| match app.backend.get_profile_metadata(pk_hex.clone()) {
                Ok(Some(meta)) => ProfileOption::Some(meta),
                Ok(None) => ProfileOption::Waiting,
                Err(e) => {
                    error!("Could not fetch profile metadata: {}", e);
                    ProfileOption::Waiting
                }
            });

        if !app.profile_metadata.contains_key(&pk_hex) {
            app.profile_metadata
                .insert(pk_hex.clone(), profile_metadata.clone());
        }

        ui.horizontal(|ui| {
            let key_meta_state = app
                .state
                .settings
                .metadata_state
                .get(&pk_hex)
                .expect("metadata state should exist");

            let mut save_clicked = false;
            let mut cancel_clicked = false;
            let mut edit_clicked = false;
            let mut new_name_to_save: Option<String> = None;

            {
                let mut meta_state = key_meta_state.borrow_mut();
                let is_editing = meta_state.editing;
                let display_name = match &profile_metadata {
                    ProfileOption::Some(meta) => {
                        meta.display_name.as_deref().or(meta.name.as_deref())
                    }
                    ProfileOption::Waiting => None,
                };

                if is_editing {
                    ui.label("Display Name:");
                    ui.text_edit_singleline(&mut meta_state.display_name);
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                    if ui.button("Save").clicked() {
                        save_clicked = true;
                        new_name_to_save = Some(meta_state.display_name.clone());
                    }
                } else {
                    ui.label(format!(
                        "Display Name: {}",
                        display_name.unwrap_or("Not Found")
                    ));
                    if ui.button("Edit").clicked() {
                        edit_clicked = true;
                    }
                }

                if edit_clicked {
                    meta_state.display_name = display_name.unwrap_or_default().to_string();
                    meta_state.editing = true;
                }
                if cancel_clicked || save_clicked {
                    meta_state.editing = false;
                }
            }

            if let Some(new_name) = new_name_to_save {
                let mut new_meta = match &profile_metadata {
                    ProfileOption::Some(meta) => meta.clone(),
                    ProfileOption::Waiting => ProfileMetadata::default(),
                };
                new_meta.display_name = if new_name.trim().is_empty() {
                    None
                } else {
                    Some(new_name)
                };
                if let Err(e) = app
                    .backend
                    .update_profile_metadata(pk_hex.clone(), new_meta.clone())
                {
                    error!("Could not update profile metadata: {}", e);
                } else {
                    app.profile_metadata
                        .insert(pk_hex.clone(), ProfileOption::Some(new_meta));
                }
            }
        });

        ui.add_space(4.0);
        Self::nip05_management(app, ui, &pk_hex);
    }

    fn nip05_management(app: &mut Hoot, ui: &mut Ui, pk_hex: &str) {
        app.state
            .settings
            .nip05_state
            .entry(pk_hex.to_string())
            .or_default();

        let nip05s = match app.backend.get_nip05s_for_pubkey(pk_hex.to_string()) {
            Ok(entries) => entries,
            Err(e) => {
                error!("Failed to get NIP-05s for key: {}", e);
                Vec::new()
            }
        };

        ui.label("NIP-05 Identifiers:");
        for entry in &nip05s {
            ui.horizontal(|ui| {
                let (icon, color, _tooltip) = crate::ui::nip05_status::status_display(entry);
                ui.colored_label(color, icon);
                ui.label(&entry.nip05);
                if entry.is_own {
                    ui.small("(own)");
                }
                if ui.button("Remove").clicked() {
                    if let Err(e) = app
                        .backend
                        .delete_nip05(pk_hex.to_string(), entry.nip05.clone())
                    {
                        error!("Failed to delete NIP-05: {}", e);
                    }
                }
                if ui.button("Verify").clicked() {
                    if let Err(e) =
                        app.backend
                            .add_nip05(pk_hex.to_string(), entry.nip05.clone(), entry.is_own)
                    {
                        error!("Failed to queue NIP-05 verification: {}", e);
                    }
                }
            });
        }
        if nip05s.is_empty() {
            ui.small("No NIP-05 identifiers added yet.");
        }

        ui.horizontal(|ui| {
            let nip05_state = app
                .state
                .settings
                .nip05_state
                .get(pk_hex)
                .expect("NIP-05 state should exist");
            let mut add_clicked = false;
            let new_nip05_value;
            {
                let mut state = nip05_state.borrow_mut();
                ui.text_edit_singleline(&mut state.new_nip05);
                new_nip05_value = state.new_nip05.trim().to_string();
                if ui.button("Verify & Add").clicked() {
                    add_clicked = true;
                }
                if let Some(error) = &state.verification_error {
                    ui.colored_label(Color32::RED, error);
                }
            }
            if add_clicked && !new_nip05_value.is_empty() {
                let mut state = nip05_state.borrow_mut();
                if !looks_like_nip05(&new_nip05_value) {
                    state.verification_error =
                        Some("Invalid NIP-05 format. Use: user@domain.com".to_string());
                } else if let Err(e) =
                    app.backend
                        .add_nip05(pk_hex.to_string(), new_nip05_value.clone(), true)
                {
                    state.verification_error = Some(format!("Failed to save: {}", e));
                } else {
                    state.new_nip05.clear();
                    state.verification_error = None;
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
                if let Err(e) = app.backend.add_relay(new_relay.clone()) {
                    error!("Failed to add relay: {}", e);
                }
                app.state.settings.new_relay_url.clear();
            }
        });

        ui.add_space(10.0);
        ui.label("Your Relays:");
        ui.vertical(|ui| {
            let mut relay_to_remove: Option<String> = None;
            match app.backend.relay_statuses() {
                Ok(relays) => {
                    for relay in relays {
                        ui.horizontal(|ui| {
                            let conn_fill = relay_status_color(&relay.status);
                            let size = Vec2::splat(12.0);
                            let (response, painter) = ui.allocate_painter(size, Sense::hover());
                            let rect = response.rect;
                            painter.circle_filled(
                                rect.center(),
                                rect.width() / 2.0 - 1.0,
                                conn_fill,
                            );
                            ui.label(relay.url.clone());
                            ui.small(format!("{:?}", relay.status));
                            if ui.button("Remove Relay").clicked() {
                                relay_to_remove = Some(relay.url);
                            }
                        });
                    }
                }
                Err(e) => {
                    error!("Failed to list relays: {}", e);
                    ui.colored_label(Color32::RED, "Failed to load relays.");
                }
            }
            if let Some(url) = relay_to_remove {
                if let Err(e) = app.backend.remove_relay(url) {
                    error!("Failed to remove relay: {}", e);
                }
            }
        });
    }
}

fn looks_like_nip05(value: &str) -> bool {
    let mut parts = value.split('@');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), Some(domain), None) => !local.is_empty() && domain.contains('.'),
        _ => false,
    }
}

fn relay_status_color(status: &RelayConnectionStatus) -> Color32 {
    match status {
        RelayConnectionStatus::Connecting => Color32::YELLOW,
        RelayConnectionStatus::Connected => Color32::LIGHT_GREEN,
        RelayConnectionStatus::Disconnected => Color32::RED,
    }
}
