use crate::mail_event::MailMessage;
use crate::nip05::Nip05Resolution;
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
    pub selected_nip05: Option<String>,
    pub minimized: bool,
    pub draft_id: Option<i64>,
    /// Status message shown above the send button (e.g., resolution progress/errors)
    pub send_status: Option<(String, Color32)>,
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
                                .hint_text("Recipient (npub, hex, or nip05 like user@domain.com)"),
                        );
                    });

                    ui.add_space(2.0);

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
                                    .corner_radius(6),
                            )
                            .clicked()
                        {
                            if state.selected_account.is_none() {
                                error!("No Account Selected!");
                                return;
                            }
                            let to_field = state.to_field.clone();

                            let mut recipient_keys: Vec<PublicKey> = Vec::new();
                            let mut any_pending = false;
                            let mut failed_nip05s: Vec<String> = Vec::new();

                            for key_string in to_field.split_whitespace() {
                                use nostr::FromBech32;

                                // Try to parse as NIP-05 identifier first
                                if key_string.contains('@') {
                                    match app.nip05_resolver.get(key_string) {
                                        Some(Nip05Resolution::Resolved(pubkey_hex)) => {
                                            match PublicKey::from_hex(pubkey_hex) {
                                                Ok(k) => {
                                                    recipient_keys.push(k);
                                                    continue;
                                                }
                                                Err(e) => {
                                                    debug!("could not parse resolved NIP-05 pubkey: {}", e);
                                                    failed_nip05s.push(key_string.to_string());
                                                    continue;
                                                }
                                            }
                                        }
                                        Some(Nip05Resolution::Pending) => {
                                            any_pending = true;
                                            continue;
                                        }
                                        Some(Nip05Resolution::Failed) => {
                                            failed_nip05s.push(key_string.to_string());
                                            continue;
                                        }
                                        None => {
                                            // Not yet requested — enqueue and mark pending
                                            app.nip05_resolver.request(key_string.to_string());
                                            any_pending = true;
                                            continue;
                                        }
                                    }
                                }

                                match PublicKey::from_bech32(key_string) {
                                    Ok(k) => recipient_keys.push(k),
                                    Err(e) => debug!("could not parse public key as bech32: {}", e),
                                };

                                match PublicKey::from_hex(key_string) {
                                    Ok(k) => recipient_keys.push(k),
                                    Err(e) => debug!("could not parse public key as hex: {}", e),
                                };
                            }

                            if !failed_nip05s.is_empty() {
                                state.send_status = Some((
                                    format!("Could not resolve: {}", failed_nip05s.join(", ")),
                                    Color32::RED,
                                ));
                                return;
                            }

                            if any_pending {
                                state.send_status = Some((
                                    "Resolving NIP-05 addresses...".to_string(),
                                    style::TEXT_MUTED,
                                ));
                                return;
                            }

                            if recipient_keys.is_empty() {
                                state.send_status = Some((
                                    "No valid recipients".to_string(),
                                    Color32::RED,
                                ));
                                return;
                            }

                            // All recipients resolved — send
                            state.send_status = None;

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
                                sender_nip05: state.selected_nip05.clone(),
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

                        // Show send status message if any
                        if let Some((ref msg, color)) = state.send_status {
                            ui.label(RichText::new(msg).color(color).small());
                        }

                        // Save Draft button
                        if ui
                            .add(egui::Button::new(RichText::new("Save Draft")).corner_radius(6))
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
                            egui::ComboBox::from_id_salt("account_selector")
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    for (key, name) in &account_options {
                                        let selected = state
                                            .selected_account
                                            .as_ref()
                                            .map(|k| k.public_key() == key.public_key())
                                            .unwrap_or(false);

                                        // Show the key option (no NIP-05)
                                        let display_text = format!("{} (raw key)", name);
                                        if ui
                                            .selectable_label(
                                                selected && state.selected_nip05.is_none(),
                                                &display_text,
                                            )
                                            .clicked()
                                        {
                                            state.selected_account = Some(key.clone());
                                            state.selected_nip05 = None;
                                        }

                                        // Show NIP-05 options for this key (all NIP-05s, not just "own")
                                        let pk_hex = key.public_key().to_hex();
                                        if let Ok(nip05s) = app.db.get_nip05s_for_pubkey(&pk_hex) {
                                            for nip05_entry in nip05s {
                                                let nip05_selected = selected
                                                    && state.selected_nip05.as_ref()
                                                        == Some(&nip05_entry.nip05);
                                                let (status_icon, _, _) =
                                                    nip05_entry.status_display();
                                                let display_text = format!(
                                                    "{} {} ({})",
                                                    status_icon, nip05_entry.nip05, name
                                                );
                                                if ui
                                                    .selectable_label(nip05_selected, &display_text)
                                                    .clicked()
                                                {
                                                    state.selected_account = Some(key.clone());
                                                    state.selected_nip05 = Some(nip05_entry.nip05);
                                                }
                                            }
                                        }
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
}
