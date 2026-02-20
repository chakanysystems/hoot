use eframe::egui::{self, Color32, RichText, Vec2b};
use egui_extras::{Column, TableBuilder};
use nostr::EventId;
use tracing::error;

use crate::db;
use crate::style;
use crate::ui;
use crate::Hoot;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.heading("Drafts");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh").clicked() {
                app.refresh_drafts();
            }
        });
    });

    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    if app.drafts.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No drafts")
                    .size(16.0)
                    .color(style::TEXT_MUTED),
            );
        });
    } else {
        let mut draft_to_delete: Option<i64> = None;
        let mut draft_to_open: Option<db::Draft> = None;

        TableBuilder::new(ui)
            .column(Column::initial(200.0).at_least(100.0)) // Subject
            .column(Column::initial(200.0).at_least(100.0)) // To
            .column(Column::initial(120.0).at_least(80.0)) // Last Modified
            .column(Column::initial(60.0).at_least(60.0)) // Actions
            .striped(true)
            .auto_shrink(Vec2b { x: false, y: false })
            .header(28.0, |mut header| {
                header.col(|ui| {
                    ui.label(RichText::new("Subject").small().color(style::TEXT_MUTED));
                });
                header.col(|ui| {
                    ui.label(RichText::new("To").small().color(style::TEXT_MUTED));
                });
                header.col(|ui| {
                    ui.label(
                        RichText::new("Last Modified")
                            .small()
                            .color(style::TEXT_MUTED),
                    );
                });
                header.col(|ui| {
                    ui.label(RichText::new("").small());
                });
            })
            .body(|body| {
                let drafts: Vec<db::Draft> = app.drafts.clone();
                body.rows(style::INBOX_ROW_HEIGHT, drafts.len(), |mut row| {
                    let draft = &drafts[row.index()];

                    row.col(|ui| {
                        let subject = if draft.subject.is_empty() {
                            "(No Subject)"
                        } else {
                            &draft.subject
                        };
                        if ui.link(RichText::new(subject).strong()).clicked() {
                            draft_to_open = Some(draft.clone());
                        }
                    });
                    row.col(|ui| {
                        let to = if draft.to_field.is_empty() {
                            "(No Recipient)"
                        } else {
                            &draft.to_field
                        };
                        ui.label(RichText::new(to).color(style::TEXT_MUTED));
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(style::format_timestamp(draft.updated_at))
                                .color(style::TEXT_MUTED)
                                .small(),
                        );
                    });
                    row.col(|ui| {
                        if ui
                            .button(RichText::new("X").color(Color32::RED))
                            .on_hover_text("Delete draft")
                            .clicked()
                        {
                            draft_to_delete = Some(draft.id);
                        }
                    });
                });
            });

        if let Some(draft) = draft_to_open {
            let parent_events: Vec<EventId> = draft
                .parent_events
                .iter()
                .filter_map(|s| EventId::parse(s).ok())
                .collect();
            let selected_account = draft.selected_account.as_ref().and_then(|pk_str| {
                app.account_manager
                    .loaded_keys
                    .iter()
                    .find(|k| k.public_key().to_string() == *pk_str)
                    .cloned()
            });
            let state = ui::compose_window::ComposeWindowState {
                subject: draft.subject,
                to_field: draft.to_field,
                content: draft.content,
                parent_events,
                selected_account,
                selected_nip05: None,
                minimized: false,
                draft_id: Some(draft.id),
                send_status: None,
            };
            app.state
                .compose_window
                .insert(egui::Id::new(rand::random::<u32>()), state);
        }

        if let Some(id) = draft_to_delete {
            if let Err(e) = app.db.delete_draft(id) {
                error!("Failed to delete draft: {}", e);
            }
            app.refresh_drafts();
        }
    }
}
