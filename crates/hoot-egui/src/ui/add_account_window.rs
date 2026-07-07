use super::account_setup::AccountCreationMode;
use crate::{style, Page};
use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Vec2};
use hoot_backend::AccountSummary;
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq)]
pub enum AccountCreationStep {
    DatabaseSetup,
    DatabaseUnlock,
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
    pub imported_account: Option<AccountSummary>,
    pub imported_nsec: Option<String>,

    // Generated key preview
    pub generated_account: Option<AccountSummary>,
    pub generated_nsec: Option<String>,

    // Metadata fields
    pub display_name: String,
    pub name: String,
    pub picture_url: String,
    pub metadata_fetched: bool,

    // UI state
    pub error_message: Option<String>,
    pub publish_metadata: bool,
    pub db_password_input: String,
    pub db_password_confirm_input: String,
}

impl Default for AddAccountWindowState {
    fn default() -> Self {
        Self {
            mode: None,
            step: AccountCreationStep::ModeSelection,
            nsec_input: String::new(),
            imported_account: None,
            imported_nsec: None,
            generated_account: None,
            generated_nsec: None,
            display_name: String::new(),
            name: String::new(),
            picture_url: String::new(),
            metadata_fetched: false,
            error_message: None,
            publish_metadata: true,
            db_password_input: String::new(),
            db_password_confirm_input: String::new(),
        }
    }
}

pub struct AddAccountWindow {}

pub const ONBOARDING_ADD_ACCOUNT_WINDOW_ID: &str = "onboarding_add_account_window";

pub(crate) fn should_render_onboarding_account_window(page: &Page) -> bool {
    matches!(page, Page::OnboardingNewUser)
}

pub(crate) fn keeps_onboarding_account_window_state(page: &Page) -> bool {
    matches!(page, Page::OnboardingNewUser | Page::OnboardingNewShowKey)
}

