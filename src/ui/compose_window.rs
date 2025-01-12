use crate::mail_event::MailMessage;
use crate::relay::ClientMessage;
use eframe::egui::{self, RichText};
use nostr::{Keys, PublicKey};
use tracing::{debug, error, info};

#[derive(Debug, Clone)]
pub struct ComposeWindowState {
    pub subject: String,
    pub to_field: String,
    pub content: String,
    pub selected_account: Option<Keys>,
    pub minimized: bool,
}

pub struct ComposeWindow {}

impl ComposeWindow {
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) {
        let screen_rect = ctx.screen_rect();
        let min_width = screen_rect.width().min(600.0);
        let min_height = screen_rect.height().min(400.0);
        
        // First collect all window IDs and their minimized state
        let window_states: Vec<_> = app.state.compose_window.iter()
            .map(|(id, state)| (*id, state.minimized))
            .collect();
        
        let mut should_close = false;
        
        let state = app
            .state
            .compose_window
            .get_mut(&id)
            .expect("no state found for id");

        if state.minimized {
            // Count how many minimized windows are before this one
            let minimized_index = window_states.iter()
                .filter(|(other_id, is_minimized)| {
                    *is_minimized && other_id.value() < id.value()
                })
                .count();
            
            let minimized_height = 32.0;
            let minimized_width = 200.0;
            let minimized_spacing = 2.0;
            let stack_on_left = screen_rect.width() < min_width + minimized_width + 40.0;
            
            let window_y_offset = -(minimized_height + minimized_spacing) * (minimized_index as f32 + 1.0);
            
            let (anchor, anchor_pos) = if stack_on_left {
                (egui::Align2::LEFT_BOTTOM, [20.0, window_y_offset])
            } else {
                (egui::Align2::RIGHT_BOTTOM, [-20.0, window_y_offset])
            };
            
            egui::Window::new("📧")
                .id(id)
                .fixed_size([minimized_width, minimized_height])
                .anchor(anchor, anchor_pos)
                .title_bar(true)
                .frame(egui::Frame::window(&ctx.style()).multiply_with_opacity(0.95))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("📧").clicked() {
                            state.minimized = false;
                        }
                        let subject = state.subject.as_str();
                        let display_text = if subject.is_empty() {
                            "New Message"
                        } else {
                            subject.get(0..20).unwrap_or(subject)
                        };
                        ui.label(RichText::new(display_text).size(11.0));
                    });
                });
            return;
        } else {
            egui::Window::new("New Message")
                .id(id)
                .default_size([min_width, min_height])
                .min_width(300.0)
                .min_height(200.0)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-20.0, -20.0])
                .default_pos([screen_rect.right() - min_width - 20.0, screen_rect.bottom() - min_height - 20.0])
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        // Window controls at the top
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("❌").clicked() {
                                    should_close = true;
                                }
                                if ui.button("🗕").clicked() {
                                    state.minimized = true;
                                }
                                ui.add_space(ui.available_width() - 50.0); // Push buttons to the right
                            });
                        });

                        // Header section
                        ui.horizontal(|ui| {
                            ui.label("To:");
                            ui.add_sized(
                                [ui.available_width(), 24.0],
                                egui::TextEdit::singleline(&mut state.to_field)
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("Subject:");
                            ui.add_sized(
                                [ui.available_width(), 24.0],
                                egui::TextEdit::singleline(&mut state.subject)
                            );
                        });

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
                                    egui::TextEdit::multiline(&mut state.content)
                                );
                            });

                        // Bottom bar with account selector and send button
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Send").clicked() {
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
                                    to: recipient_keys,
                                    cc: vec![],
                                    bcc: vec![],
                                    subject: state.subject.clone(),
                                    content: state.content.clone(),
                                };
                                let events_to_send =
                                    msg.to_events(&state.selected_account.clone().unwrap());

                                info!("new events! {:?}", events_to_send);
                                // send over wire
                                for event in events_to_send {
                                    match serde_json::to_string(&ClientMessage::Event { event: event.1 }) {
                                        Ok(v) => match app.relays.send(ewebsock::WsMessage::Text(v)) {
                                            Ok(r) => r,
                                            Err(e) => error!("could not send event to relays: {}", e),
                                        },
                                        Err(e) => error!("could not serialize event: {}", e),
                                    };
                                }
                            }

                            // Account selector
                            let accounts = app.account_manager.loaded_keys.clone();
                            use nostr::ToBech32;
                            let mut formatted_key = String::new();
                            if state.selected_account.is_some() {
                                formatted_key = state
                                    .selected_account
                                    .clone()
                                    .unwrap()
                                    .public_key()
                                    .to_bech32()
                                    .unwrap();
                            }

                            egui::ComboBox::from_id_source("account_selector")
                                .selected_text(format!("{}", formatted_key))
                                .show_ui(ui, |ui| {
                                    for key in accounts {
                                        ui.selectable_value(
                                            &mut state.selected_account,
                                            Some(key.clone()),
                                            key.public_key().to_bech32().unwrap(),
                                        );
                                    }
                                });
                        });
                    });
                });
        }
        
        if should_close {
            app.state.compose_window.remove(&id);
        }
    }

    // Keep the original show method for backward compatibility
    pub fn show(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        Self::show_window(app, ui.ctx(), id);
    }
}
