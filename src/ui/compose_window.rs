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
}

pub struct ComposeWindow {}

impl ComposeWindow {
    pub fn show_window(app: &mut crate::Hoot, ctx: &egui::Context, id: egui::Id) {
        let state = app
            .state
            .compose_window
            .get_mut(&id)
            .expect("no state found for id");
        egui::Window::new(&state.subject)
            .id(id)
            .show(ctx, |ui| {
                ui.label("Hello!");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("To:");
                        ui.text_edit_singleline(&mut state.to_field);
                    });

                    {
                        // god this is such a fucking mess
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

                        egui::ComboBox::from_label("Select Keys to Send With")
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
                    }

                    ui.horizontal(|ui| {
                        ui.label("Subject:");
                        ui.text_edit_singleline(&mut state.subject);
                    });
                    ui.label("Body:");
                    ui.text_edit_multiline(&mut state.content);

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
                });
            });
    }

    // Keep the original show method for backward compatibility
    pub fn show(app: &mut crate::Hoot, ui: &mut egui::Ui, id: egui::Id) {
        Self::show_window(app, ui.ctx(), id);
    }
}
