use eframe::egui::{self, Color32, Frame, Margin, RichText, ScrollArea, Stroke};
use hoot_backend::{MailMessageDto, SenderStatusDto};
use tracing::error;

use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::ui;
use crate::Hoot;
use crate::Page;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    let events = match app
        .backend
        .get_thread(app.focused_post.clone(), app.show_trashed_post)
    {
        Ok(events) => events,
        Err(e) => {
            error!("Failed to load thread for {}: {}", app.focused_post, e);
            app.page = Page::Inbox;
            app.focused_post.clear();
            return;
        }
    };

    let own_pubkeys: Vec<String> = app
        .accounts
        .iter()
        .map(|account| account.pubkey_hex.clone())
        .collect();
    let thread_sender = events.iter().find_map(|ev| {
        ev.author_pubkey
            .clone()
            .filter(|pk| !own_pubkeys.contains(pk))
    });

    let sender_status = thread_sender.as_ref().and_then(|pubkey| {
        if app.contacts_manager.find_contact(pubkey).is_some() {
            return Some("contact");
        }
        match app.backend.get_sender_status(pubkey.clone()) {
            Ok(Some(SenderStatusDto::Allowed)) => Some("allowed"),
            Ok(Some(SenderStatusDto::Junked)) => Some("junked"),
            Ok(None) => Some("request"),
            Err(e) => {
                error!("Failed to read sender status: {}", e);
                None
            }
        }
    });

    let mut accept_sender: Option<(String, bool)> = None;
    let mut deny_sender: Option<String> = None;

    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            if let Some(pubkey) = &thread_sender {
                match sender_status.as_deref() {
                    Some("request") => {
                        sender_prompt(
                            app,
                            ui,
                            pubkey,
                            "Do you wish to receive correspondence from:",
                            "Accept",
                            "Deny",
                            &mut accept_sender,
                            &mut deny_sender,
                        );
                    }
                    Some("junked") => {
                        sender_prompt(
                            app,
                            ui,
                            pubkey,
                            "This sender is in your Junk:",
                            "Allow",
                            "Keep in Junk",
                            &mut accept_sender,
                            &mut deny_sender,
                        );
                    }
                    Some("allowed") => {
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Move to Junk").clicked() {
                                    deny_sender = Some(pubkey.to_string());
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                    _ => {}
                }
            }

            for ev in events {
                render_message(app, ui, ev);
            }
        });

    if let Some((pubkey, add_to_contacts)) = accept_sender {
        if let Err(e) = app
            .backend
            .set_sender_status(pubkey.clone(), SenderStatusDto::Allowed)
        {
            error!("Failed to accept sender: {}", e);
        } else {
            if add_to_contacts {
                if let Err(e) = app.backend.save_contact(pubkey.clone(), None) {
                    error!("Failed to add contact: {}", e);
                }
                app.refresh_contacts();
            }
            app.state.requests.add_to_contacts = false;
            app.refresh_requests();
            app.refresh_junk();
            app.refresh_inbox();
        }
    }

    if let Some(pubkey) = deny_sender {
        if let Err(e) = app.backend.set_sender_status(pubkey, SenderStatusDto::Junked) {
            error!("Failed to junk sender: {}", e);
        } else {
            app.state.requests.add_to_contacts = false;
            app.refresh_requests();
            app.refresh_junk();
            app.refresh_inbox();
            app.page = Page::Requests;
            app.focused_post.clear();
        }
    }
}

fn sender_prompt(
    app: &mut Hoot,
    ui: &mut egui::Ui,
    pubkey: &str,
    label_prefix: &str,
    accept_label: &str,
    deny_label: &str,
    accept_sender: &mut Option<(String, bool)>,
    deny_sender: &mut Option<String>,
) {
    Frame::none()
        .fill(style::ACCENT_LIGHT)
        .stroke(Stroke::new(1.0, style::ACCENT))
        .inner_margin(Margin::symmetric(16, 12))
        .corner_radius(8)
        .show(ui, |ui| {
            let name = app
                .resolve_name(pubkey)
                .unwrap_or_else(|| pubkey.to_string());
            ui.label(RichText::new(format!("{} {}", label_prefix, name)).strong());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(
                        &mut app.state.requests.add_to_contacts,
                        "Also add to Contacts",
                    );
                    if ui.button(accept_label).clicked() {
                        *accept_sender = Some((
                            pubkey.to_string(),
                            app.state.requests.add_to_contacts,
                        ));
                    }
                    if ui.button(deny_label).clicked() && deny_label != "Keep in Junk" {
                        *deny_sender = Some(pubkey.to_string());
                    }
                });
            });
        });
    ui.add_space(8.0);
}

