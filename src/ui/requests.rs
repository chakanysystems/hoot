use eframe::egui::{self, RichText, Sense, Vec2b};
use egui_extras::{Column, TableBuilder};
use tracing::error;

use crate::db::sender_status::SenderStatus;
use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::Hoot;
use crate::Page;
use crate::TableEntry;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.heading("Requests");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh").clicked() {
                app.refresh_requests();
            }
        });
    });

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    if app.request_entries.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No message requests")
                    .size(16.0)
                    .color(style::TEXT_MUTED),
            );
        });
    } else {
        let mut to_accept: Option<String> = None;
        let mut to_reject: Option<String> = None;

        TableBuilder::new(ui)
            .column(Column::initial(160.0).at_least(100.0))
            .column(Column::remainder())
            .column(Column::initial(100.0).at_least(70.0))
            .column(Column::initial(140.0).at_least(120.0))
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
                let events: Vec<TableEntry> = app.request_entries.to_vec();
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
                            if ui.button("Accept").clicked() {
                                to_accept = Some(event.pubkey.clone());
                            }
                            if ui.button("Reject").clicked() {
                                to_reject = Some(event.pubkey.clone());
                            }
                        });
                    });

                    if row.response().clicked() {
                        app.focused_post = event.id.clone();
                        app.page = Page::Post;
                        app.show_trashed_post = false;
                    }
                });
            });

        if let Some(pubkey) = to_accept {
            if let Err(e) = app.db.set_sender_status(&pubkey, &SenderStatus::Allowed) {
                error!("Failed to accept sender: {}", e);
            } else {
                app.refresh_requests();
                match app.db.get_top_level_messages() {
                    Ok(msgs) => app.table_entries = msgs,
                    Err(e) => error!("Could not refresh inbox: {}", e),
                }
            }
        }

        if let Some(pubkey) = to_reject {
            if let Err(e) = app.db.set_sender_status(&pubkey, &SenderStatus::Junked) {
                error!("Failed to reject sender: {}", e);
            } else {
                app.refresh_requests();
                app.refresh_junk();
            }
        }
    }
}
