use crate::style;
use eframe::egui::{self, Color32, RichText};
use hoot_backend::{ComposeMessageInput, DraftDto, DraftInput};
use tracing::{error, info};

#[derive(Debug, Clone)]
pub struct ComposeWindowState {
    pub subject: String,
    pub to_field: String,
    pub parent_event_ids: Vec<String>,
    pub content: String,
    pub selected_account_pubkey: Option<String>,
    pub selected_nip05: Option<String>,
    pub minimized: bool,
    pub draft_id: Option<i64>,
    pub send_status: Option<(String, Color32)>,
}

enum DraftAction {
    None,
    Save(DraftInput, Option<i64>),
    Delete(i64),
}

pub struct ComposeWindow {}

impl ComposeWindow {
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) -> bool {
        let screen_rect = ctx.screen_rect();
        let min_width = screen_rect.width().min(600.0);
        let min_height = screen_rect.height().min(400.0);
        let mut open = true;
        let mut should_close = false;
        let mut draft_action = DraftAction::None;

        egui::Window::new("New Message")
            .id(id)
            .default_size([min_width, min_height])
            .min_width(400.0)
            .min_height(300.0)
            .open(&mut open)
            .show(ctx, |ui| {
                let state = match app.state.compose_window.get_mut(&id) {
                    Some(state) => state,
                    None => return,
                };

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("To:");
                        ui.text_edit_singleline(&mut state.to_field);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Subject:");
                        ui.text_edit_singleline(&mut state.subject);
                    });
                    ui.separator();
                    ui.add_sized(
                        [ui.available_width(), ui.available_height() - 64.0],
                        egui::TextEdit::multiline(&mut state.content).hint_text("Write your message..."),
                    );

                    if let Some((message, color)) = &state.send_status {
                        ui.colored_label(*color, message);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Send").color(Color32::WHITE))
                                    .fill(style::ACCENT)
                                    .corner_radius(6),
                            )
                            .clicked()
                        {
                            let input = ComposeMessageInput {
                                subject: state.subject.clone(),
                                content: state.content.clone(),
                                to_field: state.to_field.clone(),
                                parent_event_ids: state.parent_event_ids.clone(),
                                selected_account_pubkey: state.selected_account_pubkey.clone(),
                                selected_nip05: state.selected_nip05.clone(),
                            };
                            match app.backend.send_message(input) {
                                Ok(result) if !result.pending_nip05.is_empty() => {
                                    state.send_status = Some((
                                        "Resolving NIP-05 addresses...".to_string(),
                                        style::TEXT_MUTED,
                                    ));
                                }
                                Ok(result) if !result.failed_nip05.is_empty() => {
                                    state.send_status = Some((
                                        format!("Could not resolve: {}", result.failed_nip05.join(", ")),
                                        Color32::RED,
                                    ));
                                }
                                Ok(result) => {
                                    info!("Sent {} message event(s)", result.sent_count);
                                    if let Some(draft_id) = state.draft_id {
                                        draft_action = DraftAction::Delete(draft_id);
                                    }
                                    should_close = true;
                                }
                                Err(e) => {
                                    error!("Failed to send message: {}", e);
                                    state.send_status = Some((format!("Failed to send: {}", e), Color32::RED));
                                }
                            }
                        }

                        if ui.button("Save Draft").clicked() {
                            draft_action = DraftAction::Save(
                                DraftInput {
                                    subject: state.subject.clone(),
                                    to_field: state.to_field.clone(),
                                    content: state.content.clone(),
                                    parent_events: state.parent_event_ids.clone(),
                                    selected_account: state.selected_account_pubkey.clone(),
                                },
                                state.draft_id,
                            );
                        }

                        egui::ComboBox::from_id_salt(id.with("account"))
                            .selected_text(
                                state
                                    .selected_account_pubkey
                                    .as_deref()
                                    .unwrap_or("Select account"),
                            )
                            .show_ui(ui, |ui| {
                                for account in &app.accounts {
                                    let selected = state.selected_account_pubkey.as_deref()
                                        == Some(account.pubkey_hex.as_str());
                                    if ui
                                        .selectable_label(selected, &account.pubkey_hex)
                                        .clicked()
                                    {
                                        state.selected_account_pubkey = Some(account.pubkey_hex.clone());
                                    }
                                }
                            });
                    });
                });
            });

        match draft_action {
            DraftAction::Save(input, Some(id_existing)) => {
                let draft = DraftDto {
                    id: id_existing,
                    subject: input.subject,
                    to_field: input.to_field,
                    content: input.content,
                    parent_events: input.parent_events,
                    selected_account: input.selected_account,
                    created_at: 0,
                    updated_at: 0,
                };
                match app.backend.update_draft(draft) {
                    Ok(()) => info!("Draft updated"),
                    Err(e) => error!("Failed to update draft: {}", e),
                }
                app.refresh_drafts();
            }
            DraftAction::Save(input, None) => {
                match app.backend.save_draft(input) {
                    Ok(new_id) => {
                        if let Some(state) = app.state.compose_window.get_mut(&id) {
                            state.draft_id = Some(new_id);
                        }
                        info!("Draft saved with id {}", new_id);
                    }
                    Err(e) => error!("Failed to save draft: {}", e),
                }
                app.refresh_drafts();
            }
            DraftAction::Delete(draft_id) => {
                if let Err(e) = app.backend.delete_draft(draft_id) {
                    error!("Failed to delete draft after send: {}", e);
                }
                app.refresh_drafts();
            }
            DraftAction::None => {}
        }

        open && !should_close
    }
}