fn render_message(app: &mut Hoot, ui: &mut egui::Ui, ev: MailMessageDto) {
    ui.add_space(8.0);
    Frame::none()
        .fill(style::CARD_BG)
        .stroke(Stroke::new(1.0, style::CARD_STROKE))
        .inner_margin(Margin::same(16))
        .corner_radius(8)
        .show(ui, |ui| {
            let event_id = match &ev.id {
                Some(id) => id.clone(),
                None => {
                    ui.label(
                        RichText::new("Error: malformed message (missing ID)")
                            .color(Color32::RED),
                    );
                    return;
                }
            };
            let author_pk = match &ev.author_pubkey {
                Some(pubkey) => pubkey.clone(),
                None => {
                    ui.label(
                        RichText::new("Error: malformed message (missing author)")
                            .color(Color32::RED),
                    );
                    return;
                }
            };

            if app.show_trashed_post {
                ui.label(
                    RichText::new("This message is in Trash")
                        .small()
                        .color(style::TEXT_MUTED),
                );
                ui.add_space(6.0);
            }
            ui.heading(&ev.subject);
            ui.add_space(4.0);

            egui::Grid::new(format!("email_metadata-{}", event_id))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("From").color(style::TEXT_MUTED));
                    render_pubkey_with_nip05(app, ui, &author_pk, ev.sender_nip05.as_deref());
                    ui.end_row();

                    ui.label(RichText::new("To").color(style::TEXT_MUTED));
                    ui.horizontal_wrapped(|ui| {
                        for (index, pk) in ev.to_pubkeys.iter().enumerate() {
                            if index > 0 {
                                ui.label(", ");
                            }
                            render_pubkey_with_nip05(app, ui, pk, None);
                        }
                    });
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("📎 Attach").clicked() {}
                if ui.button("📝 Edit").clicked() {}
                if ui.button("🗑️ Delete").clicked() {
                    let purge_after = chrono::Utc::now().timestamp() + 30 * 24 * 60 * 60;
                    if let Err(e) = app.backend.move_to_trash(event_id.clone(), purge_after) {
                        error!("Failed to move event to trash: {}", e);
                    } else {
                        if app.focused_post == event_id {
                            app.page = Page::Inbox;
                            app.focused_post.clear();
                            app.show_trashed_post = false;
                        }
                        app.refresh_inbox();
                        app.refresh_trash();
                    }
                }
                if ui.button("↩️ Reply").clicked() {
                    let mut parent_event_ids = ev.parent_event_ids.clone();
                    parent_event_ids.push(event_id.clone());
                    let state = ui::compose_window::ComposeWindowState {
                        subject: format!("Re: {}", ev.subject),
                        to_field: author_pk.clone(),
                        content: String::new(),
                        parent_event_ids,
                        selected_account_pubkey: app.active_account_pubkey.clone(),
                        selected_nip05: None,
                        minimized: false,
                        draft_id: None,
                        send_status: None,
                    };
                    app.state
                        .compose_window
                        .insert(egui::Id::new(rand::random::<u32>()), state);
                }
                if ui.button("↪️ Forward").clicked() {}
                if ui.button("⭐ Star").clicked() {}
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);
            ui.label(ev.content);
        });
}

fn render_pubkey_with_nip05(
    app: &mut Hoot,
    ui: &mut egui::Ui,
    pubkey: &str,
    expected_nip05: Option<&str>,
) {
    let _ = get_profile_metadata(app, pubkey.to_string());
    let name = app
        .resolve_name(pubkey)
        .unwrap_or_else(|| pubkey.to_string());
    let cached = app.backend.get_cached_nip05(pubkey.to_string()).ok().flatten();
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).strong());
        if let Some(entry) = cached {
            if expected_nip05.is_none() || expected_nip05 == Some(entry.nip05.as_str()) {
                let (icon, color, tooltip) = crate::ui::nip05_status::status_display(&entry);
                ui.colored_label(color, icon).on_hover_text(tooltip);
                ui.label(RichText::new(format!("({})", entry.nip05)).color(style::TEXT_MUTED));
                return;
            }
        }
        if let Some(nip05) = expected_nip05 {
            ui.colored_label(Color32::GRAY, "?")
                .on_hover_text("NIP-05 not yet verified");
            ui.label(RichText::new(format!("({})", nip05)).color(style::TEXT_MUTED));
        }
    });
}
