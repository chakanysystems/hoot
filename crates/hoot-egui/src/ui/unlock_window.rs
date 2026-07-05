use crate::{style, HootStatus, Page};
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};
use tracing::error;

pub const UNLOCK_WINDOW_ID: &str = "unlock_window";

#[derive(Default)]
pub struct UnlockWindowState {
    pub password_input: String,
    pub error_message: Option<String>,
}

pub struct UnlockWindow;

impl UnlockWindow {
    /// Renders the standalone database unlock window.
    /// Returns false if the window should be closed.
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) -> bool {
        let screen_rect = ctx.viewport_rect();
        let window_width = (screen_rect.width() - 64.0).clamp(560.0, 720.0);
        let window_height = (screen_rect.height() - 64.0).clamp(480.0, 760.0);

        let mut keep_open = true;

        let window = egui::Window::new("unlock_window")
            .id(id)
            .title_bar(false)
            .fixed_size([window_width, window_height])
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(
                egui::Frame::new()
                    .fill(style::SURFACE)
                    .stroke(egui::Stroke::new(1.0, style::border_strong()))
                    .corner_radius(CornerRadius::same(12))
                    .shadow(style::shadow_lg()),
            );

        window.open(&mut keep_open).show(ctx, |ui| {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.vertical(|ui| {
                    ui.set_max_width(640.0);
                    ui.add_space(18.0);

                    ui.label(
                        RichText::new("Unlock Hoot")
                            .size(18.0)
                            .strong()
                            .color(style::TEXT),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Enter your database password to continue.")
                            .size(13.0)
                            .color(style::TEXT2),
                    );
                    ui.add_space(18.0);

                    let (mut password, error_message) = {
                        let state = app.state.unlock_window.get(&id).unwrap();
                        (state.password_input.clone(), state.error_message.clone())
                    };

                    if let Some(err) = &error_message {
                        egui::Frame::new()
                            .fill(Color32::from_rgba_unmultiplied(220, 38, 38, 18))
                            .stroke(Stroke::new(
                                1.0,
                                Color32::from_rgba_unmultiplied(220, 38, 38, 18)
                                    .gamma_multiply(0.9),
                            ))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::same(12))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("There was a problem")
                                        .size(12.0)
                                        .strong()
                                        .color(Color32::from_rgb(185, 28, 28)),
                                );
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new(err.as_str())
                                        .size(11.5)
                                        .color(Color32::from_rgb(185, 28, 28)),
                                );
                            });
                        ui.add_space(16.0);
                    }

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

                    {
                        let state = app.state.unlock_window.get_mut(&id).unwrap();
                        state.password_input = password.clone();
                    }

                    let submit_via_enter = password_response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_unlock = !password.is_empty();
                        let unlock_clicked = style::pointer(
                            ui.add_enabled(can_unlock, Self::primary_button("Unlock")),
                        )
                        .clicked();
                        if unlock_clicked || (submit_via_enter && can_unlock) {
                            match app.backend.unlock_database(password.clone()) {
                                Ok(_) => {
                                    let state = app.state.unlock_window.get_mut(&id).unwrap();
                                    state.password_input.clear();
                                    state.error_message = None;
                                    app.status = HootStatus::Initializing;
                                    app.page = Page::Inbox;
                                }
                                Err(e) => {
                                    error!("Failed to unlock database: {}", e);
                                    let state = app.state.unlock_window.get_mut(&id).unwrap();
                                    state.password_input.clear();
                                    state.error_message = Some(e.to_string());
                                }
                            }
                        }
                    });

                    ui.add_space(22.0);
                });
                ui.add_space(24.0);
            });
        });

        keep_open
    }

    fn primary_button(label: &str) -> egui::Button<'_> {
        egui::Button::new(
            RichText::new(label)
                .size(12.5)
                .strong()
                .color(egui::Color32::WHITE),
        )
        .fill(style::ACCENT)
        .stroke(egui::Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(Vec2::new(112.0, 32.0))
    }
}
