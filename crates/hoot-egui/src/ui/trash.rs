use eframe::egui::{self, RichText, Sense, Vec2b};
use egui_extras::{Column, TableBuilder};
use tracing::error;

use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::Hoot;
use crate::Page;
use crate::TableEntry;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.heading("Trash");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh").clicked() {
                app.refresh_trash();
            }
        });
    });

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    if app.trash_entries.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Trash is empty")
                    .size(16.0)
                    .color(style::TEXT_MUTED),
            );
        });
    } else {
        let mut to_restore: Option<String> = None;
        let mut to_delete: Option<String> = None;

        TableBuilder::new(ui)
            .column(Column::initial(160.0).at_least(100.0)) // Sender
            .column(Column::remainder()) // Subject
            .column(Column::initial(100.0).at_least(70.0)) // Time
            .column(Column::initial(140.0).at_least(120.0)) // Actions
            .striped(true)
            .sense(Sense::click())
            .auto_shrink(Vec2b { x: false, y: false })
            .header(28.0, |mut header| {
                header.col(|ui| {
                    ui.label(RichText::new("From").small().color(style::TEXT_MUTED));
                });
                header.col(|ui| {
                    ui.label(RichText::new("Subject").small().color(style::TEXT_MUTED));
                });
                header.col(|ui| {
                    ui.label(RichText::new("Date").small().color(style::TEXT_MUTED));
                });
                header.col(|ui| {
                    ui.label(RichText::new("Actions").small().color(style::TEXT_MUTED));
                });
            })
            .body(|body| {
                let events: Vec<TableEntry> = app.trash_entries.to_vec();
                body.rows(style::INBOX_ROW_HEIGHT, events.len(), |mut row| {
                    let event = &events[row.index()];

                    row.col(|ui| {
                        let _ = get_profile_metadata(app, event.pubkey.clone());
                        let label = app
                            .resolve_name(&event.pubkey)
                            .unwrap_or_else(|| event.pubkey.to_string());
                        ui.label(RichText::new(label).strong());
                    });
                    row.col(|ui| {
                        ui.label(&event.subject);
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(style::format_timestamp(event.created_at))
                                .color(style::TEXT_MUTED)
                                .small(),
                        );
                    });
                    row.col(|ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Restore").clicked() {
                                to_restore = Some(event.id.clone());
                            }
                            if ui.button("Delete now").clicked() {
                                // TODO: broadcast NIP-09 EventDeletion to relays
                                to_delete = Some(event.id.clone());
                            }
                        });
                    });

                    if row.response().clicked() {
                        app.focused_post = event.id.clone();
                        app.page = Page::Post;
                        app.show_trashed_post = true;
                    }
                });
            });

        if let Some(event_id) = to_restore {
            if let Err(e) = app.backend.restore_from_trash(event_id) {
                error!("Failed to restore from trash: {}", e);
            } else {
                app.refresh_inbox();
                app.refresh_trash();
            }
        }

        if let Some(event_id) = to_delete {
            if let Err(e) = app.backend.delete_messages_permanently(vec![event_id]) {
                error!("Failed to delete trashed event: {}", e);
            } else {
                app.refresh_trash();
            }
        }
    }
}
