use crate::{Hoot, HootStatus};
use eframe::egui::Ui;
use tracing::error;

#[derive(Debug, Default)]
pub struct UnlockDatabaseState {
    pub secret_input: String,
    pub error_string: String,
}

#[derive(Debug)]
pub struct UnlockDatabase {}

impl UnlockDatabase {
    pub fn ui(app: &mut Hoot, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.heading("Unlock Hoot");
            ui.label("If this is your first time using Hoot, then the password you enter will be your encryption password.");
            ui.text_edit_singleline(&mut app.state.unlock_database.secret_input);
            ui.horizontal(|ui| {
                if ui.button("Unlock").clicked() {
                    match app
                        .db
                        .unlock_with_password(app.state.unlock_database.secret_input.clone())
                    {
                        Ok(v) => {
                            app.state.unlock_database.secret_input = String::new(); // clear input
                            app.status = HootStatus::Initalizing;
                            app.page = crate::Page::Inbox;
                        }
                        Err(e) => {
                            let mut unknown_error = || {
                                app.state.unlock_database.error_string = e.to_string();
                            };
                            error!("Error when trying to load database: {}", e);
                            app.state.unlock_database.secret_input = String::new();
                            match e.downcast_ref::<rusqlite_migration::Error>() {
                                Some(rusqlite_migration::Error::RusqliteError { query, err }) => {
                                    match err.sqlite_error_code() {
                                        Some(rusqlite::ErrorCode::NotADatabase) => {
                                            // So, here this case would likely mean that the unlock didn't go so well.
                                            // But, it is "NotADatabase", so it could also mean that it's... not a database. aka corruption?
                                            error!("Wrong password given or the database is corrupted.");
                                            app.state.unlock_database.error_string = "Wrong password".into();
                                        }
                                        Some(_) => unknown_error(),
                                        None => unknown_error()
                                    }
                                }
                                Some(_) => unknown_error(),
                                None => unknown_error()                            }
                        }
                    }
                }

                ui.label(&app.state.unlock_database.error_string);
            });
        });
    }
}
