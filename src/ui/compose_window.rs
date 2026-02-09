use crate::mail_event::MailMessage;
use crate::relay::ClientMessage;
use crate::style;
use eframe::egui::{self, Color32, RichText};
use nostr::{EventId, Keys, PublicKey};
use tracing::{debug, error, info};

#[derive(Debug, Clone)]
pub struct ComposeWindowState {
    pub subject: String,
    pub to_field: String,
    pub parent_events: Vec<EventId>,
    pub content: String,
    pub selected_account: Option<Keys>,
    pub minimized: bool,
    pub draft_id: Option<i64>,
}

enum DraftAction {
    None,
    Save {
        subject: String,
        to_field: String,
        content: String,
        parent_events: Vec<String>,
        selected_account: Option<String>,
        existing_id: Option<i64>,
    },
    Delete(i64),
}

pub struct ComposeWindow {}

impl ComposeWindow {
    /// Returns `false` when the window has been closed and should be removed.
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) -> bool {
        let screen_rect = ctx.screen_rect();
        let min_width = screen_rect.width().min(600.0);
        let min_height = screen_rect.height().min(400.0);

        // Pre-resolve account display names before borrowing state,
        // since resolve_name borrows app immutably and state borrows app.state mutably.
        let account_options: Vec<(Keys, String)> = app
            .account_manager
            .loaded_keys
            .iter()
            .map(|k| {
                let pk_hex = k.public_key().to_hex();
                let name = app.resolve_name(&pk_hex).unwrap_or(pk_hex);
                (k.clone(), name)
            })
            .collect();

        let state = app
            .state
            .compose_window
            .get_mut(&id)
            .expect("no state found for id");

        let mut open = true;
        let mut draft_action = DraftAction::None;

        egui::Window::new("New Message")
            .id(id)
            .open(&mut open)
            .default_size([min_width, min_height])
            .min_width(300.0)
            .min_height(200.0)
            .default_pos([
                screen_rect.right() - min_width - 20.0,
                screen_rect.bottom() - min_height - 20.0,
            ])
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Header section
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("To:").color(style::TEXT_MUTED));
                        ui.add_sized(
                            [ui.available_width(), 24.0],
                            egui::TextEdit::singleline(&mut state.to_field)
                                .hint_text("Recipient public key"),
                        );
                    });

                    ui.add_space(2.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Subject:").color(style::TEXT_MUTED));
                        ui.add_sized(
                            [ui.available_width(), 24.0],
                            egui::TextEdit::singleline(&mut state.subject)
                                .hint_text("Message subject"),
                        );
                    });

                    ui.add_space(2.0);

                    // Toolbar
                    ui.horizontal(|ui| {
                        ui.style_mut().spacing.button_padding = egui::vec2(4.0, 4.0);
                        if ui.button("B").clicked() {}
                        if ui.button("I").clicked() {}
                        if ui.button("U").clicked() {}
                        ui.separator();
                        if ui.button("🔗").clicked() {}
                        if ui.button("📎").clicked() {}
                        if ui.button("😀").clicked() {}
                        ui.separator();
                        if ui.button("⌄").clicked() {}
                    });

                    // Message content
                    let available_height = ui.available_height() - 40.0; // Reserve space for bottom bar
                    egui::ScrollArea::vertical()
                        .max_height(available_height)
                        .show(ui, |ui| {
                            ui.add_sized(
                                [ui.available_width(), available_height - 20.0],
                                egui::TextEdit::multiline(&mut state.content),
                            );
                        });

                    // Bottom bar with account selector and send button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Send").color(Color32::WHITE))
                                    .fill(style::ACCENT)
                                    .rounding(6.0),
                            )
                            .clicked()
                        {
                            if state.selected_account.is_none() {
                                error!("No Account Selected!");
                                return;
                            }
                            // convert to field into PublicKey object
                            let to_field = state.to_field.clone();

                            let mut recipient_keys: Vec<PublicKey> = Vec::new();
                            for key_string in to_field.split_whitespace() {
                                use nostr::FromBech32;
                                match PublicKey::from_bech32(key_string) {
                                    Ok(k) => recipient_keys.push(k),
                                    Err(e) => debug!("could not parse public key as bech32: {}", e),
                                };

                                match PublicKey::from_hex(key_string) {
                                    Ok(k) => recipient_keys.push(k),
                                    Err(e) => debug!("could not parse public key as hex: {}", e),
                                };
                            }

                            let mut msg = MailMessage {
                                id: None,
                                created_at: None,
                                author: None,
                                to: recipient_keys,
                                cc: vec![],
                                bcc: vec![],
                                parent_events: Some(state.parent_events.clone()),
                                subject: state.subject.clone(),
                                content: state.content.clone(),
                            };
                            let events_to_send =
                                msg.to_events(&state.selected_account.clone().unwrap());

                            // send over wire
                            for event in events_to_send {
                                match serde_json::to_string(&ClientMessage::Event {
                                    event: event.1,
                                }) {
                                    Ok(v) => match app.relays.send(ewebsock::WsMessage::Text(v)) {
                                        Ok(r) => r,
                                        Err(e) => error!("could not send event to relays: {}", e),
                                    },
                                    Err(e) => error!("could not serialize event: {}", e),
                                };
                            }

                            // Delete the draft after sending
                            if let Some(draft_id) = state.draft_id {
                                draft_action = DraftAction::Delete(draft_id);
                            }
                        }

                        // Save Draft button
                        if ui
                            .add(egui::Button::new(RichText::new("Save Draft")).rounding(6.0))
                            .clicked()
                        {
                            let parent_event_strings: Vec<String> =
                                state.parent_events.iter().map(|e| e.to_hex()).collect();
                            let selected_account_str = state
                                .selected_account
                                .as_ref()
                                .map(|k| k.public_key().to_string());

                            draft_action = DraftAction::Save {
                                subject: state.subject.clone(),
                                to_field: state.to_field.clone(),
                                content: state.content.clone(),
                                parent_events: parent_event_strings,
                                selected_account: selected_account_str,
                                existing_id: state.draft_id,
                            };
                        }

                        // Account selector
                        let selected_text = state
                            .selected_account
                            .as_ref()
                            .and_then(|k| {
                                let pk = k.public_key().to_hex();
                                account_options
                                    .iter()
                                    .find(|(key, _)| key.public_key().to_hex() == pk)
                                    .map(|(_, name)| name.clone())
                            })
                            .unwrap_or_default();

                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_source("account_selector")
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    for (key, name) in &account_options {
                                        ui.selectable_value(
                                            &mut state.selected_account,
                                            Some(key.clone()),
                                            name,
                                        );
                                    }
                                });
                            ui.label("Send as:");
                        });
                    });
                });
            });

        // Apply deferred draft actions (outside the borrow of state)
        match draft_action {
            DraftAction::Save {
                subject,
                to_field,
                content,
                parent_events,
                selected_account,
                existing_id,
            } => {
                if let Some(draft_id) = existing_id {
                    match app.db.update_draft(
                        draft_id,
                        &subject,
                        &to_field,
                        &content,
                        &parent_events,
                        selected_account.as_deref(),
                    ) {
                        Ok(_) => info!("Draft updated"),
                        Err(e) => error!("Failed to update draft: {}", e),
                    }
                } else {
                    match app.db.save_draft(
                        &subject,
                        &to_field,
                        &content,
                        &parent_events,
                        selected_account.as_deref(),
                    ) {
                        Ok(new_id) => {
                            if let Some(state) = app.state.compose_window.get_mut(&id) {
                                state.draft_id = Some(new_id);
                            }
                            info!("Draft saved with id {}", new_id);
                        }
                        Err(e) => error!("Failed to save draft: {}", e),
                    }
                }
                app.refresh_drafts();
            }
            DraftAction::Delete(draft_id) => {
                if let Err(e) = app.db.delete_draft(draft_id) {
                    error!("Failed to delete draft after send: {}", e);
                }
                app.refresh_drafts();
            }
            DraftAction::None => {}
        }

        open
    }

    // Keep the original show method for backward compatibility
    pub fn show(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        Self::show_window(app, ui.ctx(), id);
    }
}
