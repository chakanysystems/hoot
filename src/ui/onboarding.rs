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
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading(egui::RichText::new("Welcome to Hoot Mail! 🦉").size(32.0));
            ui.add_space(20.0);
        });

        match app.page {
            Page::Onboarding => Self::onboarding_home(app, ui),
            Page::OnboardingNew => Self::onboarding_new(app, ui),
            Page::OnboardingNewShowKey => Self::onboarding_new_keypair_generated(app, ui),
            Page::OnboardingReturning => Self::onboarding_returning(app, ui),
            _ => error!("OnboardingScreen should not be displayed when page is not Onboarding!"),
        }
    }

    fn onboarding_home(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label("Choose how you'd like to get started:");
            ui.add_space(20.0);

            let button_size = egui::vec2(240.0, 80.0);
            
            if ui.add(egui::Button::new(
                egui::RichText::new("🆕 I'm new to Hoot Mail")
                    .size(18.0)
            ).min_size(button_size)).clicked() {
                app.page = Page::OnboardingNew;
            }

            ui.add_space(16.0);

            if ui.add(egui::Button::new(
                egui::RichText::new("👋 I've used Hoot Mail before")
                    .size(18.0)
            ).min_size(button_size)).clicked() {
                app.page = Page::OnboardingReturning;
            }
        });
    }

    fn onboarding_new(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            if ui.add(egui::Button::new("← Back").min_size(egui::vec2(80.0, 30.0))).clicked() {
                app.page = Page::Onboarding;
            }
            
            ui.add_space(20.0);
            ui.label(egui::RichText::new("To setup Hoot Mail, you need a nostr identity.").size(16.0));
            ui.add_space(20.0);

            if ui.add(egui::Button::new(
                egui::RichText::new("🔑 Create new keypair")
                    .size(18.0)
            ).min_size(egui::vec2(200.0, 50.0))).clicked() {
                let _ = app.account_manager.generate_keys();
                app.page = Page::OnboardingNewShowKey;
            }
        });
    }

    fn onboarding_new_keypair_generated(app: &mut Hoot, ui: &mut egui::Ui) {
        use crate::keystorage::KeyStorage;
        use nostr::ToBech32;

        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label(egui::RichText::new("🎉 Your identity has been created!").size(24.0));
            ui.add_space(20.0);

            // here, we are assuming that the most recent key added is the one that was generated in
            // onboarding_new()'s button click.
            let first_key = app.account_manager.loaded_keys.last().expect("wanted a key from last screen").clone();
            let pubkey = first_key.public_key().to_bech32().unwrap();
            
            ui.label("Your public key:");
            ui.add_space(8.0);
            
            ui.add(
                egui::TextEdit::multiline(&mut pubkey.to_string())
                    .font(egui::TextStyle::Monospace)
                    .desired_width(ui.available_width())
                    .desired_rows(1)
                    .frame(true)
                    .interactive(false)
            );
            
            ui.add_space(32.0);

            if ui.add(egui::Button::new(
                egui::RichText::new("✨ Start using Hoot Mail")
                    .size(18.0)
            ).min_size(egui::vec2(200.0, 50.0))).clicked() {
                app.account_manager
                    .add_key(&first_key)
                    .expect("could not write key");

                app.page = Page::Inbox;
            }
        });
    }

    fn onboarding_returning(app: &mut Hoot, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            if ui.add(egui::Button::new("← Back").min_size(egui::vec2(80.0, 30.0))).clicked() {
                app.page = Page::Onboarding;
            }
            
            ui.add_space(20.0);
            ui.label(egui::RichText::new("👋 Welcome Back!").size(24.0));
            ui.add_space(20.0);

            let parsed_secret_key = nostr::SecretKey::parse(&app.state.onboarding.secret_input);
            let valid_key = parsed_secret_key.is_ok();
            
            ui.label("Please enter your nsec here:");
            ui.add_space(8.0);
            
            let text_edit = egui::TextEdit::singleline(&mut app.state.onboarding.secret_input)
                .desired_width(400.0)
                .hint_text("nsec1...")
                .font(egui::TextStyle::Monospace);
                
            ui.add(text_edit);
            
            ui.add_space(8.0);
            match valid_key {
                true => ui.colored_label(egui::Color32::from_rgb(34, 197, 94), "✔ Key Valid"),
                false => ui.colored_label(egui::Color32::from_rgb(239, 68, 68), "⊗ Key Invalid"),
            };

            ui.add_space(32.0);

            if ui.add_enabled(valid_key, 
                egui::Button::new(
                    egui::RichText::new("✨ Continue")
                        .size(18.0)
                ).min_size(egui::vec2(200.0, 50.0))
            ).clicked() {
                use crate::keystorage::KeyStorage;
                let keypair = nostr::Keys::new(parsed_secret_key.unwrap());
                let _ = app.account_manager.add_key(&keypair);
                let _ = app.account_manager.load_keys();
                app.page = Page::Inbox;
            }
        });
    }
}
