use crate::{Hoot, Page};
use eframe::egui;
use tracing::error;

#[derive(Default)]
pub struct OnboardingState {
    // for nsecs, etc.
    pub secret_input: String,
}

pub struct OnboardingScreen {}

impl OnboardingScreen {
    pub fn ui(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.heading("Welcome to Hoot Mail!");

        match app.page {
            Page::Onboarding => Self::onboarding_home(app, ui),
            Page::OnboardingNew => Self::onboarding_new(app, ui),
            Page::OnboardingNewShowKey => Self::onboarding_new_keypair_generated(app, ui),
            Page::OnboardingReturning => Self::onboarding_returning(app, ui),
            _ => error!("OnboardingScreen should not be displayed when page is not Onboarding!"),
        }
    }

    fn onboarding_home(app: &mut Hoot, ui: &mut egui::Ui) {
        if ui.button("I am new to Hoot Mail").clicked() {
            app.page = Page::OnboardingNew;
        }

        if ui.button("I have used Hoot Mail before.").clicked() {
            app.page = Page::OnboardingReturning;
        }
    }

    fn onboarding_new(app: &mut Hoot, ui: &mut egui::Ui) {
        if ui.button("Go Back").clicked() {
            app.page = Page::Onboarding;
        }
        ui.label("To setup Hoot Mail, you need a nostr identity.");

        if ui.button("Create new keypair").clicked() {
            let _ = app.account_manager.generate_keys();
            app.page = Page::OnboardingNewShowKey;
        }
    }

    fn onboarding_new_keypair_generated(app: &mut Hoot, ui: &mut egui::Ui) {
        use crate::keystorage::KeyStorage;
        use nostr::ToBech32;

        // here, we are assuming that the most recent key added is the one that was generated in
        // onboarding_new()'s button click.
        let first_key = app
            .account_manager
            .loaded_keys
            .last()
            .expect("wanted a key from last screen")
            .clone();
        ui.label(format!(
            "New identity: {}",
            first_key.public_key().to_bech32().unwrap()
        ));

        if ui.button("OK, Save!").clicked() {
            app.account_manager
                .add_key(&first_key)
                .expect("could not write key");

            app.page = Page::Inbox;
        }
    }

    fn onboarding_returning(app: &mut Hoot, ui: &mut egui::Ui) {
        if ui.button("Go Back").clicked() {
            app.page = Page::Onboarding;
        }
        ui.label("Welcome Back!");

        let parsed_secret_key = nostr::SecretKey::parse(&app.state.onboarding.secret_input);
        let valid_key = parsed_secret_key.is_ok();
        ui.horizontal(|ui| {
            ui.label("Please enter your nsec here:");
            ui.text_edit_singleline(&mut app.state.onboarding.secret_input);
            match valid_key {
                true => ui.colored_label(egui::Color32::LIGHT_GREEN, "✔ Key Valid"),
                false => ui.colored_label(egui::Color32::RED, "⊗ Key Invalid"),
            }
        });

        if ui
            .add_enabled(valid_key, egui::Button::new("Save"))
            .clicked()
        {
            use crate::keystorage::KeyStorage;
            let keypair = nostr::Keys::new(parsed_secret_key.unwrap());
            let _ = app.account_manager.add_key(&keypair);
            let _ = app.account_manager.load_keys();
            app.page = Page::Inbox;
        }
    }
}
