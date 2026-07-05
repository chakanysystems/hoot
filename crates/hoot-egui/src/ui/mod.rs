pub mod account_setup;
pub mod add_account_window;
pub mod compose_window;
pub mod contacts;
pub mod drafts_page;
pub mod inbox;
pub mod junk;
pub mod onboarding;
pub mod requests;
pub mod search;
pub mod settings;
pub mod thread_view;
pub mod trash;

use eframe::egui;

pub fn page_header(ui: &mut egui::Ui, title: &str) {
    egui::TopBottomPanel::top(egui::Id::new(format!("page_header_{}", title)))
        .exact_height(48.0)
        .frame(
            egui::Frame::new()
                .fill(crate::style::SURFACE)
                .inner_margin(egui::Margin::symmetric(16, 8)),
        )
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(15.0)
                        .strong()
                        .color(crate::style::TEXT),
                );
            });
        });
}

pub fn empty_state(ui: &mut egui::Ui, message: &str) {
    let available = ui.available_size();
    ui.add_space(available.y / 2.0 - 12.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(message)
                .size(14.0)
                .color(crate::style::TEXT2),
        );
    });
}

pub mod nip05_status;
pub mod unlock_window;

