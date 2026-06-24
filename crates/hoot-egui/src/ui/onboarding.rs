use super::account_setup::{self, AccountCreationMode};
use crate::{style, Hoot, HootStatus, Page};
use eframe::egui::{self, Color32, CornerRadius, RichText, Vec2};
use tracing::error;

#[derive(Default)]
pub struct OnboardingState {
    pub secret_input: String,
    pub secret_input_2: String,
    pub mode: Option<AccountCreationMode>,
    pub pending_account: Option<hoot_backend::AccountSummary>,
    pub generated_account: Option<hoot_backend::AccountSummary>,
    pub generated_nsec: Option<String>,
    pub nsec_input: String,
    pub display_name: String,
    pub name: String,
    pub picture_url: String,
    pub metadata_fetched: bool,
    pub publish_metadata: bool,
    pub key_saved: bool,
    pub relays: Vec<String>,
    pub relay_input: String,
    pub error_string: String,
}

pub struct OnboardingScreen;

impl OnboardingScreen {
    pub fn ui(app: &mut Hoot, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        egui::Frame::new().fill(style::BG).show(ui, |ui| {
            ui.set_min_size(avail);
            ui.vertical_centered(|ui| {
                ui.set_max_width(420.0);
                ui.add_space(avail.y * 0.12);

                match app.page {
                    Page::Onboarding => render_welcome(app, ui),
                    Page::OnboardingNewUser => Self::render_new_user_steps(app, ui),
                    Page::OnboardingNewShowKey => render_show_key(app, ui),
                    Page::OnboardingReturning => Self::render_returning_steps(app, ui),
                    Page::OnboardingRelay => render_relays(app, ui),
                    Page::OnboardingReady => render_ready(app, ui),
                    _ => {}
                }

                ui.add_space(24.0);
                render_progress_dots(ui, &app.page);
            });
        });
    }

    fn render_new_user_steps(app: &mut Hoot, ui: &mut egui::Ui) {
        if !app.backend.is_database_initialized().unwrap_or(false) {
            if app.backend.db_file_has_password().unwrap_or(false) {
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
            && app.state.onboarding.pending_account.is_none()
        {
            Self::render_import_step(app, ui);
            return;
        }

        if app.state.onboarding.mode == Some(AccountCreationMode::Generate)
            && app.state.onboarding.generated_account.is_none()
        {
            match app.backend.generate_account_preview() {
                Ok(preview) => {
                    app.state.onboarding.generated_account = Some(preview.summary);
                    app.state.onboarding.generated_nsec = Some(preview.nsec);
                    app.page = Page::OnboardingNewShowKey;
                }
                Err(e) => {
                    app.state.onboarding.error_string = e.to_string();
                }
            }
            return;
        }

        if app.state.onboarding.mode == Some(AccountCreationMode::Generate) {
            app.page = Page::OnboardingNewShowKey;
            return;
        }

        Self::render_metadata_step(app, ui);
    }

    fn render_returning_steps(app: &mut Hoot, ui: &mut egui::Ui) {
        if !app.backend.is_database_initialized().unwrap_or(false) {
            if app.backend.db_file_has_password().unwrap_or(false) {
                Self::onboarding_unlock_database(app, ui);
            } else {
                Self::onboarding_setup_database(app, ui);
            }
            return;
        }
        Self::onboarding_returning(app, ui);
    }

    fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.label(RichText::new(title).size(22.0).strong().color(style::TEXT));
        ui.add_space(8.0);
        ui.label(RichText::new(subtitle).size(14.0).color(style::TEXT2));
        ui.add_space(28.0);
    }

    fn show_error(ui: &mut egui::Ui, error: &str) {
        if !error.is_empty() {
            ui.colored_label(egui::Color32::RED, format!("⚠ {}", error));
            ui.add_space(10.0);
        }
    }

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
            PasswordStrength::Fair => (style::AMBER, "Fair"),
            PasswordStrength::Strong => (style::GREEN, "Strong"),
        };

