use crate::{Hoot, HootStatus};
use eframe::egui;
use eframe::egui::Ui;
use tracing::error;

#[derive(Debug, Default)]
pub struct UnlockDatabaseState {
    pub secret_input: String,
    pub error_string: String,
}

#[derive(Debug)]
pub struct UnlockDatabase;

impl UnlockDatabase {
    pub fn ui(app: &mut Hoot, ui: &mut Ui) {
        egui::Frame::none()
            .inner_margin(egui::Margin::same(20.0))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);

                    ui.heading(egui::RichText::new("Unlock Hoot").size(24.0));
                    ui.add_space(10.0);

                    ui.label(
                        egui::RichText::new("Enter your password to unlock the database")
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add_space(30.0);

                    if !app.state.unlock_database.error_string.is_empty() {
                        ui.colored_label(
                            egui::Color32::RED,
                            format!("⚠ {}", app.state.unlock_database.error_string),
                        );
                        ui.add_space(10.0);
                    }

                    ui.label("Password:");
                    ui.add_sized(
                        [300.0, 24.0],
                        egui::TextEdit::singleline(&mut app.state.unlock_database.secret_input)
                            .password(true)
                            .hint_text("Enter password"),
                    );
                    ui.add_space(20.0);

                    if ui
                        .add_sized(
                            [200.0, 40.0],
                            egui::Button::new(egui::RichText::new("Unlock").size(15.0)),
                        )
                        .clicked()
                    {
                        Self::attempt_unlock(app);
                    }
                });
            });
    }

    fn attempt_unlock(app: &mut Hoot) {
        match app
            .db
            .unlock_with_password(app.state.unlock_database.secret_input.clone())
        {
            Ok(_) => {
                app.state.unlock_database.secret_input.clear();
                app.state.unlock_database.error_string.clear();
                app.status = HootStatus::Initializing;
                app.page = crate::Page::Inbox;
            }
            Err(e) => {
                error!("Error when trying to load database: {}", e);
                app.state.unlock_database.secret_input.clear();
                app.state.unlock_database.error_string = crate::db::format_unlock_error(&e);
            }
        }
    }
}
