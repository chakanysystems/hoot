use super::account_setup::AccountCreationMode;
use eframe::egui::{self, RichText};
use hoot_backend::AccountSummary;

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
    pub nsec_input: String,
    pub pending_account: Option<AccountSummary>,
    pub display_name: String,
    pub name: String,
    pub picture_url: String,
    pub metadata_fetched: bool,
    pub error_message: Option<String>,
    pub publish_metadata: bool,
}

impl Default for AddAccountWindowState {
    fn default() -> Self {
        Self {
            mode: None,
            step: AccountCreationStep::ModeSelection,
            nsec_input: String::new(),
            pending_account: None,
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
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) -> bool {
        let mut keep_open = true;
        let mut should_close = false;
        let screen_rect = ctx.screen_rect();
        egui::Window::new("Add Account")
            .id(id)
            .default_size([500.0, 400.0])
            .min_width(450.0)
            .min_height(350.0)
            .default_pos([
                screen_rect.center().x - 250.0,
                screen_rect.center().y - 200.0,
            ])
            .open(&mut keep_open)
            .show(ctx, |ui| {
                let step = app
                    .state
                    .add_account_window
                    .get(&id)
                    .map(|state| state.step.clone())
                    .unwrap_or(AccountCreationStep::ModeSelection);
                Self::render_step_indicator(ui, &step);
                if let Some(error) = app
                    .state
                    .add_account_window
                    .get(&id)
                    .and_then(|state| state.error_message.clone())
                {
                    ui.colored_label(egui::Color32::RED, format!("⚠ {}", error));
                    ui.add_space(5.0);
                }
                match step {
                    AccountCreationStep::ModeSelection => Self::render_mode_selection(app, ui, id),
                    AccountCreationStep::ImportKey => Self::render_import_step(app, ui, id),
                    AccountCreationStep::ConfigureMetadata => {
                        Self::render_metadata_step(app, ui, id)
                    }
                    AccountCreationStep::Review => {
                        if Self::render_review_step(app, ui, id) {
                            should_close = true;
                        }
                    }
                }
            });
        keep_open && !should_close
    }

    fn render_step_indicator(ui: &mut egui::Ui, step: &AccountCreationStep) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Step:").strong());
            ui.label(match step {
                AccountCreationStep::ModeSelection => "1. Choose Method",
                AccountCreationStep::ImportKey => "2. Import Key",
                AccountCreationStep::ConfigureMetadata => "Configure Metadata",
                AccountCreationStep::Review => "Review",
            });
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
                state.step = AccountCreationStep::ConfigureMetadata;
                state.error_message = None;
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
        let mut nsec_input = app
            .state
            .add_account_window
            .get(&id)
            .unwrap()
            .nsec_input
            .clone();
        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(&mut nsec_input)
                .hint_text("nsec1...")
                .password(true),
        );
        app.state
            .add_account_window
            .get_mut(&id)
            .unwrap()
            .nsec_input = nsec_input.clone();
        let validation_result = super::account_setup::validate_nsec(app, &nsec_input);
        ui.horizontal(|ui| match &validation_result {
            Ok(_) => ui.colored_label(egui::Color32::GREEN, "✓ Valid nsec format"),
            Err(e) if !nsec_input.is_empty() => {
                ui.colored_label(egui::Color32::RED, format!("⊗ {}", e))
            }
            _ => ui.label(""),
        });
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Next").clicked() {
                match validation_result {
                    Ok(account) => {
                        if super::account_setup::account_already_exists(app, &account.pubkey_hex) {
                            app.state
                                .add_account_window
                                .get_mut(&id)
                                .unwrap()
                                .error_message = Some("This account is already added".to_string());
                        } else {
                            let state = app.state.add_account_window.get_mut(&id).unwrap();
                            state.pending_account = Some(account.clone());
                            let (display_name, name, picture_url, fetched) =
                                super::account_setup::fetch_and_prefill_metadata(
                                    app,
                                    &account.pubkey_hex,
                                );
                            let state = app.state.add_account_window.get_mut(&id).unwrap();
                            state.display_name = display_name;
                            state.name = name;
                            state.picture_url = picture_url;
                            state.metadata_fetched = fetched;
                            state.step = AccountCreationStep::ConfigureMetadata;
                            state.error_message = None;
                        }
                    }
                    Err(e) => {
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
                    AccountCreationStep::ModeSelection;
            }
        });
    }

    fn render_metadata_step(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        ui.add_space(10.0);
        ui.label("Profile Metadata (Optional):");
        let state = app.state.add_account_window.get_mut(&id).unwrap();
        ui.horizontal(|ui| {
            ui.label("Display Name:");
            ui.text_edit_singleline(&mut state.display_name);
        });
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut state.name);
        });
        ui.horizontal(|ui| {
            ui.label("Picture URL:");
            ui.text_edit_singleline(&mut state.picture_url);
        });
        ui.checkbox(&mut state.publish_metadata, "Publish metadata to relays");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Review").clicked() {
                app.state.add_account_window.get_mut(&id).unwrap().step =
                    AccountCreationStep::Review;
            }
            if ui.button("Back").clicked() {
                let step = if app.state.add_account_window.get(&id).unwrap().mode
                    == Some(AccountCreationMode::Import)
                {
                    AccountCreationStep::ImportKey
                } else {
                    AccountCreationStep::ModeSelection
                };
                app.state.add_account_window.get_mut(&id).unwrap().step = step;
            }
        });
    }

    fn render_review_step(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) -> bool {
        let state = app.state.add_account_window.get(&id).unwrap().clone();
        ui.add_space(10.0);
        ui.label(RichText::new("Review Account").strong());
        match state.mode {
            Some(AccountCreationMode::Generate) => ui.label("Mode: Generate new account"),
            Some(AccountCreationMode::Import) => ui.label("Mode: Import existing account"),
            None => ui.label("Mode: Unknown"),
        };
        if let Some(account) = &state.pending_account {
            ui.label(format!("Public key: {}", account.pubkey_hex));
        }
        ui.label(format!("Display name: {}", state.display_name));
        ui.label(format!("Name: {}", state.name));
        ui.label(format!("Picture: {}", state.picture_url));
        let mut should_close = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Save Account").clicked() {
                let result = match state.mode {
                    Some(AccountCreationMode::Generate) => super::account_setup::generate_account(
                        app,
                        &state.display_name,
                        &state.name,
                        &state.picture_url,
                        state.publish_metadata,
                    ),
                    Some(AccountCreationMode::Import) => {
                        super::account_setup::save_imported_account(
                            app,
                            &state.nsec_input,
                            &state.display_name,
                            &state.name,
                            &state.picture_url,
                            state.publish_metadata,
                        )
                    }
                    None => Err("No account mode selected".to_string()),
                };
                match result {
                    Ok(_) => should_close = true,
                    Err(e) => {
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
            }
        });
        should_close
    }
}
