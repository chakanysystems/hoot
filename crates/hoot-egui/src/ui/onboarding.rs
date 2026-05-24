use super::account_setup::AccountCreationMode;
use crate::{style, Hoot, HootStatus, Page};
use eframe::egui;
use hoot_backend::AccountSummary;
use tracing::error;

#[derive(Default)]
pub struct OnboardingState {
    pub secret_input: String,
    pub secret_input_2: String,
    pub mode: Option<AccountCreationMode>,
    pub pending_account: Option<AccountSummary>,
    pub nsec_input: String,
    pub display_name: String,
    pub name: String,
    pub picture_url: String,
    pub metadata_fetched: bool,
    pub publish_metadata: bool,
    pub error_string: String,
}

pub struct OnboardingScreen {}

impl OnboardingScreen {
    pub fn ui(app: &mut Hoot, ui: &mut egui::Ui) {
        egui::Frame::none()
            .fill(style::CARD_BG)
            .inner_margin(egui::Margin::same(32))
            .show(ui, |ui| match app.page {
                Page::Onboarding => Self::onboarding_home(app, ui),
                Page::OnboardingNewUser | Page::OnboardingNewShowKey => {
                    Self::onboarding_new_user_flow(app, ui)
                }
                Page::OnboardingReturning => Self::onboarding_returning(app, ui),
                _ => {}
            });
    }

    fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.vertical_centered(|ui| {
            ui.heading(title);
            ui.label(egui::RichText::new(subtitle).color(style::TEXT_MUTED));
        });
        ui.add_space(20.0);
    }

    fn show_error(ui: &mut egui::Ui, error: &str) {
        if !error.is_empty() {
            ui.colored_label(egui::Color32::RED, format!("⚠ {}", error));
            ui.add_space(8.0);
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

    fn onboarding_home(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            Self::page_header(ui, "Welcome to Hoot", "Nostr email");
            if ui.button("Get Started").clicked() {
                app.page = Page::OnboardingNewUser;
            }
            if ui.button("I already have a database").clicked() {
                app.page = Page::OnboardingReturning;
            }
        });
    }

    fn onboarding_new_user_flow(app: &mut Hoot, ui: &mut egui::Ui) {
        if !app.backend.is_database_initialized().unwrap_or(false)
            && app.backend.db_file_has_password().unwrap_or(false)
        {
            Self::onboarding_unlock_database(app, ui);
            return;
        }

        if app.state.onboarding.mode.is_none() {
            Self::render_mode_selection(app, ui);
        } else {
            Self::render_metadata_step(app, ui);
        }
    }

    fn onboarding_unlock_database(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            Self::page_header(
                ui,
                "Unlock Database",
                "Enter your password to continue setup",
            );
            Self::show_error(ui, &app.state.onboarding.error_string);
            Self::password_field(ui, &mut app.state.onboarding.secret_input, "Password");
            if ui.button("Unlock").clicked() {
                match app
                    .backend
                    .unlock_database(app.state.onboarding.secret_input.clone())
                {
                    Ok(()) => app.state.onboarding.error_string.clear(),
                    Err(e) => app.state.onboarding.error_string = e.to_string(),
                }
            }
        });
    }

    fn render_mode_selection(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            Self::page_header(
                ui,
                "Create your account",
                "Generate a new key or import an existing nsec",
            );
            if ui.button("Generate New Keypair").clicked() {
                app.state.onboarding.mode = Some(AccountCreationMode::Generate);
                app.state.onboarding.error_string.clear();
            }
            ui.add_space(12.0);
            ui.label("Import existing private key:");
            ui.add_sized(
                [360.0, 24.0],
                egui::TextEdit::singleline(&mut app.state.onboarding.nsec_input)
                    .password(true)
                    .hint_text("nsec1..."),
            );
            if ui.button("Continue with imported key").clicked() {
                match app
                    .backend
                    .validate_nsec(app.state.onboarding.nsec_input.clone())
                {
                    Ok(account) => {
                        app.state.onboarding.pending_account = Some(account.clone());
                        let (display_name, name, picture_url, fetched) =
                            super::account_setup::fetch_and_prefill_metadata(
                                app,
                                &account.pubkey_hex,
                            );
                        app.state.onboarding.display_name = display_name;
                        app.state.onboarding.name = name;
                        app.state.onboarding.picture_url = picture_url;
                        app.state.onboarding.metadata_fetched = fetched;
                        app.state.onboarding.mode = Some(AccountCreationMode::Import);
                        app.state.onboarding.error_string.clear();
                    }
                    Err(e) => app.state.onboarding.error_string = e.to_string(),
                }
            }
            Self::show_error(ui, &app.state.onboarding.error_string);
        });
    }

    fn render_metadata_step(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            Self::page_header(ui, "Profile metadata", "Optional display details");
            ui.horizontal(|ui| {
                ui.label("Display name");
                ui.text_edit_singleline(&mut app.state.onboarding.display_name);
            });
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut app.state.onboarding.name);
            });
            ui.horizontal(|ui| {
                ui.label("Picture URL");
                ui.text_edit_singleline(&mut app.state.onboarding.picture_url);
            });
            ui.checkbox(
                &mut app.state.onboarding.publish_metadata,
                "Publish metadata to relays",
            );
            Self::show_error(ui, &app.state.onboarding.error_string);
            if ui.button("Save Account").clicked() {
                if Self::save_account(app) {
                    Self::finish_onboarding(app);
                }
            }
            if ui.button("Back").clicked() {
                app.state.onboarding.mode = None;
            }
        });
    }

    fn onboarding_returning(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            Self::page_header(ui, "Welcome Back", "Unlock your existing Hoot database");
            Self::show_error(ui, &app.state.onboarding.error_string);
            Self::password_field(ui, &mut app.state.onboarding.secret_input, "Password");
            if ui.button("Unlock").clicked() {
                match app
                    .backend
                    .unlock_database(app.state.onboarding.secret_input.clone())
                {
                    Ok(()) => {
                        app.status = HootStatus::Initializing;
                        app.page = Page::Inbox;
                    }
                    Err(e) => {
                        error!("{}", e);
                        app.state.onboarding.error_string = e.to_string();
                    }
                }
            }
        });
    }

    fn finish_onboarding(app: &mut Hoot) {
        if let Err(e) = app.backend.mark_onboarding_complete() {
            error!("Failed to write done file: {}", e);
        }
        app.status = HootStatus::Initializing;
        app.page = Page::Inbox;
    }

    fn save_account(app: &mut Hoot) -> bool {
        let result = match app.state.onboarding.mode.clone() {
            Some(AccountCreationMode::Generate) => super::account_setup::generate_account(
                app,
                &app.state.onboarding.display_name.clone(),
                &app.state.onboarding.name.clone(),
                &app.state.onboarding.picture_url.clone(),
                app.state.onboarding.publish_metadata,
            ),
            Some(AccountCreationMode::Import) => super::account_setup::save_imported_account(
                app,
                &app.state.onboarding.nsec_input.clone(),
                &app.state.onboarding.display_name.clone(),
                &app.state.onboarding.name.clone(),
                &app.state.onboarding.picture_url.clone(),
                app.state.onboarding.publish_metadata,
            ),
            None => Err("No account mode selected".to_string()),
        };
        match result {
            Ok(_) => true,
            Err(e) => {
                app.state.onboarding.error_string = e;
                false
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum PasswordStrength {
    Weak,
    Fair,
    Strong,
}