        ui.label(RichText::new("Password").size(13.0).color(style::TEXT3));
        style::boxed_password_text_edit(
            ui,
            ui.id().with("setup_password"),
            &mut app.state.onboarding.secret_input,
            "Enter password",
            13.5,
        );
        if !app.state.onboarding.secret_input.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Strength:").size(12.0).color(style::TEXT3));
                ui.colored_label(color, label);
            });
        }
        ui.add_space(12.0);

        let passwords_match =
            app.state.onboarding.secret_input == app.state.onboarding.secret_input_2;

        ui.label(
            RichText::new("Confirm Password")
                .size(13.0)
                .color(style::TEXT3),
        );
        style::boxed_password_text_edit(
            ui,
            ui.id().with("setup_password_confirm"),
            &mut app.state.onboarding.secret_input_2,
            "Re-enter password",
            13.5,
        );
        if !passwords_match && !app.state.onboarding.secret_input_2.is_empty() {
            ui.colored_label(egui::Color32::RED, "Passwords do not match");
        }
        ui.add_space(24.0);

        let can_continue = passwords_match && app.state.onboarding.secret_input.len() >= 4;
        let continue_btn =
            egui::Button::new(RichText::new("Continue").color(Color32::WHITE).size(14.0))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(240.0, 44.0));
        if ui.add_enabled(can_continue, continue_btn).clicked() {
            match app
                .backend
                .unlock_database(app.state.onboarding.secret_input.clone())
            {
                Ok(()) => {
                    app.state.onboarding.secret_input.clear();
                    app.state.onboarding.secret_input_2.clear();
                    app.state.onboarding.error_string.clear();
                }
                Err(e) => {
                    app.state.onboarding.error_string = format!("Failed to set password: {}", e);
                    error!("Failed to set database password: {}", e);
                }
            }
        }
        ui.add_space(12.0);
        if ui
            .add(
                egui::Button::new(RichText::new("← Back").color(style::TEXT).size(13.5))
                    .fill(style::SURFACE)
                    .stroke(egui::Stroke::new(1.0, style::border_strong()))
                    .corner_radius(CornerRadius::same(10))
                    .min_size(Vec2::new(240.0, 40.0)),
            )
            .clicked()
        {
            app.page = Page::Onboarding;
            app.state.onboarding.error_string.clear();
        }
    }

    fn onboarding_unlock_database(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Unlock Database",
            "Enter your database password to continue setup",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        ui.label(RichText::new("Password").size(13.0).color(style::TEXT3));
        style::boxed_password_text_edit(
            ui,
            ui.id().with("unlock_password"),
            &mut app.state.onboarding.secret_input,
            "Enter password",
            13.5,
        );
        ui.add_space(24.0);

        let unlock_btn =
            egui::Button::new(RichText::new("Unlock").color(Color32::WHITE).size(14.0))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(240.0, 44.0));
        if ui.add(unlock_btn).clicked() {
            match app
                .backend
                .unlock_database(app.state.onboarding.secret_input.clone())
            {
                Ok(()) => {
                    app.state.onboarding.secret_input.clear();
                    app.state.onboarding.error_string.clear();
                }
                Err(e) => {
                    app.state.onboarding.error_string = e.to_string();
                    error!("Failed to unlock database: {}", e);
                }
            }
        }
        ui.add_space(12.0);
        if ui
            .add(
                egui::Button::new(RichText::new("← Back").color(style::TEXT).size(13.5))
                    .fill(style::SURFACE)
                    .stroke(egui::Stroke::new(1.0, style::border_strong()))
                    .corner_radius(CornerRadius::same(10))
                    .min_size(Vec2::new(240.0, 40.0)),
            )
            .clicked()
        {
            app.page = Page::Onboarding;
            app.state.onboarding.secret_input.clear();
            app.state.onboarding.error_string.clear();
        }
    }

    fn render_mode_selection(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Create Your Account",
            "Choose how you'd like to set up your Nostr identity",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        let card_width = 320.0;
        Self::option_card(
            ui,
            "Generate New Identity",
            "Create a fresh keypair for Hoot",
            "Generate New Keypair",
            card_width,
            || {
                app.state.onboarding.mode = Some(AccountCreationMode::Generate);
                app.state.onboarding.error_string.clear();
            },
        );
        ui.add_space(16.0);
        Self::option_card(
            ui,
            "Import Existing Key",
            "Use your existing Nostr private key (nsec)",
            "Import Private Key",
            card_width,
            || {
                app.state.onboarding.mode = Some(AccountCreationMode::Import);
                app.state.onboarding.error_string.clear();
            },
        );
        ui.add_space(24.0);
        if ui
            .add(
                egui::Button::new(RichText::new("← Back").color(style::TEXT).size(13.5))
                    .fill(style::SURFACE)
                    .stroke(egui::Stroke::new(1.0, style::border_strong()))
                    .corner_radius(CornerRadius::same(10))
                    .min_size(Vec2::new(120.0, 40.0)),
            )
            .clicked()
        {
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
        width: f32,
        mut on_click: impl FnMut(),
    ) {
        egui::Frame::new()
            .fill(style::SURFACE)
            .stroke(egui::Stroke::new(1.0, style::border()))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::same(20))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(16.0).strong().color(style::TEXT));
                    ui.add_space(5.0);
                    ui.label(RichText::new(description).size(12.0).color(style::TEXT2));
                    ui.add_space(12.0);
                    let btn = egui::Button::new(
                        RichText::new(button_label).color(Color32::WHITE).size(13.5),
                    )
                    .fill(style::ACCENT)
                    .corner_radius(CornerRadius::same(8))
                    .min_size(Vec2::new(width - 40.0, 36.0));
                    if ui.add(btn).clicked() {
                        on_click();
                    }
                });
            });
    }

    fn render_import_step(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Import Private Key",
            "Enter your Nostr private key (nsec) to import your identity",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        ui.label(
            RichText::new("Private Key (nsec):")
                .size(13.0)
                .color(style::TEXT3),
        );
        style::boxed_password_text_edit(
            ui,
            ui.id().with("import_nsec"),
            &mut app.state.onboarding.nsec_input,
            "nsec1...",
            13.5,
        );
        ui.add_space(8.0);

        let validation = account_setup::validate_nsec(app, &app.state.onboarding.nsec_input);
        match &validation {
            Ok(_) if !app.state.onboarding.nsec_input.is_empty() => {
                ui.colored_label(style::GREEN, "Valid nsec format");
            }
            Err(e) if !app.state.onboarding.nsec_input.is_empty() => {
                ui.colored_label(egui::Color32::RED, e.clone());
            }
            _ => {}
        }
        ui.add_space(24.0);

        let can_continue = validation.is_ok();
        let continue_btn =
            egui::Button::new(RichText::new("Continue").color(Color32::WHITE).size(14.0))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(120.0, 44.0));
        let back_btn = egui::Button::new(RichText::new("← Back").color(style::TEXT).size(13.5))
            .fill(style::SURFACE)
            .stroke(egui::Stroke::new(1.0, style::border_strong()))
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(120.0, 40.0));
        ui.horizontal(|ui| {
            if ui.add(back_btn).clicked() {
                app.state.onboarding.mode = None;
                app.state.onboarding.error_string.clear();
                app.state.onboarding.nsec_input.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(can_continue, continue_btn).clicked() {
                    let account = validation.unwrap();
                    app.state.onboarding.pending_account = Some(account.clone());
                    let (display_name, name, picture_url, fetched) =
                        account_setup::fetch_and_prefill_metadata(app, &account.pubkey_hex);
                    app.state.onboarding.display_name = display_name;
                    app.state.onboarding.name = name;
                    app.state.onboarding.picture_url = picture_url;
                    app.state.onboarding.metadata_fetched = fetched;
                    app.state.onboarding.error_string.clear();
                }
            });
        });
    }

    fn render_metadata_step(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Set Up Your Profile",
            "Customize how you appear to others",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        if app.state.onboarding.metadata_fetched {
            ui.colored_label(style::ACCENT, "Profile information loaded from relays");
            ui.add_space(10.0);
        }

        ui.label(RichText::new("Display Name").size(13.0).color(style::TEXT3));
        style::underline_text_edit(
            ui,
            &mut app.state.onboarding.display_name,
            "Your friendly name",
            13.5,
        );
        ui.add_space(12.0);

        ui.label(RichText::new("Username").size(13.0).color(style::TEXT3));
        style::underline_text_edit(ui, &mut app.state.onboarding.name, "@username", 13.5);
        ui.add_space(12.0);

        ui.collapsing("Advanced", |ui| {
            ui.label(RichText::new("Picture URL:").size(13.0).color(style::TEXT3));
            style::underline_text_edit(
                ui,
                &mut app.state.onboarding.picture_url,
                "https://...",
                13.5,
            );
        });
        ui.add_space(16.0);

        ui.checkbox(
            &mut app.state.onboarding.publish_metadata,
            "Publish profile to Nostr relays",
        );
        ui.add_space(24.0);

        let save_btn =
            egui::Button::new(RichText::new("Continue").color(Color32::WHITE).size(14.0))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(120.0, 44.0));
        let skip_btn =
            egui::Button::new(RichText::new("Skip Profile").color(style::TEXT3).size(13.5))
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE);
        let back_btn = egui::Button::new(RichText::new("← Back").color(style::TEXT).size(13.5))
            .fill(style::SURFACE)
            .stroke(egui::Stroke::new(1.0, style::border_strong()))
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(120.0, 40.0));
        ui.horizontal(|ui| {
            if ui.add(back_btn).clicked() {
                match app.state.onboarding.mode {
                    Some(AccountCreationMode::Generate) => {
                        app.state.onboarding.mode = None;
                        app.state.onboarding.generated_account = None;
                        app.state.onboarding.generated_nsec = None;
                    }
                    Some(AccountCreationMode::Import) => {
                        app.state.onboarding.pending_account = None;
                    }
                    None => app.page = Page::Onboarding,
                }
                app.state.onboarding.error_string.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(skip_btn).clicked() {
                    app.state.onboarding.publish_metadata = false;
                    if Self::save_account(app) {
                        app.page = Page::OnboardingRelay;
                    }
                }
                if ui.add(save_btn).clicked() {
                    if Self::save_account(app) {
                        app.page = Page::OnboardingRelay;
                    }
                }
            });
        });
    }

    fn onboarding_returning(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::page_header(
            ui,
            "Welcome Back!",
            "Enter your private key to access your account",
        );
        Self::show_error(ui, &app.state.onboarding.error_string);

        ui.label(
            RichText::new("Private Key (nsec):")
                .size(13.0)
                .color(style::TEXT3),
        );
        style::boxed_password_text_edit(
            ui,
            ui.id().with("returning_nsec"),
            &mut app.state.onboarding.secret_input,
            "nsec1...",
            13.5,
        );
        ui.add_space(8.0);

        let validation = account_setup::validate_nsec(app, &app.state.onboarding.secret_input);
        let valid = validation.is_ok();
        if !app.state.onboarding.secret_input.is_empty() {
            if valid {
                ui.colored_label(style::GREEN, "Valid nsec format");
            } else {
                ui.colored_label(egui::Color32::RED, validation.as_ref().unwrap_err().clone());
            }
        }
        ui.add_space(24.0);

        let continue_btn =
            egui::Button::new(RichText::new("Continue").color(Color32::WHITE).size(14.0))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(120.0, 44.0));
        let back_btn = egui::Button::new(RichText::new("← Back").color(style::TEXT).size(13.5))
            .fill(style::SURFACE)
            .stroke(egui::Stroke::new(1.0, style::border_strong()))
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(120.0, 40.0));
        ui.horizontal(|ui| {
            if ui.add(back_btn).clicked() {
                app.page = Page::Onboarding;
                app.state.onboarding.secret_input.clear();
                app.state.onboarding.error_string.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(valid, continue_btn).clicked() {
                    let account = validation.unwrap();
                    if account_setup::account_already_exists(app, &account.pubkey_hex) {
                        app.state.onboarding.error_string =
                            "This account is already added".to_string();
                    } else {
                        match app
                            .backend
                            .import_account(app.state.onboarding.secret_input.clone())
                        {
                            Ok(_) => {
                                app.status = HootStatus::Initializing;
                                app.page = Page::Inbox;
                                Self::finish_onboarding(app);
                            }
                            Err(e) => {
                                error!("{}", e);
                                app.state.onboarding.error_string = e.to_string();
                            }
                        }
                    }
                }
            });
        });
    }

    fn finish_onboarding(app: &mut Hoot) {
        if let Err(e) = app.backend.mark_onboarding_complete() {
            error!("Failed to write done file: {}", e);
        }
        app.page = Page::Inbox;
    }

    fn save_account(app: &mut Hoot) -> bool {
        let result = match app.state.onboarding.mode.clone() {
            Some(AccountCreationMode::Generate) => {
                if let Some(nsec) = &app.state.onboarding.generated_nsec {
                    app.backend
                        .import_account(nsec.clone())
                        .map_err(|e| format!("Failed to save key: {}", e))
                } else {
                    Err("No generated key found".to_string())
                }
            }
            Some(AccountCreationMode::Import) => app
                .backend
                .import_account(app.state.onboarding.nsec_input.clone())
                .map_err(|e| format!("Failed to save key: {}", e)),
            None => Err("No account mode selected".to_string()),
        };

        match result {
            Ok(account) => {
                app.active_account_pubkey = Some(account.pubkey_hex.clone());
                app.refresh_accounts();
                if app.state.onboarding.publish_metadata {
                    let metadata = crate::profile_metadata::ProfileMetadata {
                        display_name: non_empty(&app.state.onboarding.display_name),
                        name: non_empty(&app.state.onboarding.name),
                        picture: non_empty(&app.state.onboarding.picture_url),
                        nip05: None,
                    };
                    if metadata.display_name.is_some()
                        || metadata.name.is_some()
                        || metadata.picture.is_some()
                    {
                        match crate::profile_metadata::update_logged_in_profile_metadata(
                            app,
                            account.pubkey_hex,
                            metadata,
                        ) {
                            Ok(_) => tracing::info!("Metadata published successfully"),
                            Err(e) => tracing::warn!("Failed to publish metadata: {}", e),
                        }
                    }
                }
                true
            }
            Err(e) => {
                error!("{}", e);
                app.state.onboarding.error_string = e;
                false
            }
        }
    }
}

