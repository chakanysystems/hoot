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

pub fn list_row(ui: &mut egui::Ui, primary: &str, secondary: &str, meta: &str) -> egui::Response {
    use eframe::egui::{CornerRadius, FontId, Pos2, Sense, Stroke, StrokeKind, Vec2};
    let available_width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(available_width, 56.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if ui.is_rect_visible(rect) {
        let bg = if response.hovered() {
            crate::style::SURFACE
        } else {
            crate::style::BG
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
        if response.hovered() {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(1.0, crate::style::border_strong()),
                StrokeKind::Outside,
            );
        }
        let x = rect.left() + 16.0;
        let meta_galley = ui.painter().layout_no_wrap(
            meta.to_string(),
            FontId::proportional(11.5),
            crate::style::TEXT3,
        );
        let meta_x = rect.right() - 12.0 - meta_galley.size().x;
        ui.painter().galley(
            Pos2::new(meta_x, rect.top() + 12.0),
            meta_galley,
            crate::style::TEXT3,
        );
        let text_width = (meta_x - x - 8.0).max(0.0);
        let primary_galley = ui.painter().layout(
            primary.to_string(),
            FontId::proportional(13.5),
            crate::style::TEXT,
            text_width,
        );
        ui.painter().galley(
            Pos2::new(x, rect.top() + 12.0),
            primary_galley,
            crate::style::TEXT,
        );
        let secondary_galley = ui.painter().layout(
            secondary.to_string(),
            FontId::proportional(12.0),
            crate::style::TEXT2,
            text_width,
        );
        ui.painter().galley(
            Pos2::new(x, rect.top() + 32.0),
            secondary_galley,
            crate::style::TEXT2,
        );
    }
    response
}

pub mod nip05_status;
pub mod unlock_database;

/// Resolve the best display name for a pubkey and the corresponding initials,
/// triggering a metadata fetch if needed.
pub fn resolve_avatar_info(app: &mut crate::Hoot, pubkey: &str) -> (String, String) {
    let _ = crate::profile_metadata::get_profile_metadata(app, pubkey.to_string());
    let name = app
        .resolve_name(pubkey)
        .unwrap_or_else(|| pubkey.to_string());
    let initials = crate::style::initials_for_name(&name);
    (name, initials)
}
