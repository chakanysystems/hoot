use eframe::egui::{
    self, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke,
};
use hoot_backend::{npub_string, MailMessageDto, SenderStatusDto};
use tracing::error;

use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::ui;
use crate::Hoot;
use crate::Page;

fn short_pubkey(pubkey: &str) -> String {
    let npub = npub_string(pubkey).unwrap_or_else(|| pubkey.to_string());
    if npub.len() > 30 {
        format!("{}…", &npub[..30])
    } else {
        npub
    }
}

fn build_reply_state(
    app: &Hoot,
    last_event_id: Option<String>,
    last_event_author: Option<String>,
    last_event_parent_events: Vec<String>,
    last_event_subject: &str,
) -> Option<ui::compose_window::ComposeWindowState> {
    let event_id = last_event_id?;
    let author = last_event_author?;
    let mut parent_event_ids = last_event_parent_events;
    parent_event_ids.push(event_id);

    Some(ui::compose_window::ComposeWindowState {
        subject: format!("Re: {}", last_event_subject),
        to_input: String::new(),
        recipients: ui::compose_window::hydrate_recipients(&author),
        content: String::new(),
        parent_event_ids,
        selected_account_pubkey: app.active_account_pubkey.clone(),
        selected_nip05: None,
        minimized: false,
        draft_id: None,
        send_status: None,
    })
}

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
            app.state.thread_view.reply_post_id.clear();
            app.state.thread_view.reply_compose = None;
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

    let first_event = events.first().cloned();
    let first_subject = first_event
        .as_ref()
        .map(|ev| ev.subject.clone())
        .unwrap_or_default();

    let last_event_id = events.last().and_then(|ev| ev.id.clone());
    let last_event_author = events.last().and_then(|ev| ev.author_pubkey.clone());
    let last_event_parent_events = events
        .last()
        .map(|ev| ev.parent_event_ids.clone())
        .unwrap_or_default();
    let last_event_subject = events
        .last()
        .map(|ev| ev.subject.clone())
        .unwrap_or_default();

    let mut accept_sender: Option<(String, bool)> = None;
    let mut deny_sender: Option<String> = None;
    let mut trash_event_id: Option<String> = None;
    let mut focus_reply_editor = false;

    Frame::new()
        .fill(style::SURFACE)
        .inner_margin(Margin::symmetric(16, 8))
        .show(ui, |ui| {
            let panel_rect = ui.max_rect();

            ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                if style::pointer(
                    ui.add(
                        egui::Button::new(RichText::new("← Inbox").size(13.5).color(style::ACCENT))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(6)),
                    ),
                )
                .clicked()
                {
                    app.page = Page::Inbox;
                    app.focused_post.clear();
                    app.show_trashed_post = false;
                    app.state.thread_view.reply_post_id.clear();
                    app.state.thread_view.reply_compose = None;
                }

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    let (reply_clicked, delete_clicked) = ui
                        .allocate_ui_with_layout(
                            egui::vec2(108.0, 32.0),
                            Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let r = ui.max_rect();
                                ui.painter()
                                    .rect_filled(r, CornerRadius::same(8), style::SURFACE2);
                                ui.painter().rect_stroke(
                                    r,
                                    CornerRadius::same(8),
                                    Stroke::new(1.0, style::border_strong()),
                                    eframe::epaint::StrokeKind::Middle,
                                );

                                let reply_clicked = style::pointer(
                                    ui.add(
                                        egui::Button::new(
                                            RichText::new("↩").size(14.0).color(style::TEXT2),
                                        )
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::NONE)
                                        .corner_radius(CornerRadius::same(6))
                                        .min_size(egui::vec2(30.0, 30.0)),
                                    ),
                                )
                                .on_hover_text("Reply")
                                .clicked();

                                style::pointer(
                                    ui.add(
                                        egui::Button::new(
                                            RichText::new("📁").size(14.0).color(style::TEXT2),
                                        )
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::NONE)
                                        .corner_radius(CornerRadius::same(6))
                                        .min_size(egui::vec2(30.0, 30.0)),
                                    ),
                                )
                                .on_hover_text("Archive");

                                let (div_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(9.0, 30.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().line_segment(
                                    [div_rect.center_top(), div_rect.center_bottom()],
                                    Stroke::new(1.0, style::border_strong()),
                                );

                                let delete_clicked = style::pointer(
                                    ui.add(
                                        egui::Button::new(
                                            RichText::new("🗑").size(14.0).color(style::TEXT2),
                                        )
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::NONE)
                                        .corner_radius(CornerRadius::same(6))
                                        .min_size(egui::vec2(30.0, 30.0)),
                                    ),
                                )
                                .on_hover_text("Delete")
                                .clicked();

                                (reply_clicked, delete_clicked)
                            },
                        )
                        .inner;

                    if reply_clicked {
                        focus_reply_editor = true;
                    }
                    if delete_clicked && !app.focused_post.is_empty() {
                        let purge_after = chrono::Utc::now().timestamp() + 30 * 24 * 60 * 60;
                        if let Err(e) = app
                            .backend
                            .move_to_trash(app.focused_post.clone(), purge_after)
                        {
                            error!("Failed to move event to trash: {}", e);
                        } else {
                            trash_event_id = Some(app.focused_post.clone());
                        }
                    }
                });
            });

            let bottom_y = panel_rect.bottom();
            ui.painter().line_segment(
                [
                    egui::pos2(panel_rect.left(), bottom_y),
                    egui::pos2(panel_rect.right(), bottom_y),
                ],
                Stroke::new(1.0, style::border()),
            );
        });

    if app.page != Page::Post || app.focused_post.is_empty() {
        return;
    }

    let reply_state_needs_reset = app.state.thread_view.reply_post_id != app.focused_post
        || app.state.thread_view.reply_compose.is_none();
    let mut reply_state = if reply_state_needs_reset {
        build_reply_state(
            app,
            last_event_id,
            last_event_author,
            last_event_parent_events,
            &last_event_subject,
        )
    } else {
        app.state.thread_view.reply_compose.take()
    };

    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.add_space(32.0);

            let available = ui.available_width();
            let content_width = available.min(640.0);
            let h_pad = ((available - content_width) / 2.0).max(0.0).min(120.0) as i8;

            Frame::new()
                .inner_margin(Margin {
                    left: h_pad,
                    right: h_pad,
                    top: 0,
                    bottom: 0,
                })
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
                                    true,
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
                                    true,
                                    &mut accept_sender,
                                    &mut deny_sender,
                                );
                            }
                            Some("allowed") => {
                                ui.horizontal(|ui| {
                                    ui.with_layout(
                                        Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if style::pointer(ui.small_button("Move to Junk"))
                                                .clicked()
                                            {
                                                deny_sender = Some(pubkey.clone());
                                            }
                                        },
                                    );
                                });
                                ui.add_space(8.0);
                            }
                            _ => {}
                        }
                    }

                    if let Some(ev) = &first_event {
                        render_thread_header(app, ui, ev, &first_subject);
                        ui.add_space(20.0);
                        ui.separator();
                        ui.add_space(20.0);
                    }

                    for (idx, ev) in events.iter().enumerate() {
                        if idx > 0 {
                            ui.add_space(18.0);
                            ui.separator();
                            ui.add_space(18.0);
                        }

                        if app.show_trashed_post {
                            ui.label(
                                RichText::new("This message is in Trash")
                                    .small()
                                    .color(style::TEXT2),
                            );
                            ui.add_space(8.0);
                        }

                        if idx > 0 {
                            render_message_header(app, ui, ev);
                            ui.add_space(10.0);
                        }

                        for paragraph in ev.content.split('\n').filter(|p| !p.trim().is_empty()) {
                            ui.label(
                                RichText::new(paragraph.trim())
                                    .size(15.0)
                                    .color(style::TEXT),
                            );
                            ui.add_space(8.0);
                        }
                    }

                    if let Some(state) = reply_state.as_mut() {
                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("REPLY")
                                .size(11.0)
                                .color(style::TEXT3)
                                .strong(),
                        );
                        ui.add_space(8.0);

                        Frame::new()
                            .fill(style::SURFACE)
                            .stroke(Stroke::new(1.0, style::border_strong()))
                            .corner_radius(CornerRadius::same(12))
                            .show(ui, |ui| {
                                let reply_id =
                                    egui::Id::new("thread_reply_compose").with(&app.focused_post);
                                let panel_output = ui::compose_window::render_panel(
                                    app,
                                    ui,
                                    reply_id,
                                    "Reply",
                                    false,
                                    focus_reply_editor,
                                    state,
                                );
                                if panel_output.sent_message {
                                    state.content.clear();
                                    state.draft_id = None;
                                    app.refresh_inbox();
                                }
                            });
                    }

                    ui.add_space(32.0);
                });
        });

    app.state.thread_view.reply_post_id = app.focused_post.clone();
    app.state.thread_view.reply_compose = reply_state;

    if trash_event_id.is_some() {
        app.page = Page::Inbox;
        app.focused_post.clear();
        app.show_trashed_post = false;
        app.state.thread_view.reply_post_id.clear();
        app.state.thread_view.reply_compose = None;
        app.refresh_inbox();
        app.refresh_trash();
    }

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
        if let Err(e) = app
            .backend
            .set_sender_status(pubkey, SenderStatusDto::Junked)
        {
            error!("Failed to junk sender: {}", e);
        } else {
            app.state.requests.add_to_contacts = false;
            app.refresh_requests();
            app.refresh_junk();
            app.refresh_inbox();
            app.page = Page::Requests;
            app.focused_post.clear();
            app.state.thread_view.reply_post_id.clear();
            app.state.thread_view.reply_compose = None;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sender_prompt(
    app: &mut Hoot,
    ui: &mut egui::Ui,
    pubkey: &str,
    label_prefix: &str,
    accept_label: &str,
    deny_label: &str,
    show_checkbox: bool,
    accept_sender: &mut Option<(String, bool)>,
    deny_sender: &mut Option<String>,
) {
    Frame::new()
        .fill(style::accent_soft())
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
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if show_checkbox {
                        style::pointer(ui.checkbox(
                            &mut app.state.requests.add_to_contacts,
                            "Also add to Contacts",
                        ));
                    }
                    if style::pointer(ui.button(accept_label)).clicked() {
                        *accept_sender =
                            Some((pubkey.to_string(), app.state.requests.add_to_contacts));
                    }
                    if deny_label != "Keep in Junk"
                        && style::pointer(ui.button(deny_label)).clicked()
                    {
                        *deny_sender = Some(pubkey.to_string());
                    }
                });
            });
        });
    ui.add_space(12.0);
}

