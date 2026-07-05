use super::account_setup::AccountCreationMode;
use crate::{style, Hoot, Page};
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

impl OnboardingState {
    fn with_default_relays() -> Self {
        Self {
            relays: vec!["wss://talon.quest".to_string()],
            publish_metadata: true,
            ..Default::default()
        }
    }
}

pub struct OnboardingScreen;

impl OnboardingScreen {
    pub fn ui(app: &mut Hoot, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        egui::Frame::new().fill(style::BG).show(ui, |ui| {
            ui.set_min_size(avail);
            if app.page == Page::Onboarding {
                ui.vertical_centered(|ui| {
                    let estimated_card_height = 170.0;
                    let top_pad = ((avail.y - estimated_card_height) * 0.5).max(0.0);
                    ui.add_space(top_pad);
                    render_welcome(app, ui);
                });
                return;
            }

            ui.vertical_centered(|ui| {
                ui.set_max_width(420.0);
                ui.add_space(avail.y * 0.12);

                match app.page {
                    Page::OnboardingNewUser => Self::render_new_user_steps(app, ui),
                    Page::OnboardingNewShowKey => render_show_key(app, ui),
                    Page::OnboardingReturning => Self::render_returning_steps(app, ui),
                    Page::OnboardingRelay => render_relays(app, ui),
                    Page::OnboardingReady => render_ready(app, ui),
                    _ => {}
                }
            });
        });
    }

    fn render_new_user_steps(app: &mut Hoot, ui: &mut egui::Ui) {
        let onboarding_window_id =
            egui::Id::new(super::add_account_window::ONBOARDING_ADD_ACCOUNT_WINDOW_ID);

        app.state
            .add_account_window
            .entry(onboarding_window_id)
            .or_insert_with(super::add_account_window::AddAccountWindowState::default);

        ui.label(
            RichText::new("Set up your account")
                .size(24.0)
                .strong()
                .color(style::TEXT),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Complete identity setup in the account window. You'll choose relays right after.",
            )
            .size(14.0)
            .color(style::TEXT2),
        );
    }

    fn render_returning_steps(app: &mut Hoot, ui: &mut egui::Ui) {
        Self::render_new_user_steps(app, ui);
    }

    fn finish_onboarding(app: &mut Hoot) {
        if let Err(e) = app.backend.mark_onboarding_complete() {
            error!("Failed to mark onboarding complete: {}", e);
        }
        app.page = Page::Inbox;
    }
}

fn onboarding_primary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        RichText::new(label)
            .size(13.0)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(style::ACCENT)
    .stroke(egui::Stroke::NONE)
    .corner_radius(CornerRadius::same(8))
    .min_size(Vec2::new(240.0, 40.0))
}

fn render_welcome(app: &mut Hoot, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(style::SURFACE)
        .stroke(egui::Stroke::new(1.0, style::border_strong()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_min_width(380.0);
            ui.set_max_width(380.0);
            ui.label(RichText::new("Hoot").size(28.0).strong().color(style::TEXT));
            ui.add_space(4.0);
            ui.label(
                RichText::new("Email that belongs to you.")
                    .size(13.0)
                    .color(style::TEXT3),
            );
            ui.add_space(16.0);

            if style::pointer(ui.add(onboarding_primary_button("Get Started"))).clicked() {
                app.page = Page::OnboardingNewUser;
                app.state.onboarding = OnboardingState::with_default_relays();
                app.state.add_account_window.remove(&egui::Id::new(
                    super::add_account_window::ONBOARDING_ADD_ACCOUNT_WINDOW_ID,
                ));
            }
        });
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
        let mut npub_clone = account.npub;
        ui.label(
            RichText::new("Your address (share this freely)")
                .size(12.0)
                .strong()
                .color(style::TEXT3),
        );
        ui.add_space(4.0);
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
                let mut nsec_clone = nsec;
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
        style::pointer(ui.checkbox(
            &mut app.state.onboarding.key_saved,
            "I've saved my private key somewhere safe",
        ));
        ui.add_space(16.0);

        if style::pointer(
            ui.add_enabled(
                app.state.onboarding.key_saved,
                egui::Button::new(RichText::new("Continue").size(14.0).color(Color32::WHITE))
                    .fill(style::ACCENT)
                    .corner_radius(CornerRadius::same(10))
                    .min_size(Vec2::new(240.0, 44.0)),
            ),
        )
        .clicked()
        {
            app.page = Page::OnboardingRelay;
        }
    } else if style::pointer(ui.button("← Back")).clicked() {
        app.page = Page::OnboardingNewUser;
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
                if style::pointer(
                    ui.add(
                        egui::Button::new(RichText::new("Remove").size(12.0).color(style::TEXT3))
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                    ),
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
        ui.add(
            egui::TextEdit::singleline(&mut app.state.onboarding.relay_input)
                .hint_text("wss://relay.damus.io")
                .desired_width(260.0),
        );
        if style::pointer(
            ui.add(
                egui::Button::new(RichText::new("Add").color(style::ACCENT))
                    .fill(style::accent_soft())
                    .corner_radius(CornerRadius::same(6)),
            ),
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
    if style::pointer(
        ui.add(
            egui::Button::new(RichText::new("Continue").size(14.0).color(Color32::WHITE))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(240.0, 44.0)),
        ),
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
    if style::pointer(
        ui.add(
            egui::Button::new(RichText::new("Skip for now").size(13.0).color(style::TEXT3))
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE),
        ),
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
    if style::pointer(
        ui.add(
            egui::Button::new(RichText::new("Open Hoot").size(15.0).color(Color32::WHITE))
                .fill(style::ACCENT)
                .corner_radius(CornerRadius::same(10))
                .min_size(Vec2::new(240.0, 48.0)),
        ),
    )
    .clicked()
    {
        app.page = Page::Inbox;
        OnboardingScreen::finish_onboarding(app);
    }
}