impl AddAccountWindow {
    /// Main rendering function - returns false if window should be closed
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) -> bool {
        let is_onboarding = id == egui::Id::new(ONBOARDING_ADD_ACCOUNT_WINDOW_ID);
        if is_onboarding && !should_render_onboarding_account_window(&app.page) {
            return keeps_onboarding_account_window_state(&app.page);
        }

        let mut keep_open = true;
        let mut close_clicked = false;
        let mut should_close_from_save = false;
        let window_width = 620.0;
        let window_height = 540.0;

        let window = egui::Window::new("add_account_window")
            .id(id)
            .title_bar(false)
            .fixed_size([window_width, window_height])
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(
                egui::Frame::new()
                    .fill(style::SURFACE)
                    .stroke(Stroke::new(1.0, style::border_strong()))
                    .corner_radius(CornerRadius::same(12))
                    .shadow(style::shadow_lg()),
            );

        window.open(&mut keep_open).show(ctx, |ui| {
            should_close_from_save =
                Self::render_contents(app, ui, id, is_onboarding, &mut close_clicked);
        });

        if close_clicked && is_onboarding {
            app.page = Page::Onboarding;
        }

        if is_onboarding && !should_render_onboarding_account_window(&app.page) {
            return keeps_onboarding_account_window_state(&app.page);
        }

        keep_open && !close_clicked && !should_close_from_save
    }

    pub(crate) fn render_onboarding_panel(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        let mut close_clicked = false;
        let _ = Self::render_contents(app, ui, id, true, &mut close_clicked);
        if close_clicked {
            app.page = Page::Onboarding;
        }
    }

    fn render_contents(
        app: &mut crate::Hoot,
        ui: &mut egui::Ui,
        id: egui::Id,
        is_onboarding: bool,
        close_clicked: &mut bool,
    ) -> bool {
        let mut should_close_from_save = false;
        ui.set_min_width(560.0);
        ui.vertical(|ui| {
            Self::render_window_header(ui, close_clicked, is_onboarding);
            ui.add_space(4.0);

            let content_width = 572.0;
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.vertical(|ui| {
                    ui.set_max_width(content_width);
                    ui.add_space(18.0);

                    let db_initialized = app.backend.is_database_initialized().unwrap_or(false);
                    let db_file_has_password = app.backend.db_file_has_password().unwrap_or(false);
                    {
                        let state = app.state.add_account_window.get_mut(&id).unwrap();
                        Self::sync_database_gate_step(
                            db_initialized,
                            db_file_has_password,
                            state,
                            is_onboarding,
                        );
                    }

                    let current_step = app
                        .state
                        .add_account_window
                        .get(&id)
                        .map(|s| s.step.clone())
                        .unwrap_or(AccountCreationStep::ModeSelection);

                    ui.label(
                        RichText::new(if is_onboarding {
                            "Create your first identity"
                        } else {
                            "Add an account"
                        })
                        .size(18.0)
                        .strong()
                        .color(style::TEXT),
                    );
                    ui.add_space(4.0);
                    let subtitle = if is_onboarding {
                        match current_step {
                            AccountCreationStep::DatabaseSetup => {
                                "Create a database password to protect your local data."
                            }
                            AccountCreationStep::DatabaseUnlock => {
                                "Enter your database password to continue."
                            }
                            AccountCreationStep::ModeSelection => {
                                "Choose whether to generate a new key or import an existing one."
                            }
                            AccountCreationStep::ImportKey => {
                                "Paste an existing private key and validate it locally."
                            }
                            AccountCreationStep::ConfigureMetadata => {
                                "Review and adjust profile details before saving your account."
                            }
                            AccountCreationStep::Review => "Review everything before the account is saved.",
                        }
                    } else {
                        "Use an existing key or create a new one. Nothing is saved until you confirm."
                    };
                    ui.label(RichText::new(subtitle).size(13.0).color(style::TEXT2));
                    ui.add_space(18.0);

                    Self::render_step_indicator_for_step(ui, &current_step, is_onboarding);
                    ui.add_space(18.0);

                    if let Some(error) = app
                        .state
                        .add_account_window
                        .get(&id)
                        .and_then(|s| s.error_message.clone())
                    {
                        Self::render_error_banner(ui, &error);
                        ui.add_space(16.0);
                    }

                    match current_step {
                        AccountCreationStep::DatabaseSetup => {
                            Self::render_database_setup_step(app, ui, id, is_onboarding)
                        }
                        AccountCreationStep::DatabaseUnlock => {
                            if Self::render_database_unlock_step(app, ui, id, is_onboarding) {
                                should_close_from_save = true;
                            }
                        }
                        AccountCreationStep::ModeSelection => {
                            Self::render_mode_selection(app, ui, id, is_onboarding)
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

                    ui.add_space(22.0);
                });
                ui.add_space(24.0);
            });
        });
        should_close_from_save
    }

    fn render_window_header(ui: &mut egui::Ui, close_clicked: &mut bool, is_onboarding: bool) {
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            ui.allocate_ui_with_layout(
                egui::vec2((ui.available_width() - 32.0).max(0.0), 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        RichText::new("Identity Setup")
                            .size(14.0)
                            .strong()
                            .color(style::TEXT),
                    );

                    if !is_onboarding {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if style::pointer(ui.add(Self::secondary_button("Close"))).clicked() {
                                *close_clicked = true;
                            }
                        });
                    }
                },
            );
            ui.add_space(14.0);
        });
        ui.add_space(12.0);

        let y = ui.cursor().top();
        ui.painter().line_segment(
            [
                egui::Pos2::new(ui.min_rect().left() + 1.0, y),
                egui::Pos2::new(ui.min_rect().right() - 1.0, y),
            ],
            Stroke::new(1.0, style::border()),
        );
    }

    fn render_step_indicator_for_step(
        ui: &mut egui::Ui,
        step: &AccountCreationStep,
        is_onboarding: bool,
    ) {
        let (active_index, steps): (usize, &[&str]) = if is_onboarding {
            (
                match step {
                    AccountCreationStep::DatabaseSetup | AccountCreationStep::DatabaseUnlock => 0,
                    AccountCreationStep::ModeSelection => 1,
                    AccountCreationStep::ImportKey => 2,
                    AccountCreationStep::ConfigureMetadata => 3,
                    AccountCreationStep::Review => 4,
                },
                &["Database", "Method", "Key", "Profile", "Review"],
            )
        } else {
            (
                match step {
                    AccountCreationStep::DatabaseSetup | AccountCreationStep::DatabaseUnlock => 0,
                    AccountCreationStep::ModeSelection => 0,
                    AccountCreationStep::ImportKey => 1,
                    AccountCreationStep::ConfigureMetadata => 2,
                    AccountCreationStep::Review => 3,
                },
                &["Method", "Key", "Profile", "Review"],
            )
        };

        ui.horizontal(|ui| {
            let mut previous_chip_rect: Option<egui::Rect> = None;
            for (index, label) in steps.iter().enumerate() {
                let is_active = index == active_index;
                let is_complete = index < active_index;

                let (fill, text_color) = if is_active {
                    (style::ACCENT, Color32::WHITE)
                } else if is_complete {
                    (style::accent_soft(), style::ACCENT)
                } else {
                    (style::SURFACE2, style::TEXT2)
                };

                let chip_response = egui::Frame::new()
                    .fill(fill)
                    .stroke(Stroke::new(
                        1.0,
                        if is_active {
                            style::ACCENT
                        } else {
                            style::border_strong()
                        },
                    ))
                    .corner_radius(CornerRadius::same(255))
                    .inner_margin(Margin {
                        left: 10,
                        right: 10,
                        top: 5,
                        bottom: 5,
                    })
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("{}. {}", index + 1, label))
                                .size(11.5)
                                .color(text_color)
                                .strong(),
                        );
                    });

                let chip_rect = chip_response.response.rect;
                if let Some(prev_rect) = previous_chip_rect {
                    let y = (prev_rect.center().y + chip_rect.center().y) * 0.5;
                    // Draw edge-to-edge (with tiny overlap) so the connector never collapses.
                    let x1 = prev_rect.right() - 1.0;
                    let x2 = chip_rect.left() + 1.0;
                    if x2 >= x1 {
                        ui.painter().line_segment(
                            [egui::pos2(x1, y), egui::pos2(x2, y)],
                            Stroke::new(1.0, style::border_strong()),
                        );
                    }
                }
                previous_chip_rect = Some(chip_rect);
            }
        });
    }

    fn sync_database_gate_step(
        db_initialized: bool,
        db_file_has_password: bool,
        state: &mut AddAccountWindowState,
        is_onboarding: bool,
    ) {
        if !is_onboarding {
            if matches!(
                state.step,
                AccountCreationStep::DatabaseSetup | AccountCreationStep::DatabaseUnlock
            ) {
                state.step = AccountCreationStep::ModeSelection;
            }
            return;
        }

        if db_initialized {
            if matches!(
                state.step,
                AccountCreationStep::DatabaseSetup | AccountCreationStep::DatabaseUnlock
            ) {
                state.step = AccountCreationStep::ModeSelection;
                state.error_message = None;
                state.db_password_input.clear();
                state.db_password_confirm_input.clear();
            }
        } else {
            state.step = if db_file_has_password {
                AccountCreationStep::DatabaseUnlock
            } else {
                AccountCreationStep::DatabaseSetup
            };
        }
    }

    fn render_database_setup_step(
        app: &mut crate::Hoot,
        ui: &mut egui::Ui,
        id: egui::Id,
        is_onboarding: bool,
    ) {
        let (mut password, mut confirm) = {
            let state = app.state.add_account_window.get(&id).unwrap();
            (
                state.db_password_input.clone(),
                state.db_password_confirm_input.clone(),
            )
        };

        ui.label(
            RichText::new("Password")
                .size(12.0)
                .strong()
                .color(style::TEXT2),
        );
        let password_response = style::boxed_password_text_edit(
            ui,
            id.with("db_password_setup"),
            &mut password,
            "Enter password",
            13.5,
        );
        if !password.is_empty() {
            ui.add_space(6.0);
            let (color, label) = match check_password_strength(&password) {
                PasswordStrength::Weak => (egui::Color32::RED, "Weak"),
                PasswordStrength::Fair => (egui::Color32::YELLOW, "Fair"),
                PasswordStrength::Strong => (egui::Color32::LIGHT_GREEN, "Strong"),
            };
            ui.label(
                RichText::new(format!("Strength: {label}"))
                    .size(11.5)
                    .color(color),
            );
        }
        ui.add_space(10.0);

        ui.label(
            RichText::new("Confirm password")
                .size(12.0)
                .strong()
                .color(style::TEXT2),
        );
        let confirm_response = style::boxed_password_text_edit(
            ui,
            id.with("db_password_confirm"),
            &mut confirm,
            "Re-enter password",
            13.5,
        );
        let passwords_match = password == confirm;
        if !passwords_match && !confirm.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Passwords do not match")
                    .size(11.5)
                    .color(egui::Color32::from_rgb(185, 28, 28)),
            );
        }
        ui.add_space(16.0);

        {
            let state = app.state.add_account_window.get_mut(&id).unwrap();
            state.db_password_input = password.clone();
            state.db_password_confirm_input = confirm.clone();
        }

        let can_continue = passwords_match && password.len() >= 4;
        let submit_via_enter = (password_response.lost_focus() || confirm_response.lost_focus())
            && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let continue_clicked =
                style::pointer(ui.add_enabled(can_continue, Self::primary_button("Continue")))
                    .clicked();
            if continue_clicked || (submit_via_enter && can_continue) {
                match app.backend.unlock_database(password.clone()) {
                    Ok(_) => {
                        let state = app.state.add_account_window.get_mut(&id).unwrap();
                        state.db_password_input.clear();
                        state.db_password_confirm_input.clear();
                        state.error_message = None;
                        state.step = AccountCreationStep::ModeSelection;
                    }
                    Err(e) => {
                        let state = app.state.add_account_window.get_mut(&id).unwrap();
                        state.error_message = Some(format!("Failed to set password: {e}"));
                        error!("Failed to initialize database password: {}", e);
                    }
                }
            }

            if is_onboarding && style::pointer(ui.add(Self::secondary_button("Back"))).clicked() {
                app.page = Page::Onboarding;
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.error_message = None;
                state.db_password_input.clear();
                state.db_password_confirm_input.clear();
            }
        });
    }

    fn render_database_unlock_step(
        app: &mut crate::Hoot,
        ui: &mut egui::Ui,
        id: egui::Id,
        is_onboarding: bool,
    ) -> bool {
        let mut password = app
            .state
            .add_account_window
            .get(&id)
            .unwrap()
            .db_password_input
            .clone();

        ui.label(
            RichText::new("Password")
                .size(12.0)
                .strong()
                .color(style::TEXT2),
        );
        let password_response = style::boxed_password_text_edit(
            ui,
            id.with("db_password_unlock"),
            &mut password,
            "Enter password",
            13.5,
        );
        ui.add_space(16.0);

        app.state
            .add_account_window
            .get_mut(&id)
            .unwrap()
            .db_password_input = password.clone();

        let should_close = false;
        let submit_via_enter =
            password_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_unlock = !password.is_empty();
            let unlock_clicked =
                style::pointer(ui.add_enabled(can_unlock, Self::primary_button("Unlock")))
                    .clicked();
            if unlock_clicked || (submit_via_enter && can_unlock) {
                match app.backend.unlock_database(password.clone()) {
                    Ok(_) => {
                        let state = app.state.add_account_window.get_mut(&id).unwrap();
                        state.db_password_input.clear();
                        state.error_message = None;

                        state.step = AccountCreationStep::ModeSelection;
                    }
                    Err(e) => {
                        error!("Failed to unlock database: {}", e);
                        let state = app.state.add_account_window.get_mut(&id).unwrap();
                        state.db_password_input.clear();
                        state.error_message = Some(e.to_string());
                    }
                }
            }

            if is_onboarding && style::pointer(ui.add(Self::secondary_button("Back"))).clicked() {
                app.page = Page::Onboarding;
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.error_message = None;
                state.db_password_input.clear();
            }
        });

        should_close
    }

    fn render_mode_selection(
        app: &mut crate::Hoot,
        ui: &mut egui::Ui,
        id: egui::Id,
        is_onboarding: bool,
    ) {
        ui.label(
            RichText::new("Choose how you want to continue.")
                .size(13.0)
                .color(style::TEXT2),
        );
        ui.add_space(14.0);

        if Self::option_card(
            ui,
            "Generate a new identity",
            "Create a fresh keypair for Hoot. You can name it before saving it.",
            "Create New Keypair",
            true,
        ) {
            let state = app.state.add_account_window.get_mut(&id).unwrap();
            state.mode = Some(AccountCreationMode::Generate);
            state.error_message = None;

            match app.backend.generate_account_preview() {
                Ok(preview) => {
                    let pubkey = preview.summary.pubkey_hex.clone();
                    let (display_name, name, picture_url, fetched) =
                        super::account_setup::fetch_and_prefill_metadata(app, &pubkey);
                    let state = app.state.add_account_window.get_mut(&id).unwrap();
                    state.generated_account = Some(preview.summary);
                    state.generated_nsec = Some(preview.nsec);
                    state.imported_account = None;
                    state.imported_nsec = None;
                    state.display_name = display_name;
                    state.name = name;
                    state.picture_url = picture_url;
                    state.metadata_fetched = fetched;
                    state.step = AccountCreationStep::ConfigureMetadata;
                }
                Err(e) => {
                    let state = app.state.add_account_window.get_mut(&id).unwrap();
                    state.error_message = Some(format!("Failed to generate account: {e}"));
                }
            }
        }

        ui.add_space(12.0);

        if Self::option_card(
            ui,
            "Import an existing key",
            "Paste an `nsec` private key. Hoot will validate it before anything is saved.",
            "Use Existing Key",
            false,
        ) {
            let state = app.state.add_account_window.get_mut(&id).unwrap();
            state.mode = Some(AccountCreationMode::Import);
            state.step = AccountCreationStep::ImportKey;
            state.error_message = None;
            state.generated_account = None;
            state.generated_nsec = None;
        }

        if is_onboarding {
            ui.add_space(16.0);
            if style::pointer(ui.add(Self::secondary_button("Back"))).clicked() {
                app.page = Page::Onboarding;
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.error_message = None;
            }
        }
    }

    fn render_import_step(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        ui.label(
            RichText::new("Paste your private key.")
                .size(13.0)
                .strong()
                .color(style::TEXT),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("We only use this locally to add the account.")
                .size(12.0)
                .color(style::TEXT3),
        );
        ui.add_space(12.0);

        let mut nsec_input = app
            .state
            .add_account_window
            .get(&id)
            .unwrap()
            .nsec_input
            .clone();

        let input_id = id.with("nsec_input");
        let input_response =
            style::boxed_password_text_edit(ui, input_id, &mut nsec_input, "nsec1...", 13.0);
        if input_response.changed() {
            app.state
                .add_account_window
                .get_mut(&id)
                .unwrap()
                .error_message = None;
        }
        ui.add_space(10.0);

        app.state
            .add_account_window
            .get_mut(&id)
            .unwrap()
            .nsec_input = nsec_input.clone();

        let validation_result = Self::validate_nsec(app, &nsec_input);
        match &validation_result {
            Ok(account) => {
                let npub = account.npub.clone();
                Self::render_status_banner(
                    ui,
                    "Valid key",
                    "This key looks valid. The matching public address is shown below.",
                    Color32::from_rgba_unmultiplied(22, 163, 74, 18),
                    style::GREEN,
                );
                ui.add_space(10.0);
                Self::render_mono_field(ui, "Public address", &npub);
            }
            Err(e) if !nsec_input.is_empty() => {
                Self::render_status_banner(
                    ui,
                    "Key format issue",
                    e,
                    Color32::from_rgba_unmultiplied(220, 38, 38, 18),
                    Color32::from_rgb(185, 28, 28),
                );
            }
            _ => {}
        }

        ui.add_space(18.0);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next_enabled = validation_result.is_ok();

            if style::pointer(ui.add_enabled(next_enabled, Self::primary_button("Continue")))
                .clicked()
            {
                if let Ok(account) = validation_result {
                    if super::account_setup::account_already_exists(app, &account.pubkey_hex) {
                        app.state
                            .add_account_window
                            .get_mut(&id)
                            .unwrap()
                            .error_message = Some("This account is already added".to_string());
                    } else {
                        let pubkey_str = account.pubkey_hex.clone();

                        let state = app.state.add_account_window.get_mut(&id).unwrap();
                        state.imported_account = Some(account);
                        state.imported_nsec = Some(nsec_input.clone());
                        state.error_message = None;

                        let (display_name, name, picture_url, fetched) =
                            super::account_setup::fetch_and_prefill_metadata(app, &pubkey_str);
                        let state = app.state.add_account_window.get_mut(&id).unwrap();
                        state.display_name = display_name;
                        state.name = name;
                        state.picture_url = picture_url;
                        state.metadata_fetched = fetched;
                        state.step = AccountCreationStep::ConfigureMetadata;
                    }
                }
            }

            if style::pointer(ui.add(Self::secondary_button("Back"))).clicked() {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.step = AccountCreationStep::ModeSelection;
                state.error_message = None;
                state.nsec_input.clear();
            }
        });
    }

    fn render_metadata_step(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        let (npub, metadata_fetched) = {
            let state = app.state.add_account_window.get(&id).unwrap();
            let account = if let Some(account) = &state.generated_account {
                account
            } else if let Some(account) = &state.imported_account {
                account
            } else {
                ui.label("Error: No key found");
                return;
            };

            (account.npub.clone(), state.metadata_fetched)
        };

        ui.label(
            RichText::new("Review the profile details for this account.")
                .size(13.0)
                .color(style::TEXT2),
        );
        ui.add_space(12.0);

        Self::render_mono_field(ui, "Public address", &npub);
        ui.add_space(10.0);

        if metadata_fetched {
            Self::render_status_banner(
                ui,
                "Profile found",
                "Existing profile details were loaded from your relays. Edit anything before saving.",
                Color32::from_rgba_unmultiplied(124, 58, 237, 16),
                style::ACCENT,
            );
            ui.add_space(10.0);
        }

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

        ui.label(
            RichText::new("Display name")
                .size(11.5)
                .strong()
                .color(style::TEXT3),
        );
        style::underline_text_edit(ui, &mut display_name, "", 13.5);
        ui.add_space(10.0);

        ui.label(
            RichText::new("Username")
                .size(11.5)
                .strong()
                .color(style::TEXT3),
        );
        style::underline_text_edit(ui, &mut name, "@username", 13.5);
        ui.add_space(10.0);

        ui.label(
            RichText::new("Picture URL")
                .size(11.5)
                .strong()
                .color(style::TEXT3),
        );
        style::underline_text_edit(ui, &mut picture_url, "https://...", 13.5);
        ui.add_space(14.0);

        egui::Frame::new()
            .fill(style::SURFACE2)
            .stroke(Stroke::new(1.0, style::border()))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                style::pointer(ui.checkbox(&mut publish_metadata, "Publish profile details to relays"));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Turn this off if you want to add the key now and publish profile changes later.",
                    )
                    .size(11.5)
                    .color(style::TEXT3),
                );
            });
        ui.add_space(18.0);

        let state = app.state.add_account_window.get_mut(&id).unwrap();
        state.display_name = display_name;
        state.name = name;
        state.picture_url = picture_url;
        state.publish_metadata = publish_metadata;

        let mode = state.mode.clone();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if style::pointer(ui.add(Self::primary_button("Continue"))).clicked() {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.step = AccountCreationStep::Review;
                state.error_message = None;
            }

            if style::pointer(ui.add(Self::secondary_button("Skip"))).clicked() {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                state.step = AccountCreationStep::Review;
                state.publish_metadata = false;
                state.error_message = None;
            }

            if style::pointer(ui.add(Self::secondary_button("Back"))).clicked() {
                let state = app.state.add_account_window.get_mut(&id).unwrap();
                match mode {
                    Some(AccountCreationMode::Generate) => {
                        state.step = AccountCreationStep::ModeSelection;
                        state.generated_account = None;
                        state.generated_nsec = None;
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
        ui.label(
            RichText::new("Check the account before saving.")
                .size(13.0)
                .color(style::TEXT2),
        );
        ui.add_space(14.0);

        let (account_type, npub, display_name, name, picture_url, publish_metadata, is_generate) = {
            let state = app.state.add_account_window.get(&id).unwrap();
            let account = if let Some(account) = &state.generated_account {
                account
            } else if let Some(account) = &state.imported_account {
                account
            } else {
                ui.label("Error: No key found");
                return false;
            };

            let account_type = match &state.mode {
                Some(AccountCreationMode::Generate) => "New keypair",
                Some(AccountCreationMode::Import) => "Imported key",
                None => "Unknown",
            };

            (
                account_type,
                account.npub.clone(),
                state.display_name.clone(),
                state.name.clone(),
                state.picture_url.clone(),
                state.publish_metadata,
                matches!(state.mode, Some(AccountCreationMode::Generate)),
            )
        };

        let has_metadata = !display_name.is_empty() || !name.is_empty() || !picture_url.is_empty();

        egui::Frame::new()
            .fill(style::BG)
            .stroke(Stroke::new(1.0, style::border()))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                Self::summary_row(ui, "Account type", account_type, false);
                Self::summary_row(ui, "Public address", &npub, true);
                if has_metadata {
                    if !display_name.is_empty() {
                        Self::summary_row(ui, "Display name", &display_name, false);
                    }
                    if !name.is_empty() {
                        Self::summary_row(ui, "Username", &name, false);
                    }
                    if !picture_url.is_empty() {
                        Self::summary_row(ui, "Picture URL", &picture_url, false);
                    }
                } else {
                    Self::summary_row(ui, "Profile", "No profile details set", false);
                }
                Self::summary_row(
                    ui,
                    "Relay publish",
                    if publish_metadata {
                        "Profile details will be published"
                    } else {
                        "Profile details will stay local for now"
                    },
                    false,
                );
            });

        ui.add_space(14.0);

        if is_generate {
            Self::render_status_banner(
                ui,
                "Private key reminder",
                "This creates a brand new identity. Make sure you export the private key after saving it.",
                Color32::from_rgb(254, 243, 199),
                Color32::from_rgb(146, 64, 14),
            );
            ui.add_space(14.0);
        }

        let mut should_close = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if style::pointer(ui.add(Self::primary_button("Save Account"))).clicked() {
                let state_clone = app.state.add_account_window.get(&id).unwrap().clone();

                match Self::save_account(app, &state_clone) {
                    Ok(_) => {
                        info!("Account saved successfully");
                        if id == egui::Id::new(ONBOARDING_ADD_ACCOUNT_WINDOW_ID) {
                            let is_generate = matches!(
                                app.state.add_account_window.get(&id).unwrap().mode,
                                Some(AccountCreationMode::Generate)
                            );
                            app.page = if is_generate {
                                Page::OnboardingNewShowKey
                            } else {
                                Page::OnboardingRelay
                            };
                        }
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

            if style::pointer(ui.add(Self::secondary_button("Back"))).clicked() {
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

    fn validate_nsec(app: &crate::Hoot, input: &str) -> Result<AccountSummary, String> {
        super::account_setup::validate_nsec(app, input)
    }

    fn save_account(app: &mut crate::Hoot, state: &AddAccountWindowState) -> Result<(), String> {
        let nsec = match state.mode {
            Some(AccountCreationMode::Generate) => state
                .generated_nsec
                .as_deref()
                .ok_or_else(|| "Generated account is missing its private key".to_string())?,
            Some(AccountCreationMode::Import) => state
                .imported_nsec
                .as_deref()
                .ok_or_else(|| "Imported account is missing its private key".to_string())?,
            None => return Err("No account creation mode selected".to_string()),
        };

        super::account_setup::save_imported_account(
            app,
            nsec,
            &state.display_name,
            &state.name,
            &state.picture_url,
            state.publish_metadata,
        )
        .map(|_| ())
    }

    fn render_error_banner(ui: &mut egui::Ui, message: &str) {
        Self::render_status_banner(
            ui,
            "There was a problem",
            message,
            Color32::from_rgba_unmultiplied(220, 38, 38, 18),
            Color32::from_rgb(185, 28, 28),
        );
    }

    fn render_status_banner(
        ui: &mut egui::Ui,
        title: &str,
        body: &str,
        fill: Color32,
        text_color: Color32,
    ) {
        egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, fill.gamma_multiply(0.9)))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.label(RichText::new(title).size(12.0).strong().color(text_color));
                ui.add_space(3.0);
                ui.label(RichText::new(body).size(11.5).color(text_color));
            });
    }

    fn render_mono_field(ui: &mut egui::Ui, label: &str, value: &str) {
        ui.label(RichText::new(label).size(11.5).strong().color(style::TEXT3));
        ui.add_space(4.0);
        egui::Frame::new()
            .fill(style::SURFACE2)
            .stroke(Stroke::new(1.0, style::border()))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin {
                left: 10,
                right: 10,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(value)
                            .size(11.5)
                            .family(egui::FontFamily::Monospace)
                            .color(style::TEXT2),
                    )
                    .wrap(),
                );
            });
    }

    fn option_card(
        ui: &mut egui::Ui,
        title: &str,
        description: &str,
        action_label: &str,
        accented: bool,
    ) -> bool {
        let mut clicked = false;
        egui::Frame::new()
            .fill(style::BG)
            .stroke(Stroke::new(
                1.0,
                if accented {
                    style::border_strong()
                } else {
                    style::border()
                },
            ))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.label(RichText::new(title).size(14.0).strong().color(style::TEXT));
                ui.add_space(6.0);
                ui.label(RichText::new(description).size(12.0).color(style::TEXT2));
                ui.add_space(14.0);

                let button = if accented {
                    Self::primary_button(action_label)
                } else {
                    Self::secondary_button(action_label)
                };
                if style::pointer(ui.add(button)).clicked() {
                    clicked = true;
                }
            });
        clicked
    }

    fn summary_row(ui: &mut egui::Ui, label: &str, value: &str, mono: bool) {
        ui.horizontal(|ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(label).size(11.5).strong().color(style::TEXT3));
            ui.add_space(10.0);
            let text = if mono {
                RichText::new(value)
                    .size(11.5)
                    .family(egui::FontFamily::Monospace)
                    .color(style::TEXT2)
            } else {
                RichText::new(value).size(12.0).color(style::TEXT2)
            };
            ui.add(egui::Label::new(text).wrap().selectable(false));
        });
        ui.add_space(8.0);
    }

    fn primary_button(label: &str) -> egui::Button<'_> {
        egui::Button::new(
            RichText::new(label)
                .size(12.5)
                .strong()
                .color(Color32::WHITE),
        )
        .fill(style::ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(Vec2::new(112.0, 32.0))
    }

    fn secondary_button(label: &str) -> egui::Button<'_> {
        egui::Button::new(RichText::new(label).size(12.0).color(style::TEXT2))
            .fill(style::SURFACE2)
            .stroke(Stroke::new(1.0, style::border_strong()))
            .corner_radius(CornerRadius::same(8))
            .min_size(Vec2::new(72.0, 32.0))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_account_window_keeps_generated_key_backup_state_without_rendering_modal() {
        assert!(should_render_onboarding_account_window(
            &Page::OnboardingNewUser
        ));
        assert!(keeps_onboarding_account_window_state(
            &Page::OnboardingNewUser
        ));

        assert!(!should_render_onboarding_account_window(
            &Page::OnboardingNewShowKey
        ));
        assert!(keeps_onboarding_account_window_state(
            &Page::OnboardingNewShowKey
        ));

        for page in [
            Page::Onboarding,
            Page::OnboardingRelay,
            Page::OnboardingReady,
            Page::Inbox,
        ] {
            assert!(!should_render_onboarding_account_window(&page));
            assert!(!keeps_onboarding_account_window_state(&page));
        }
    }
}
