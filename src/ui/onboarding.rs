use crate::profile_metadata::{
    get_profile_metadata, update_logged_in_profile_metadata, ProfileMetadata, ProfileOption,
};
use crate::{Hoot, Page};
use eframe::egui;
use nostr::key::Keys;
use nostr::{PublicKey, ToBech32};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq)]
pub enum AccountCreationMode {
    Generate,
    Import,
}

pub struct OnboardingState {
    pub secret_input: String,
    pub secret_input_2: String,
    pub mode: Option<AccountCreationMode>,
    pub generated_keys: Option<Keys>,
    pub imported_key: Option<Keys>,
    pub nsec_input: String,
    pub display_name: String,
    pub name: String,
    pub picture_url: String,
    pub metadata_fetched: bool,
    pub publish_metadata: bool,
    pub error_string: String,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            secret_input: String::new(),
            secret_input_2: String::new(),
            mode: None,
            generated_keys: None,
            imported_key: None,
            nsec_input: String::new(),
            display_name: String::new(),
            name: String::new(),
            picture_url: String::new(),
            metadata_fetched: false,
            publish_metadata: true,
            error_string: String::new(),
        }
    }
}

impl OnboardingState {
    fn active_keys(&self) -> Option<&Keys> {
        self.generated_keys.as_ref().or(self.imported_key.as_ref())
    }
}

pub struct OnboardingScreen;