fn render_welcome(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(RichText::new("Hoot").size(36.0).strong().color(style::TEXT));
    ui.add_space(8.0);
    ui.label(
        RichText::new("Email that belongs to you.")
            .size(16.0)
            .color(style::TEXT2),
    );
    ui.add_space(48.0);

    let btn_size = Vec2::new(240.0, 44.0);
    if ui
        .add(
            egui::Button::new(
                RichText::new("Create new identity")
                    .size(14.0)
                    .color(Color32::WHITE),
            )
            .fill(style::ACCENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(btn_size),
        )
        .clicked()
    {
        app.page = Page::OnboardingNewUser;
        app.state.onboarding = Default::default();
    }
    ui.add_space(12.0);
    if ui
        .add(
            egui::Button::new(RichText::new("I have a key").size(14.0).color(style::TEXT))
                .fill(style::SURFACE)
                .stroke(egui::Stroke::new(1.0, style::border_strong()))
                .corner_radius(CornerRadius::same(10))
                .min_size(btn_size),
        )
        .clicked()
    {
        app.page = Page::OnboardingReturning;
        app.state.onboarding = Default::default();
    }
}

fn render_show_key(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Your identity is ready.")
            .size(22.0)
            .strong()
            .color(style::TEXT),
    );
    ui.add_space(8.0);
    ui.label(
        RichText::new("Save your private key before continuing.")
            .size(14.0)
            .color(style::TEXT2),
    );
    ui.add_space(28.0);

    if let (Some(account), Some(nsec)) = (
        app.state.onboarding.generated_account.clone(),
        app.state.onboarding.generated_nsec.clone(),
    ) {
        let npub = account.npub;

        ui.label(
            RichText::new("Your address (share this freely)")
                .size(12.0)
                .strong()
                .color(style::TEXT3),
        );
        ui.add_space(4.0);
        let mut npub_clone = npub.clone();
        ui.add(
            egui::TextEdit::singleline(&mut npub_clone)
                .interactive(false)
                .desired_width(360.0)
                .font(egui::FontId::monospace(11.0)),
        );
        ui.add_space(20.0);

        egui::Frame::new()
            .fill(egui::Color32::from_rgb(254, 243, 199))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_max_width(360.0);
                ui.label(
                    RichText::new("⚠  Private key — never share this")
                        .size(12.0)
                        .strong()
                        .color(egui::Color32::from_rgb(146, 64, 14)),
                );
                ui.add_space(8.0);
                let mut nsec_clone = nsec.clone();
                ui.add(
                    egui::TextEdit::singleline(&mut nsec_clone)
                        .interactive(false)
                        .desired_width(328.0)
                        .font(egui::FontId::monospace(11.0)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "If you lose this key, your account cannot be recovered. Write it down somewhere safe.",
                    )
                    .size(12.0)
                    .color(egui::Color32::from_rgb(146, 64, 14)),
                );
            });

        ui.add_space(20.0);
        ui.checkbox(
            &mut app.state.onboarding.key_saved,
            "I've saved my private key somewhere safe",
        );
        ui.add_space(16.0);

        if ui
            .add_enabled(
                app.state.onboarding.key_saved,
                egui::Button::new(RichText::new("Continue").size(14.0).color(Color32::WHITE))
                    .fill(style::ACCENT)
                    .corner_radius(CornerRadius::same(10))
                    .min_size(Vec2::new(240.0, 44.0)),
            )
            .clicked()
            && OnboardingScreen::save_account(app)
        {
            app.page = Page::OnboardingRelay;
        }
    } else {
        if ui
            .add(
                egui::Button::new(RichText::new("← Back").color(style::TEXT).size(13.5))
                    .fill(style::SURFACE)
                    .stroke(egui::Stroke::new(1.0, style::border_strong()))
                    .corner_radius(CornerRadius::same(10))
                    .min_size(Vec2::new(120.0, 40.0)),
            )
            .clicked()
        {
            app.state.onboarding.mode = None;
            app.state.onboarding.generated_account = None;
            app.state.onboarding.generated_nsec = None;
            app.page = Page::OnboardingNewUser;
        }
    }
}

