use eframe::egui::{self, RichText};
use tracing::error;

use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::Hoot;
use crate::Page;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    super::page_header(ui, "Junk");

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(style::BG))
        .show_inside(ui, |ui| {
            if app.junk_entries.is_empty() {
                super::empty_state(ui, "No junk messages.");
                return;
            }

            let mut to_unblock: Option<String> = None;
            let mut to_view: Option<String> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    let events = app.junk_entries.clone();
                    for event in &events {
                        let _ = get_profile_metadata(app, event.pubkey.clone());
                        let label = app
                            .resolve_name(&event.pubkey)
                            .unwrap_or_else(|| event.pubkey.to_string());
                        let timestamp = style::format_timestamp(event.created_at);

                        let available_width = ui.available_width();
                        egui::Frame::new().fill(style::BG).show(ui, |ui| {
                            ui.set_min_width(available_width);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_min_width(available_width - 160.0);
                                    ui.label(RichText::new(&label).size(13.5).color(style::TEXT));
                                    ui.label(
                                        RichText::new(&event.subject)
                                            .size(12.0)
                                            .color(style::TEXT2),
                                    );
                                    ui.label(
                                        RichText::new(&timestamp).size(11.5).color(style::TEXT3),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if style::pointer(ui.button("Unblock")).clicked() {
                                            to_unblock = Some(event.pubkey.clone());
                                        }
                                        if style::pointer(ui.button("View")).clicked() {
                                            to_view = Some(event.id.clone());
                                        }
                                    },
                                );
                            });
                        });

                        ui.add_space(4.0);
                        ui.painter().line_segment(
                            [
                                egui::Pos2::new(ui.min_rect().left() + 8.0, ui.cursor().top()),
                                egui::Pos2::new(ui.min_rect().right() - 8.0, ui.cursor().top()),
                            ],
                            egui::Stroke::new(1.0, style::border()),
                        );
                        ui.add_space(4.0);
                    }
                    ui.add_space(8.0);
                });

            if let Some(event_id) = to_view {
                app.focused_post = event_id;
                app.page = Page::Post;
                app.show_trashed_post = false;
            }

            if let Some(pubkey) = to_unblock {
                if let Err(e) = app.backend.remove_sender_status(pubkey) {
                    error!("Failed to unblock sender: {}", e);
                } else {
                    app.refresh_junk();
                    app.refresh_requests();
                }
            }
        });
}
