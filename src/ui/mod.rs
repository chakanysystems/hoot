use eframe::egui;

pub mod add_account_window;
pub mod compose_window;
pub mod onboarding;
pub mod settings;
pub mod unlock_database;

pub trait View {
    fn ui(&mut self, ui: &mut egui::Ui);
}