fn render_relays(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Connect to relays")
            .size(22.0)
            .strong()
            .color(style::TEXT),
    );
    ui.add_space(8.0);
    ui.label(
        RichText::new("Relays are servers that deliver your messages.")
            .size(14.0)
            .color(style::TEXT2),
    );
    ui.add_space(28.0);

    let mut to_remove: Option<usize> = None;
    for (i, relay) in app.state.onboarding.relays.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(relay).size(13.0).color(style::TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Remove").size(12.0).color(style::TEXT3))
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                    )
                    .clicked()
                {
                    to_remove = Some(i);
                }
            });
        });
        ui.add_space(4.0);
    }
    if let Some(i) = to_remove {
        app.state.onboarding.relays.remove(i);
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        style::underline_text_edit(
            ui,
            &mut app.state.onboarding.relay_input,
            "wss://relay.damus.io",
            13.5,
        );
        if ui
            .add(
                egui::Button::new(RichText::new("Add").color(style::ACCENT))
                    .fill(style::accent_soft())
                    .corner_radius(CornerRadius::same(6)),
            )
            .clicked()
        {
            let url = app.state.onboarding.relay_input.trim().to_string();
            if !url.is_empty() && (url.starts_with("wss://") || url.starts_with("ws://")) {
                app.state.onboarding.relays.push(url);
                app.state.onboarding.relay_input.clear();
            }
        }
    });

    if app.state.onboarding.relays.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new("Without a relay you won't receive messages.")
                .size(12.0)
                .color(style::AMBER),
        );
    }

    ui.add_space(24.0);
    if ui
        .add(
            egui::Button::new(RichText::new("Continue").size(14.0).color(Color32::WHITE))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(240.0, 44.0)),
        )
        .clicked()
    {
        for relay_url in &app.state.onboarding.relays {
            if let Err(e) = app.backend.add_relay(relay_url.clone()) {
                tracing::error!("Failed to add relay: {}", e);
            }
        }
        app.page = Page::OnboardingReady;
    }
    ui.add_space(8.0);
    if ui
        .add(
            egui::Button::new(RichText::new("Skip for now").size(13.0).color(style::TEXT3))
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE),
        )
        .clicked()
    {
        app.page = Page::OnboardingReady;
    }
}

