use eframe::egui::{self, RichText, Sense, Vec2b};
use egui_extras::{Column, TableBuilder};
use tracing::error;

use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::Hoot;
use crate::Page;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.add_space(8.0);

    // Top bar with search
    ui.horizontal(|ui| {
        if ui.button("Refresh").clicked() {
            match app.db.get_top_level_messages() {
                Ok(msgs) => app.table_entries = msgs,
                Err(e) => {
                    error!("Could not fetch table entries to display from DB: {}", e)
                }
            }
        }
        ui.add_space(16.0);
        let search_width = ui.available_width() - 100.0;
        ui.add_sized(
            [search_width, 32.0],
            egui::TextEdit::singleline(&mut String::new())
                .hint_text("Search")
                .margin(egui::vec2(8.0, 4.0)),
        );
    });

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    if app.table_entries.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No messages yet")
                    .size(16.0)
                    .color(style::TEXT_MUTED),
            );
        });
    } else {
        // Email list using TableBuilder
        TableBuilder::new(ui)
            .column(Column::auto()) // Checkbox
            .column(Column::auto()) // Star
            .column(Column::initial(160.0).at_least(100.0)) // Sender
            .column(Column::remainder()) // Subject
            .column(Column::initial(100.0).at_least(70.0)) // Time
            .striped(true)
            .sense(Sense::click())
            .auto_shrink(Vec2b { x: false, y: false })
            .header(28.0, |mut header| {
                header.col(|ui| {
                    ui.checkbox(&mut false, "");
                });
                header.col(|ui| {
                    ui.label(RichText::new("⭐").size(12.0));
                });
                header.col(|ui| {
                    ui.label(RichText::new("From").small().color(style::TEXT_MUTED));
                });
                header.col(|ui| {
                    ui.label(RichText::new("Subject").small().color(style::TEXT_MUTED));
                });
                header.col(|ui| {
                    ui.label(RichText::new("Date").small().color(style::TEXT_MUTED));
                });
            })
            .body(|body| {
                let events: Vec<crate::TableEntry> = app.table_entries.to_vec();
                body.rows(style::INBOX_ROW_HEIGHT, events.len(), |mut row| {
                    let event = &events[row.index()];

                    row.col(|ui| {
                        ui.checkbox(&mut false, "");
                    });
                    row.col(|ui| {
                        ui.checkbox(&mut false, "");
                    });
                    row.col(|ui| {
                        let _ = get_profile_metadata(app, event.pubkey.clone());
                        let label = app
                            .resolve_name(&event.pubkey)
                            .unwrap_or_else(|| event.pubkey.to_string());
                        ui.label(RichText::new(label).strong());
                    });
                    row.col(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(&event.subject);
                            if event.thread_count > 1 {
                                ui.label(
                                    RichText::new(format!("{}", event.thread_count))
                                        .small()
                                        .color(style::TEXT_MUTED),
                                );
                            }
                        });
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(style::format_timestamp(event.created_at))
                                .color(style::TEXT_MUTED)
                                .small(),
                        );
                    });

                    if row.response().clicked() {
                        app.focused_post = event.id.clone();
                        app.page = Page::Post;
                        app.show_trashed_post = false;
                    }
                });
            });
    }
}
