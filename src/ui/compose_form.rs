use eframe::egui::{self, Color32, CornerRadius, Frame, Layout, RichText, Stroke};
use crate::style;

/// Renders a compact reply/compose box: textarea + footer with send button.
/// Returns `true` if Send was clicked (caller should send and clear `content`).
pub fn render_reply_box(ui: &mut egui::Ui, content: &mut String) -> bool {
    let mut send_clicked = false;

    Frame::new()
        .fill(style::SURFACE)
        .stroke(Stroke::new(1.0, style::border_strong()))
        .corner_radius(CornerRadius::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // Textarea
            ui.add(
                egui::TextEdit::multiline(content)
                    .hint_text("Write a reply…")
                    .desired_rows(4)
                    .frame(false)
                    .font(egui::FontId::proportional(14.0))
                    .desired_width(f32::INFINITY),
            );

            // Footer separator line
            let footer_top = ui.cursor().top();
            let avail_width = ui.available_width();
            ui.painter().line_segment(
                [
                    egui::pos2(ui.min_rect().left(), footer_top),
                    egui::pos2(ui.min_rect().left() + avail_width, footer_top),
                ],
                Stroke::new(1.0, style::border()),
            );

            ui.add_space(1.0); // ensure separator is allocated

            ui.horizontal(|ui| {
                ui.add_space(4.0);

                // Attach placeholder button
                ui.add(
                    egui::Button::new(RichText::new("📎").size(13.0).color(style::TEXT3))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .min_size(egui::vec2(28.0, 28.0)),
                );

                // Encrypted status
                ui.label(
                    RichText::new("🔒 end-to-end encrypted")
                        .size(11.0)
                        .color(style::GREEN),
                );

                // Send button right-aligned
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    let btn = ui.add(
                        egui::Button::new(
                            RichText::new("Send").size(13.0).color(Color32::WHITE),
                        )
                        .fill(style::ACCENT)
                        .corner_radius(CornerRadius::same(7))
                        .min_size(egui::vec2(60.0, 30.0)),
                    );
                    if btn.clicked() {
                        send_clicked = true;
                    }
                });
            });

            ui.add_space(4.0);
        });

    send_clicked
}