fn render_ready(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("You're all set.")
            .size(28.0)
            .strong()
            .color(style::TEXT),
    );
    ui.add_space(12.0);
    ui.label(
        RichText::new("Your inbox is ready and waiting.")
            .size(16.0)
            .color(style::TEXT2),
    );
    ui.add_space(48.0);
    if ui
        .add(
            egui::Button::new(RichText::new("Open Hoot").size(15.0).color(Color32::WHITE))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(240.0, 48.0)),
        )
        .clicked()
    {
        app.page = Page::Inbox;
        OnboardingScreen::finish_onboarding(app);
    }
}

fn render_progress_dots(ui: &mut egui::Ui, page: &Page) {
    let steps = [
        Page::Onboarding,
        Page::OnboardingNewUser,
        Page::OnboardingNewShowKey,
        Page::OnboardingRelay,
        Page::OnboardingReady,
    ];
    let effective_page = if *page == Page::OnboardingReturning {
        &Page::OnboardingNewUser
    } else {
        page
    };
    let current_step = steps.iter().position(|p| p == effective_page);

    ui.horizontal(|ui| {
        let dot_count = steps.len() as f32;
        let total_width = dot_count * 8.0 + (dot_count - 1.0) * 6.0;
        ui.add_space((ui.available_width() - total_width) / 2.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        for (i, _) in steps.iter().enumerate() {
            let active = current_step == Some(i);
            let dot_size = Vec2::splat(if active { 8.0 } else { 6.0 });
            let (r, _) = ui.allocate_exact_size(dot_size, egui::Sense::hover());
            let color = if active {
                style::ACCENT
            } else {
                style::border_strong()
            };
            ui.painter()
                .circle_filled(r.center(), r.width() / 2.0, color);
        }
    });
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

#[derive(Debug, PartialEq)]
pub enum PasswordStrength {
    Weak,
    Fair,
    Strong,
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
