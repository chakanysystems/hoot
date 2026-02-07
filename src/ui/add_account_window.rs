use crate::profile_metadata::{
    get_profile_metadata, update_logged_in_profile_metadata, ProfileMetadata, ProfileOption,
};
use crate::relay::Subscription;
use eframe::egui::{self, RichText};
use nostr::{Keys, PublicKey, ToBech32};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq)]
pub enum AccountCreationMode {
    Generate,
    Import,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AccountCreationStep {
    ModeSelection,
    ImportKey,
    ConfigureMetadata,
    Review,
}

#[derive(Debug, Clone)]
pub struct AddAccountWindowState {
    pub mode: Option<AccountCreationMode>,
    pub step: AccountCreationStep,

    // Import fields
    pub nsec_input: String,
    pub imported_key: Option<Keys>,

    // Generated key
    pub generated_key: Option<Keys>,

    // Metadata fields
    pub display_name: String,
    pub name: String,
    pub picture_url: String,
    pub metadata_fetched: bool,

    // UI state
    pub error_message: Option<String>,
    pub publish_metadata: bool,
}

impl Default for AddAccountWindowState {
    fn default() -> Self {
        Self {
            mode: None,
            step: AccountCreationStep::ModeSelection,
            nsec_input: String::new(),
            imported_key: None,
            generated_key: None,
            display_name: String::new(),
            name: String::new(),
            picture_url: String::new(),
            metadata_fetched: false,
            error_message: None,
            publish_metadata: true,
        }
    }
}

pub struct AddAccountWindow {}

impl AddAccountWindow {
    /// Main rendering function - returns false if window should be closed
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) -> bool {
        let mut keep_open = true;
        let mut should_close_from_save = false;
        let screen_rect = ctx.screen_rect();
        let window_width = 500.0;
        let window_height = 400.0;

        egui::Window::new("Add Account")
            .id(id)
            .default_size([window_width, window_height])
            .min_width(450.0)
            .min_height(350.0)
            .default_pos([
                screen_rect.center().x - window_width / 2.0,
                screen_rect.center().y - window_height / 2.0,
            ])
            .open(&mut keep_open)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Get current step to determine what to render
                    let current_step = app
                        .state
                        .add_account_window
                        .get(&id)
                        .map(|s| s.step.clone())
                        .unwrap_or(AccountCreationStep::ModeSelection);

                    // Render step indicator
                    Self::render_step_indicator_for_step(ui, &current_step);
                    ui.add_space(10.0);

                    // Error message if present
                    if let Some(error) = app
                        .state
                        .add_account_window
                        .get(&id)
                        .and_then(|s| s.error_message.clone())
                    {
                        ui.colored_label(egui::Color32::RED, format!("⚠ {}", error));
                        ui.add_space(5.0);
                    }

                    // Main content area
                    match current_step {
                        AccountCreationStep::ModeSelection => {
                            Self::render_mode_selection(app, ui, id)
                        }
                        AccountCreationStep::ImportKey => Self::render_import_step(app, ui, id),
                        AccountCreationStep::ConfigureMetadata => {
                            Self::render_metadata_step(app, ui, id)
                        }
                        AccountCreationStep::Review => {
                            if Self::render_review_step(app, ui, id) {
                                should_close_from_save = true;
                            }
                        }
                    }
                });
            });

        keep_open && !should_close_from_save
    }

    fn render_step_indicator_for_step(ui: &mut egui::Ui, step: &AccountCreationStep) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Step:").strong());
            let step_text = match step {
                AccountCreationStep::ModeSelection => "1. Choose Method",
                AccountCreationStep::ImportKey => "2. Import Key",
                AccountCreationStep::ConfigureMetadata => "Configure Metadata",
                AccountCreationStep::Review => "Review",
            };
            ui.label(step_text);
        });
        ui.separator();
    }

    fn render_mode_selection(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label(RichText::new("How would you like to add an account?").size(16.0));
            ui.add_space(30.0);

            let button_size = [ui.available_width() * 0.8, 60.0];

            if ui
                .add_sized(
                    button_size,
                    egui::Button::new(RichText::new("Generate New Keypair").size(14.0)),
                )
                .clicked()
            {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.mode = Some(AccountCreationMode::Generate);
                state.error_message = None;

                // Generate the key (but don't save to account manager yet)
                let new_keypair = Keys::generate();
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.generated_key = Some(new_keypair);
                state.step = AccountCreationStep::ConfigureMetadata;
            }

            ui.add_space(15.0);

            if ui
                .add_sized(
                    button_size,
                    egui::Button::new(RichText::new("Import Existing Key").size(14.0)),
                )
                .clicked()
            {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.mode = Some(AccountCreationMode::Import);
                state.step = AccountCreationStep::ImportKey;
                state.error_message = None;
            }
        });
    }

    fn render_import_step(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        ui.add_space(10.0);
        ui.label("Enter your private key (nsec):");
        ui.add_space(5.0);

        // Clone data we need before entering closures
        let mut nsec_input = app
            .state
            .add_account_window
            .get(&id)
            .unwrap()
            .nsec_input
            .clone();

        // Text input for nsec
        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(&mut nsec_input)
                .hint_text("nsec1...")
                .password(true),
        );

        // Update the state with the new input
        app.state
            .add_account_window
            .get_mut(&id)
            .unwrap()
            .nsec_input = nsec_input.clone();

        // Validation indicator
        let validation_result = Self::validate_nsec(&nsec_input);
        ui.horizontal(|ui| match &validation_result {
            Ok(_) => {
                ui.colored_label(egui::Color32::GREEN, "✓ Valid nsec format");
            }
            Err(e) if !nsec_input.is_empty() => {
                ui.colored_label(egui::Color32::RED, format!("⊗ {}", e));
            }
            _ => {}
        });

        ui.add_space(10.0);

        // Navigation buttons
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next_enabled = validation_result.is_ok();

            if ui
                .add_enabled(next_enabled, egui::Button::new("Next"))
                .clicked()
            {
                if let Ok(keys) = validation_result {
                    // Check if key already exists
                    if app
                        .account_manager
                        .loaded_keys
                        .iter()
                        .any(|k| k.public_key() == keys.public_key())
                    {
                        app.state
                            .add_account_window
                            .get_mut(&id)
                            .unwrap()
                            .error_message = Some("This account is already added".to_string());
                    } else {
                        let pubkey_str = keys.public_key().to_string();

                        // Update state with imported key
                        let state = app.state.add_account_window.get_mut(&id).unwrap();
                        state.imported_key = Some(keys.clone());
                        state.error_message = None;

                        // Attempt to fetch existing metadata
                        let metadata_option = get_profile_metadata(app, pubkey_str.clone()).clone();
                        match metadata_option {
                            ProfileOption::Some(meta) => {
                                let state = app.state.add_account_window.get_mut(&id).unwrap();
                                state.display_name = meta.display_name.clone().unwrap_or_default();
                                state.name = meta.name.clone().unwrap_or_default();
                                state.picture_url = meta.picture.clone().unwrap_or_default();
                                state.metadata_fetched = true;
                                debug!("Pre-filled metadata for imported key");
                            }
                            ProfileOption::Waiting => {
                                debug!(
                                    "Metadata requested from relays, will populate when received"
                                );
                                let state = app.state.add_account_window.get_mut(&id).unwrap();
                                state.metadata_fetched = false;
                            }
                        }

                        app.state.add_account_window.get_mut(&id).unwrap().step =
                            AccountCreationStep::ConfigureMetadata;
                    }
                }
            }

            if ui.button("Back").clicked() {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.step = AccountCreationStep::ModeSelection;
                state.error_message = None;
                state.nsec_input.clear();
            }
        });
    }

    fn render_metadata_step(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        ui.add_space(10.0);

        // Show the public key
        let state = app.state.add_account_window.get(&id).unwrap();
        let pubkey = if let Some(key) = &state.generated_key {
            key.public_key()
        } else if let Some(key) = &state.imported_key {
            key.public_key()
        } else {
            ui.label("Error: No key found");
            return;
        };

        ui.label("Public Key:");
        ui.horizontal(|ui| {
            ui.style_mut().override_font_id = Some(egui::FontId::monospace(12.0));
            ui.label(pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_string()));
        });
        ui.add_space(10.0);

        // If metadata was fetched, show indicator
        let metadata_fetched = app
            .state
            .add_account_window
            .get(&id)
            .unwrap()
            .metadata_fetched;
        if metadata_fetched {
            ui.colored_label(egui::Color32::LIGHT_BLUE, "ℹ Metadata loaded from relays");
            ui.add_space(5.0);
        }

        // Clone fields for editing
        let mut display_name = app
            .state
            .add_account_window
            .get(&id)
            .unwrap()
            .display_name
            .clone();
        let mut name = app.state.add_account_window.get(&id).unwrap().name.clone();
        let mut picture_url = app
            .state
            .add_account_window
            .get(&id)
            .unwrap()
            .picture_url
            .clone();
        let mut publish_metadata = app
            .state
            .add_account_window
            .get(&id)
            .unwrap()
            .publish_metadata;

        // Metadata form
        ui.label("Display Name:");
        ui.text_edit_singleline(&mut display_name);
        ui.add_space(5.0);

        ui.label("Username:");
        ui.text_edit_singleline(&mut name);
        ui.add_space(5.0);

        ui.label("Picture URL:");
        ui.text_edit_singleline(&mut picture_url);
        ui.add_space(10.0);

        // Publish checkbox
        ui.checkbox(&mut publish_metadata, "Publish metadata to relays");
        ui.add_space(10.0);

        // Update state with edited values
        let state = app.state.add_account_window.get_mut(&id).unwrap();
        state.display_name = display_name;
        state.name = name;
        state.picture_url = picture_url;
        state.publish_metadata = publish_metadata;

        // Navigation buttons
        let mode = state.mode.clone();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Next").clicked() {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.step = AccountCreationStep::Review;
                state.error_message = None;
            }

            if ui.button("Skip Metadata").clicked() {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.step = AccountCreationStep::Review;
                state.publish_metadata = false;
                state.error_message = None;
            }

            if ui.button("Back").clicked() {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                match mode {
                    Some(AccountCreationMode::Generate) => {
                        state.step = AccountCreationStep::ModeSelection;
                        state.generated_key = None;
                    }
                    Some(AccountCreationMode::Import) => {
                        state.step = AccountCreationStep::ImportKey;
                    }
                    None => {
                        state.step = AccountCreationStep::ModeSelection;
                    }
                }
                state.error_message = None;
            }
        });
    }

    fn render_review_step(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) -> bool {
        ui.add_space(10.0);
        ui.label(RichText::new("Review Account Details").strong().size(14.0));
        ui.add_space(10.0);

        let state = app.state.add_account_window.get(&id).unwrap();
        let key = if let Some(k) = &state.generated_key {
            k.clone()
        } else if let Some(k) = &state.imported_key {
            k.clone()
        } else {
            ui.label("Error: No key found");
            return false;
        };

        // Show account type
        let account_type = match &state.mode {
            Some(AccountCreationMode::Generate) => "Generated New Key",
            Some(AccountCreationMode::Import) => "Imported Existing Key",
            None => "Unknown",
        };
        ui.label(format!("Type: {}", account_type));
        ui.add_space(5.0);

        // Show public key
        ui.label("Public Key:");
        ui.horizontal(|ui| {
            ui.style_mut().override_font_id = Some(egui::FontId::monospace(11.0));
            let npub = key
                .public_key()
                .to_bech32()
                .unwrap_or_else(|_| key.public_key().to_string());
            ui.label(&npub);
        });
        ui.add_space(10.0);

        // Show metadata summary
        let display_name = state.display_name.clone();
        let name = state.name.clone();
        let picture_url = state.picture_url.clone();
        let publish_metadata = state.publish_metadata;

        let has_metadata = !display_name.is_empty() || !name.is_empty() || !picture_url.is_empty();

        if has_metadata {
            ui.label(RichText::new("Metadata:").strong());
            if !display_name.is_empty() {
                ui.label(format!("  Display Name: {}", display_name));
            }
            if !name.is_empty() {
                ui.label(format!("  Username: {}", name));
            }
            if !picture_url.is_empty() {
                ui.label(format!("  Picture: {}", picture_url));
            }
            ui.add_space(5.0);

            if publish_metadata {
                ui.colored_label(
                    egui::Color32::LIGHT_GREEN,
                    "✓ Will publish metadata to relays",
                );
            } else {
                ui.label("Will not publish metadata");
            }
        } else {
            ui.label("No metadata configured");
        }

        ui.add_space(15.0);

        // Navigation buttons
        let mut should_close = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(RichText::new("Save Account").strong()).clicked() {
                // Clone state for saving
                let state_clone = app.state.add_account_window.get(&id).unwrap().clone();

                match Self::save_account(app, &state_clone, &key) {
                    Ok(_) => {
                        info!("Account saved successfully");
                        should_close = true;
                    }
                    Err(e) => {
                        error!("Failed to save account: {}", e);
                        app.state
                            .add_account_window
                            .get_mut(&id)
                            .unwrap()
                            .error_message = Some(e);
                    }
                }
            }

            if ui.button("Back").clicked() {
                app.state.add_account_window.get_mut(&id).unwrap().step =
                    AccountCreationStep::ConfigureMetadata;
                app.state
                    .add_account_window
                    .get_mut(&id)
                    .unwrap()
                    .error_message = None;
            }
        });

        should_close
    }

    fn validate_nsec(input: &str) -> Result<Keys, String> {
        if input.is_empty() {
            return Err("Please enter a private key".to_string());
        }

        use nostr::FromBech32;
        match nostr::SecretKey::from_bech32(input) {
            Ok(secret_key) => Ok(Keys::new(secret_key)),
            Err(_) => Err("Invalid nsec format".to_string()),
        }
    }

    fn save_account(
        app: &mut crate::Hoot,
        state: &AddAccountWindowState,
        key: &Keys,
    ) -> Result<(), String> {
        // Save the key to secure storage
        app.account_manager
            .save_keys(&app.db, &key)
            .map_err(|e| format!("Failed to save key: {}", e))?;

        // Set as active account
        app.active_account = Some(key.clone());

        // Publish metadata if requested
        if state.publish_metadata {
            let has_metadata = !state.display_name.is_empty()
                || !state.name.is_empty()
                || !state.picture_url.is_empty();

            if has_metadata {
                let metadata = ProfileMetadata {
                    display_name: if !state.display_name.is_empty() {
                        Some(state.display_name.clone())
                    } else {
                        None
                    },
                    name: if !state.name.is_empty() {
                        Some(state.name.clone())
                    } else {
                        None
                    },
                    picture: if !state.picture_url.is_empty() {
                        Some(state.picture_url.clone())
                    } else {
                        None
                    },
                };

                match update_logged_in_profile_metadata(app, key.public_key(), metadata) {
                    Ok(_) => {
                        info!("Metadata published successfully");
                    }
                    Err(e) => {
                        warn!("Failed to publish metadata (non-critical): {}", e);
                        // Continue - account is saved regardless
                    }
                }
            }
        }

        // Update relay subscriptions to include new account
        Self::update_gift_wrap_subscription(app);

        Ok(())
    }

    fn update_gift_wrap_subscription(app: &mut crate::Hoot) {
        if app.account_manager.loaded_keys.is_empty() {
            return;
        }

        let mut gw_sub = Subscription::default();
        let public_keys: Vec<PublicKey> = app
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

        gw_sub.filter(filter);

        match app.relays.add_subscription(gw_sub) {
            Ok(_) => debug!("Updated gift-wrap subscription with new account"),
            Err(e) => error!("Failed to update gift-wrap subscription: {}", e),
        }
    }
}
