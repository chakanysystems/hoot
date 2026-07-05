use eframe::egui;

use crate::style;
use crate::ui;
use crate::Hoot;
use hoot_backend::DraftDto;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    super::page_header(ui, "Drafts");

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(style::BG))
        .show_inside(ui, |ui| {
            if app.drafts.is_empty() {
                super::empty_state(ui, "No drafts.");
                return;
            }

            let mut draft_to_delete: Option<i64> = None;
            let mut draft_to_open: Option<DraftDto> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    let drafts: Vec<DraftDto> = app.drafts.clone();
                    for draft in &drafts {
                        let mut open_clicked = false;
                        let mut delete_clicked = false;
                        let subject = if draft.subject.is_empty() {
                            "(No Subject)"
                        } else {
                            &draft.subject
                        };
                        let to = if draft.to_field.is_empty() {
                            "(No Recipient)"
                        } else {
                            &draft.to_field
                        };
                        let timestamp = style::format_timestamp(draft.updated_at);

                        let available_width = ui.available_width();
                        let row_resp = egui::Frame::new().fill(style::BG).show(ui, |ui| {
                            ui.set_min_width(available_width);
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_min_width(available_width - 80.0);
                                    ui.label(
                                        egui::RichText::new(subject).size(13.5).color(style::TEXT),
                                    );
                                    ui.label(
                                        egui::RichText::new(to).size(12.0).color(style::TEXT2),
                                    );
                                    ui.label(
                                        egui::RichText::new(&timestamp)
                                            .size(11.5)
                                            .color(style::TEXT3),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if style::pointer(
                                            ui.button(
                                                egui::RichText::new("Delete")
                                                    .color(egui::Color32::from_rgb(200, 50, 50)),
                                            ),
                                        )
                                        .on_hover_text("Delete draft")
                                        .clicked()
                                        {
                                            delete_clicked = true;
                                            draft_to_delete = Some(draft.id);
                                        }
                                        if style::pointer(ui.button("Open")).clicked() {
                                            open_clicked = true;
                                            draft_to_open = Some(draft.clone());
                                        }
                                    },
                                );
                            });
                        });

                        // Make the whole row clickable to open the draft
                        if row_resp
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                            && !open_clicked
                            && !delete_clicked
                        {
                            draft_to_open = Some(draft.clone());
                        }

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

            if let Some(draft) = draft_to_open {
                let recipients = ui::compose_window::hydrate_recipients(&draft.to_field);
                for recipient in &recipients {
                    if let ui::compose_window::RecipientKind::Nip05 { identifier, .. } =
                        &recipient.kind
                    {
                        let _ = app.backend.request_nip05_resolution(identifier.clone());
                    }
                }
                let state = ui::compose_window::ComposeWindowState {
                    subject: draft.subject,
                    to_input: String::new(),
                    recipients,
                    content: draft.content,
                    parent_event_ids: draft.parent_events,
                    selected_account_pubkey: draft.selected_account,
                    selected_nip05: draft.selected_nip05,
                    minimized: false,
                    draft_id: Some(draft.id),
                    send_status: None,
                };
                app.state
                    .compose_window
                    .insert(egui::Id::new(rand::random::<u32>()), state);
            }

            if let Some(id) = draft_to_delete {
                app.delete_draft_and_refresh(id);
            }
        });
}
