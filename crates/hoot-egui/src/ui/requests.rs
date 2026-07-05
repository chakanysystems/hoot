use eframe::egui::{self, RichText};
use tracing::error;

use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::Hoot;
use crate::Page;
use hoot_backend::SenderStatusDto;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    super::page_header(ui, "Requests");

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(style::BG))
        .show_inside(ui, |ui| {
            if app.request_entries.is_empty() {
                super::empty_state(ui, "No message requests.");
                return;
            }

            let mut to_accept: Option<String> = None;
            let mut to_reject: Option<String> = None;
            let mut to_view: Option<String> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    let events = app.request_entries.clone();
                    for event in &events {
                        let _ = get_profile_metadata(app, event.pubkey.clone());
                        let label = app
                            .resolve_name(&event.pubkey)
                            .unwrap_or_else(|| event.pubkey.to_string());

                        // Check for NIP-05 and show warning if unverified
                        let has_unverified_nip05 =
                            if let Ok(Some(cached)) = app.backend.get_cached_nip05(event.pubkey.clone()) {
                                cached.last_verified.is_none()
                            } else if let Ok(Some(meta)) =
                                app.backend.get_profile_metadata(event.pubkey.clone())
                            {
                                meta.nip05.is_some()
                            } else {
                                false
                            };

                        let timestamp = style::format_timestamp(event.created_at);

                        let available_width = ui.available_width();
                        egui::Frame::new()
                            .fill(style::BG)
                            .show(ui, |ui| {
                                ui.set_min_width(available_width);
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_min_width(available_width - 220.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&label)
                                                    .size(13.5)
                                                    .color(style::TEXT),
                                            );
                                            if has_unverified_nip05 {
                                                ui.colored_label(egui::Color32::YELLOW, "⚠")
                                                    .on_hover_text("This sender has an unverified NIP-05 identifier. Proceed with caution.");
                                            }
                                        });
                                        ui.label(
                                            RichText::new(&event.subject)
                                                .size(12.0)
                                                .color(style::TEXT2),
                                        );
                                        ui.label(
                                            RichText::new(&timestamp)
                                                .size(11.5)
                                                .color(style::TEXT3),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if style::pointer(ui
                                                .button(
                                                    RichText::new("Block")
                                                        .color(egui::Color32::from_rgb(200, 50, 50)),
                                                ))
                                                .clicked()
                                            {
                                                to_reject = Some(event.pubkey.clone());
                                            }
                                            if style::pointer(ui.button("Accept")).clicked() {
                                                to_accept = Some(event.pubkey.clone());
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

            if let Some(pubkey) = to_accept {
                if let Err(e) = app.backend.set_sender_status(pubkey, SenderStatusDto::Allowed) {
                    error!("Failed to accept sender: {}", e);
                } else {
                    app.refresh_requests();
                    app.refresh_inbox();
                }
            }

            if let Some(pubkey) = to_reject {
                if let Err(e) = app.backend.set_sender_status(pubkey, SenderStatusDto::Junked) {
                    error!("Failed to reject sender: {}", e);
                } else {
                    app.refresh_requests();
                    app.refresh_junk();
                }
            }
        });
}
