use eframe::egui::{self, Color32, Frame, Margin, RichText, ScrollArea, Stroke};
use nostr::{EventId, TagKind};
use tracing::error;

use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::ui;
use crate::Hoot;
use crate::Page;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    let events = if app.show_trashed_post {
        app.db.get_email_thread_including_trash(&app.focused_post)
    } else {
        app.db.get_email_thread(&app.focused_post)
    };
    let events = match events {
        Ok(events) => events,
        Err(e) => {
            error!("Failed to load thread for {}: {}", app.focused_post, e);
            app.page = Page::Inbox;
            app.focused_post.clear();
            return;
        }
    };

    let mut event_ids: Vec<String> = Vec::new();
    for ev in &events {
        if let Some(event_id) = ev.id.as_ref() {
            event_ids.push(event_id.to_hex());
        }
    }
    let trashed_ids = match app.db.get_trashed_event_ids(&event_ids) {
        Ok(ids) => ids,
        Err(e) => {
            error!("Failed to load trashed event ids: {}", e);
            Default::default()
        }
    };

    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for ev in events {
                ui.add_space(8.0);

                let event_id = ev.id;
                let author = ev.author;

                Frame::none()
                    .fill(style::CARD_BG)
                    .stroke(Stroke::new(1.0, style::CARD_STROKE))
                    .inner_margin(Margin::same(16.0))
                    .rounding(8.0)
                    .show(ui, |ui| {
                        if event_id.is_none() || author.is_none() {
                            ui.label(
                                RichText::new("Error: malformed message (missing ID or author)")
                                    .color(Color32::RED),
                            );
                            if !ev.subject.is_empty() {
                                ui.label(format!("Subject: {}", ev.subject));
                            }
                            return;
                        }
                        let event_id = event_id.unwrap();
                        let author = author.unwrap();

                        if trashed_ids.contains(&event_id.to_hex()) {
                            ui.label(
                                RichText::new("This message is in Trash")
                                    .small()
                                    .color(style::TEXT_MUTED),
                            );
                            ui.add_space(6.0);
                        }
                        ui.heading(&ev.subject);
                        ui.add_space(4.0);

                        // Metadata grid
                        let author_pk = author.to_string();
                        egui::Grid::new(format!("email_metadata-{}", event_id.to_hex()))
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("From").color(style::TEXT_MUTED),
                                );
                                let _ = get_profile_metadata(app, author_pk.clone());
                                let from_label = app
                                    .resolve_name(&author_pk)
                                    .unwrap_or_else(|| author_pk.clone());
                                ui.label(RichText::new(from_label).strong());
                                ui.end_row();

                                ui.label(RichText::new("To").color(style::TEXT_MUTED));
                                let to_labels: Vec<String> = ev
                                    .to
                                    .iter()
                                    .map(|pk| {
                                        let pk_str = pk.to_string();
                                        let _ =
                                            get_profile_metadata(app, pk_str.clone());
                                        app.resolve_name(&pk_str).unwrap_or(pk_str)
                                    })
                                    .collect();
                                ui.label(to_labels.join(", "));
                                ui.end_row();
                            });

                        ui.add_space(8.0);

                        // Action buttons
                        ui.horizontal(|ui| {
                            if ui.button("📎 Attach").clicked() {
                                // TODO: Handle attachment
                            }
                            if ui.button("📝 Edit").clicked() {
                                // TODO: Handle edit
                            }
                            if ui.button("🗑️ Delete").clicked() {
                                // TODO: broadcast NIP-09 EventDeletion to relays
                                let now = chrono::Utc::now().timestamp();
                                let purge_after = now + 30 * 24 * 60 * 60;
                                let event_id_hex = event_id.to_hex();
                                if let Err(e) = app
                                    .db
                                    .record_trash(&[event_id_hex.clone()], purge_after)
                                {
                                    error!("Failed to move event to trash: {}", e);
                                } else {
                                    app.events
                                        .retain(|ev| ev.id.to_string() != event_id_hex);
                                    if app.focused_post == event_id_hex {
                                        app.page = Page::Inbox;
                                        app.focused_post.clear();
                                        app.show_trashed_post = false;
                                    }
                                    match app.db.get_top_level_messages() {
                                        Ok(msgs) => app.table_entries = msgs,
                                        Err(e) => error!(
                                            "Could not fetch table entries to display from DB: {}",
                                            e
                                        ),
                                    }
                                    app.refresh_trash();
                                }
                            }
                            if ui.button("↩️ Reply").clicked() {
                                let mut parent_events: Vec<EventId> =
                                    ev.parent_events.unwrap_or(Vec::new());
                                parent_events.push(event_id);
                                let state = ui::compose_window::ComposeWindowState {
                                    subject: format!("Re: {}", ev.subject),
                                    to_field: author.to_string(),
                                    content: String::new(),
                                    parent_events,
                                    selected_account: None,
                                    minimized: false,
                                    draft_id: None,
                                };
                                app.state.compose_window.insert(
                                    egui::Id::new(rand::random::<u32>()),
                                    state,
                                );
                            }
                            if ui.button("↪️ Forward").clicked() {
                                // TODO: Handle forward
                            }
                            if ui.button("⭐ Star").clicked() {
                                // TODO: Handle star
                            }
                        });

                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(12.0);

                        // Message content
                        ui.label(ev.content);
                    });
            }
        });

    if let Some(event) = app
        .events
        .iter()
        .find(|e| e.id.to_string() == app.focused_post)
    {
        if let Ok(unwrapped) = app.account_manager.unwrap_gift_wrap(event) {
            let _subject = &unwrapped
                .rumor
                .tags
                .find(TagKind::Subject)
                .and_then(|s| s.content())
                .map(|c| c.to_string())
                .unwrap_or_else(|| "No Subject".to_string());
            // Message header section
        }
    }
}