fn render_thread_header(
    app: &mut Hoot,
    ui: &mut egui::Ui,
    ev: &MailMessageDto,
    first_subject: &str,
) {
    let subject = if first_subject.is_empty() {
        "(No Subject)".to_string()
    } else {
        first_subject.to_string()
    };

    let author_pk = ev.author_pubkey.clone().unwrap_or_default();
    let _ = get_profile_metadata(app, author_pk.clone());
    let author_name = app
        .resolve_name(&author_pk)
        .unwrap_or_else(|| author_pk.clone());
    let initials = style::initials_for_name(&author_name);
    let timestamp = ev
        .created_at
        .map(style::format_timestamp)
        .unwrap_or_default();

    Frame::new().show(ui, |ui| {
        ui.label(
            RichText::new(subject)
                .size(17.0)
                .strong()
                .color(style::TEXT),
        );
        ui.add_space(16.0);

        ui.horizontal(|ui| {
            let avatar_size = egui::vec2(36.0, 36.0);
            let (avatar_rect, _) = ui.allocate_exact_size(avatar_size, egui::Sense::hover());
            style::paint_avatar(ui.painter(), avatar_rect, &initials, None);

            ui.add_space(10.0);

            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&author_name)
                        .size(13.5)
                        .color(style::TEXT)
                        .strong(),
                );

                if !author_pk.is_empty() {
                    ui.label(
                        RichText::new(short_pubkey(&author_pk))
                            .size(11.5)
                            .color(style::TEXT3),
                    );
                }
            });

            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if !timestamp.is_empty() {
                    ui.label(RichText::new(timestamp).size(12.0).color(style::TEXT3));
                }
            });
        });
    });
}

fn render_message_header(app: &mut Hoot, ui: &mut egui::Ui, ev: &MailMessageDto) {
    if let Some(author_pk) = &ev.author_pubkey {
        let _ = get_profile_metadata(app, author_pk.clone());
        let author_name = app
            .resolve_name(author_pk)
            .unwrap_or_else(|| author_pk.clone());
        let initials = style::initials_for_name(&author_name);
        let timestamp = ev
            .created_at
            .map(style::format_timestamp)
            .unwrap_or_default();
        let subject = if ev.subject.is_empty() {
            "(No Subject)".to_string()
        } else {
            ev.subject.clone()
        };
        ui.label(
            RichText::new(subject)
                .size(17.0)
                .strong()
                .color(style::TEXT),
        );
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let (avatar_rect, _) =
                ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
            style::paint_avatar(ui.painter(), avatar_rect, &initials, None);
            ui.add_space(8.0);
            ui.label(
                RichText::new(&author_name)
                    .size(13.0)
                    .color(style::TEXT)
                    .strong(),
            );
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if !timestamp.is_empty() {
                    ui.label(RichText::new(timestamp).size(12.0).color(style::TEXT3));
                }
            });
        });
    }
}