impl OnboardingScreen {
    pub fn ui(app: &mut Hoot, ui: &mut egui::Ui) {
        egui::Frame::none()
            .inner_margin(egui::Margin::same(20.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(30.0);
                    match app.page {
                        Page::Onboarding => Self::onboarding_home(app, ui),
                        Page::OnboardingNewUser => Self::onboarding_new_user_flow(app, ui),
                        Page::OnboardingReturning => Self::onboarding_returning(app, ui),
                        _ => error!("OnboardingScreen rendered on wrong page"),
                    }
                });
            });
    }

    // ── Shared UI helpers ───────────────────────────────────────────────

    fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.add_space(20.0);
        ui.heading(egui::RichText::new(title).size(20.0));
        ui.add_space(10.0);
        ui.label(egui::RichText::new(subtitle).color(ui.visuals().weak_text_color()));
        ui.add_space(20.0);
    }

    fn show_error(ui: &mut egui::Ui, error: &str) {
        if !error.is_empty() {
            ui.colored_label(egui::Color32::RED, format!("⚠ {}", error));
            ui.add_space(10.0);
        }
    }

    fn password_field(ui: &mut egui::Ui, value: &mut String, hint: &str) {
        ui.add_sized(
            [300.0, 24.0],
            egui::TextEdit::singleline(value)
                .password(true)
                .hint_text(hint),
        );
    }

    fn format_unlock_error(e: &anyhow::Error) -> String {
        crate::db::format_unlock_error(e)
    }

    // ── Page: Welcome ───────────────────────────────────────────────────

    fn onboarding_home(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.add_space(50.0);
        ui.heading(egui::RichText::new("Welcome to Hoot Mail!").size(24.0));
        ui.add_space(20.0);
        ui.label(
            egui::RichText::new("A privacy-focused email client powered by Nostr")
                .size(14.0)
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(40.0);

        let w = 280.0;
        if ui
            .add_sized(
                [w, 45.0],
                egui::Button::new(egui::RichText::new("Get Started").size(15.0)),
            )
            .clicked()
        {
            app.page = Page::OnboardingNewUser;
            app.state.onboarding = OnboardingState::default();
        }
        ui.add_space(10.0);
        if ui
            .add_sized(
                [w, 45.0],
                egui::Button::new(egui::RichText::new("I have an existing account").size(15.0)),
            )
            .clicked()
        {
            app.page = Page::OnboardingReturning;
            app.state.onboarding = OnboardingState::default();
        }
    }

    // ── Page: New user flow (multi-step) ────────────────────────────────

    fn onboarding_new_user_flow(app: &mut Hoot, ui: &mut egui::Ui) {
        if !app.db.is_initialized() {
            if Self::db_file_has_password() {
                Self::onboarding_unlock_database(app, ui);
            } else {
                Self::onboarding_setup_database(app, ui);
            }
            return;
        }

        if app.state.onboarding.mode.is_none() {
            Self::render_mode_selection(app, ui);
            return;
        }

        if app.state.onboarding.mode == Some(AccountCreationMode::Import)
            && app.state.onboarding.imported_key.is_none()
        {
            Self::render_import_step(app, ui);
            return;
        }

        if app.state.onboarding.mode == Some(AccountCreationMode::Generate)
            && app.state.onboarding.generated_keys.is_none()
        {
            app.state.onboarding.generated_keys = Some(Keys::generate());
        }

        Self::render_metadata_step(app, ui);
    }

    // ── Step: Set new database password ─────────────────────────────────

    fn onboarding_setup_database(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Secure Your Data",
            "Set a password to encrypt your local database",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        let strength = check_password_strength(&app.state.onboarding.secret_input);
        let (color, label) = match strength {
            PasswordStrength::Weak => (egui::Color32::RED, "Weak"),
            PasswordStrength::Fair => (egui::Color32::YELLOW, "Fair"),
            PasswordStrength::Strong => (egui::Color32::LIGHT_GREEN, "Strong"),
        };

        ui.label("Password:");
        Self::password_field(ui, &mut app.state.onboarding.secret_input, "Enter password");
        if !app.state.onboarding.secret_input.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Strength:");
                ui.colored_label(color, label);
            });
        }
        ui.add_space(10.0);

        let passwords_match =
            app.state.onboarding.secret_input == app.state.onboarding.secret_input_2;

        ui.label("Confirm Password:");
        Self::password_field(
            ui,
            &mut app.state.onboarding.secret_input_2,
            "Re-enter password",
        );
        if !passwords_match && !app.state.onboarding.secret_input_2.is_empty() {
            ui.colored_label(egui::Color32::RED, "Passwords do not match");
        }
        ui.add_space(20.0);

        let can_continue = passwords_match && app.state.onboarding.secret_input.len() >= 4;
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                app.page = Page::Onboarding;
                app.state.onboarding.error_string.clear();
                return;
            }
            if ui
                .add_enabled(can_continue, egui::Button::new("Continue →"))
                .clicked()
            {
                match app
                    .db
                    .unlock_with_password(app.state.onboarding.secret_input.clone())
                {
                    Ok(_) => {
                        app.state.onboarding.secret_input.clear();
                        app.state.onboarding.secret_input_2.clear();
                        app.state.onboarding.error_string.clear();
                    }
                    Err(e) => {
                        app.state.onboarding.error_string =
                            format!("Failed to set password: {}", e);
                        error!("Failed to unlock database: {}", e);
                    }
                }
            }
        });
    }

    // ── Step: Unlock existing database ──────────────────────────────────

    fn onboarding_unlock_database(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Unlock Database",
            "Enter your database password to continue setup",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        ui.label("Password:");
        Self::password_field(ui, &mut app.state.onboarding.secret_input, "Enter password");
        ui.add_space(20.0);

        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                app.page = Page::Onboarding;
                app.state.onboarding.secret_input.clear();
                app.state.onboarding.error_string.clear();
            }
            if ui.button("Unlock").clicked() {
                match app
                    .db
                    .unlock_with_password(app.state.onboarding.secret_input.clone())
                {
                    Ok(_) => {
                        app.state.onboarding.secret_input.clear();
                        app.state.onboarding.error_string.clear();
                    }
                    Err(e) => {
                        app.state.onboarding.secret_input.clear();
                        app.state.onboarding.error_string = Self::format_unlock_error(&e);
                        error!("Failed to unlock database: {}", e);
                    }
                }
            }
        });
    }

    // ── Step: Choose generate or import ─────────────────────────────────

    fn render_mode_selection(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Create Your Account",
            "Choose how you'd like to set up your Nostr identity",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        let card_button_width = 290.0;

        Self::option_card(
            ui,
            "Generate New Identity",
            "Create a fresh keypair for Hoot Mail",
            "Generate New Keypair",
            card_button_width,
            || {
                app.state.onboarding.mode = Some(AccountCreationMode::Generate);
                app.state.onboarding.error_string.clear();
            },
        );

        ui.add_space(15.0);

        Self::option_card(
            ui,
            "Import Existing Key",
            "Use your existing Nostr private key (nsec)",
            "Import Private Key",
            card_button_width,
            || {
                app.state.onboarding.mode = Some(AccountCreationMode::Import);
                app.state.onboarding.error_string.clear();
            },
        );

        ui.add_space(30.0);

        if ui.button("← Back").clicked() {
            app.page = Page::Onboarding;
            app.state.onboarding.mode = None;
            app.state.onboarding.error_string.clear();
        }
    }

    fn option_card(
        ui: &mut egui::Ui,
        title: &str,
        description: &str,
        button_label: &str,
        button_width: f32,
        mut on_click: impl FnMut(),
    ) {
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .inner_margin(egui::Margin::same(15.0))
            .rounding(egui::Rounding::same(8.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).size(16.0).strong());
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new(description)
                            .size(12.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add_space(10.0);
                    if ui
                        .add_sized(
                            [button_width, 35.0],
                            egui::Button::new(egui::RichText::new(button_label).size(14.0)),
                        )
                        .clicked()
                    {
                        on_click();
                    }
                });
            });
    }

    // ── Step: Import nsec key ───────────────────────────────────────────

    fn render_import_step(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Import Private Key",
            "Enter your Nostr private key (nsec) to import your identity",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        ui.label("Private Key (nsec):");
        ui.add_space(5.0);
        ui.add(
            egui::TextEdit::singleline(&mut app.state.onboarding.nsec_input)
                .hint_text("nsec1...")
                .password(true)
                .desired_width(400.0),
        );
        ui.add_space(5.0);

        let validation = Self::validate_nsec(&app.state.onboarding.nsec_input);
        match &validation {
            Ok(_) => {
                ui.colored_label(egui::Color32::GREEN, "Valid nsec format");
            }
            Err(e) if !app.state.onboarding.nsec_input.is_empty() => {
                ui.colored_label(egui::Color32::RED, e.as_str());
            }
            _ => {}
        }
        ui.add_space(20.0);

        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                app.state.onboarding.mode = None;
                app.state.onboarding.error_string.clear();
                app.state.onboarding.nsec_input.clear();
            }
            if ui
                .add_enabled(validation.is_ok(), egui::Button::new("Continue →"))
                .clicked()
            {
                Self::handle_import(app, validation.unwrap());
            }
        });
    }

    fn handle_import(app: &mut Hoot, keys: Keys) {
        let already_exists = app
            .account_manager
            .loaded_keys
            .iter()
            .any(|k| k.public_key() == keys.public_key());

        if already_exists {
            app.state.onboarding.error_string = "This account is already added".to_string();
            return;
        }

        let pubkey_str = keys.public_key().to_string();
        app.state.onboarding.imported_key = Some(keys);
        app.state.onboarding.error_string.clear();

        match get_profile_metadata(app, pubkey_str).clone() {
            ProfileOption::Some(meta) => {
                app.state.onboarding.display_name = meta.display_name.clone().unwrap_or_default();
                app.state.onboarding.name = meta.name.clone().unwrap_or_default();
                app.state.onboarding.picture_url = meta.picture.clone().unwrap_or_default();
                app.state.onboarding.metadata_fetched = true;
                debug!("Pre-filled metadata for imported key");
            }
            ProfileOption::Waiting => {
                debug!("Metadata requested from relays, will populate when received");
                app.state.onboarding.metadata_fetched = false;
            }
        }
    }

    // ── Step: Configure profile metadata ────────────────────────────────

    fn render_metadata_step(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(ui, "Set Up Your Profile", "");
        Self::show_error(ui, &app.state.onboarding.error_string);

        let pubkey = match app.state.onboarding.active_keys() {
            Some(k) => k.public_key(),
            None => {
                ui.label("Error: No key found");
                return;
            }
        };

        if app.state.onboarding.metadata_fetched {
            ui.colored_label(
                egui::Color32::LIGHT_BLUE,
                "Profile information loaded from relays",
            );
            ui.add_space(10.0);
        } else {
            ui.label(
                egui::RichText::new("Customize how you appear to others (optional)")
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(10.0);
        }

        ui.collapsing("Your Public Key", |ui| {
            ui.horizontal(|ui| {
                ui.style_mut().override_font_id = Some(egui::FontId::monospace(11.0));
                ui.label(pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_string()));
            });
        });
        ui.add_space(15.0);

        ui.label("Display Name:");
        ui.add_sized(
            [350.0, 24.0],
            egui::TextEdit::singleline(&mut app.state.onboarding.display_name)
                .hint_text("Your friendly name"),
        );
        ui.add_space(10.0);

        ui.label("Username:");
        ui.add_sized(
            [350.0, 24.0],
            egui::TextEdit::singleline(&mut app.state.onboarding.name).hint_text("@username"),
        );
        ui.add_space(10.0);

        ui.collapsing("Advanced", |ui| {
            ui.label("Picture URL:");
            ui.add_sized(
                [350.0, 24.0],
                egui::TextEdit::singleline(&mut app.state.onboarding.picture_url)
                    .hint_text("https://..."),
            );
        });
        ui.add_space(15.0);

        ui.checkbox(
            &mut app.state.onboarding.publish_metadata,
            "Publish profile to Nostr relays",
        );
        ui.add_space(20.0);

        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                match app.state.onboarding.mode {
                    Some(AccountCreationMode::Generate) => {
                        app.state.onboarding.mode = None;
                        app.state.onboarding.generated_keys = None;
                    }
                    Some(AccountCreationMode::Import) => {
                        app.state.onboarding.imported_key = None;
                    }
                    None => {
                        app.page = Page::Onboarding;
                    }
                }
                app.state.onboarding.error_string.clear();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(egui::RichText::new("Finish →").strong()))
                    .clicked()
                {
                    if Self::save_account(app) {
                        app.page = Page::Inbox;
                        Self::finish_onboarding(app);
                    }
                }
                if ui.button("Skip Profile").clicked() {
                    app.state.onboarding.publish_metadata = false;
                    if Self::save_account(app) {
                        app.page = Page::Inbox;
                        Self::finish_onboarding(app);
                    }
                }
            });
        });
    }

    // ── Page: Returning user (import + go) ──────────────────────────────

    fn onboarding_returning(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Welcome Back!",
            "Enter your private key to access your account",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        ui.label("Private Key (nsec):");
        ui.add_space(5.0);
        ui.add(
            egui::TextEdit::singleline(&mut app.state.onboarding.secret_input)
                .hint_text("nsec1...")
                .password(true)
                .desired_width(400.0),
        );
        ui.add_space(5.0);

        let parsed = nostr::SecretKey::parse(&app.state.onboarding.secret_input);
        let valid = parsed.is_ok();
        if !app.state.onboarding.secret_input.is_empty() {
            if valid {
                ui.colored_label(egui::Color32::GREEN, "Valid nsec format");
            } else {
                ui.colored_label(egui::Color32::RED, "Invalid nsec format");
            }
        }
        ui.add_space(20.0);

        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                app.page = Page::Onboarding;
                app.state.onboarding.secret_input.clear();
                app.state.onboarding.error_string.clear();
            }
            if ui
                .add_enabled(valid, egui::Button::new("Continue →"))
                .clicked()
            {
                let keypair = nostr::Keys::new(parsed.unwrap());
                match app.account_manager.save_keys(&app.db, &keypair) {
                    Ok(()) => {
                        Self::update_gift_wrap_subscription(app);
                        app.active_account = Some(keypair);
                        app.page = Page::Inbox;
                        Self::finish_onboarding(app);
                    }
                    Err(e) => {
                        app.state.onboarding.error_string = format!("Failed to save key: {}", e);
                        error!("Failed to save key: {}", e);
                    }
                }
            }
        });
    }

    // ── Internal helpers ────────────────────────────────────────────────

    fn finish_onboarding(app: &mut Hoot) {
        let storage_dir = eframe::storage_dir(crate::STORAGE_NAME).unwrap();
        if let Err(e) = std::fs::write(storage_dir.join("done"), []) {
            error!("Failed to write done file: {}", e);
        }
        app.page = Page::Inbox;
    }

    fn db_file_has_password() -> bool {
        let storage_dir = eframe::storage_dir(crate::STORAGE_NAME).unwrap();
        let db_path = storage_dir.join("hoot.db");
        std::fs::metadata(&db_path)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    }

    fn validate_nsec(input: &str) -> Result<Keys, String> {
        crate::account_manager::validate_nsec(input)
    }

    fn save_account(app: &mut Hoot) -> bool {
        let key = match app.state.onboarding.active_keys() {
            Some(k) => k.clone(),
            None => {
                app.state.onboarding.error_string = "No key found".to_string();
                return false;
            }
        };

        if let Err(e) = app.account_manager.save_keys(&app.db, &key) {
            app.state.onboarding.error_string = format!("Failed to save key: {}", e);
            error!("Failed to save key: {}", e);
            return false;
        }

        app.active_account = Some(key.clone());

        if app.state.onboarding.publish_metadata {
            Self::publish_metadata(app, key.public_key());
        }

        Self::update_gift_wrap_subscription(app);
        info!("Account saved successfully");
        true
    }

    fn publish_metadata(app: &mut Hoot, pubkey: PublicKey) {
        let s = &app.state.onboarding;
        let metadata = ProfileMetadata {
            display_name: non_empty(&s.display_name),
            name: non_empty(&s.name),
            picture: non_empty(&s.picture_url),
        };

        if metadata.display_name.is_none() && metadata.name.is_none() && metadata.picture.is_none()
        {
            return;
        }

        match update_logged_in_profile_metadata(app, pubkey, metadata) {
            Ok(_) => info!("Metadata published successfully"),
            Err(e) => warn!("Failed to publish metadata (non-critical): {}", e),
        }
    }

    fn update_gift_wrap_subscription(app: &mut Hoot) {
        app.update_gift_wrap_subscription();
    }
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn check_password_strength(password: &str) -> PasswordStrength {
    let len = password.len();
    let has_upper = password.chars().any(|c| c.is_uppercase());
    let has_lower = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| !c.is_alphanumeric());

    let mut score = 0;
    if len >= 8 {
        score += 1;
    }
    if len >= 12 {
        score += 1;
    }
    if has_upper && has_lower {
        score += 1;
    }
    if has_digit {
        score += 1;
    }
    if has_special {
        score += 1;
    }

    match score {
        0..=2 => PasswordStrength::Weak,
        3 => PasswordStrength::Fair,
        _ => PasswordStrength::Strong,
    }
}

enum PasswordStrength {
    Weak,
    Fair,
    Strong,
}
