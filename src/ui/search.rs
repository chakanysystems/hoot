use crate::{style, Hoot, Page, TableEntry};
use eframe::egui::{self, Frame, Margin, RichText, Sense, Stroke};
use std::time::Instant;

const DEBOUNCE_MS: u64 = 150;
const MAX_SUGGESTIONS: usize = 8;
const ITEM_HEIGHT: f32 = 44.0;

pub fn render_global_search_bar(app: &mut Hoot, ctx: &egui::Context) {
    let mut committed = false;
    let mut text_edit_rect = egui::Rect::NOTHING;

    egui::TopBottomPanel::top("search_bar")
        .frame(
            Frame::new()
                .fill(ctx.style().visuals.panel_fill)
                .inner_margin(Margin::symmetric(12, 8))
                .stroke(Stroke::NONE),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new("🔍").size(14.0));

                let text_edit_width = ui.available_width() - 36.0;
                let text_edit = ui.add_sized(
                    [text_edit_width, 30.0],
                    egui::TextEdit::singleline(&mut app.state.search.query)
                        .hint_text("Search messages...")
                        .margin(egui::vec2(8.0, 4.0)),
                );

                text_edit_rect = text_edit.rect;

                if text_edit.changed() {
                    app.state.search.last_query_time = Some(Instant::now());
                    app.state.search.selected_suggestion = None;
                }

                // Enter commits to full-page results
                if text_edit.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !app.state.search.query.trim().is_empty()
                {
                    committed = true;
                }

                // Clear button
                if !app.state.search.query.is_empty() && ui.button("×").clicked() {
                    app.state.search.clear();
                    if app.page == Page::SearchResults {
                        app.page = Page::Inbox;
                    }
                }
            });
        });

    // Debounced search execution
    if let Some(last_time) = app.state.search.last_query_time {
        if last_time.elapsed().as_millis() >= DEBOUNCE_MS as u128 {
            app.state.search.last_query_time = None;
            let query = app.state.search.query.trim().to_string();
            if !query.is_empty() && query != app.state.search.last_executed_query {
                execute_search(app);
            } else if query.is_empty() {
                app.state.search.results.clear();
                app.state.search.last_executed_query.clear();
            }
        }
    }

    // Handle keyboard navigation in dropdown
    if app.state.search.has_suggestions() && app.page != Page::SearchResults {
        handle_suggestion_keyboard(app, ctx);
    }

    // Commit: either Enter was pressed, or a suggestion was selected via keyboard
    if committed {
        if let Some(idx) = app.state.search.selected_suggestion {
            if idx < app.state.search.results.len() {
                let event_id = app.state.search.results[idx].id.clone();
                navigate_to_post(app, &event_id);
                return;
            }
        }
        // No suggestion selected -- go to full-page results
        execute_search(app);
        app.page = Page::SearchResults;
    }

    // Render suggestion dropdown below the text edit widget
    if app.state.search.has_suggestions() && app.page != Page::SearchResults {
        render_suggestion_dropdown(app, ctx, text_edit_rect);
    }
}

/// Render the full-page search results view (called from the central panel).
pub fn render_search_results(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.add_space(8.0);

    let query = app.state.search.last_executed_query.clone();
    let count = app.state.search.results.len();

    ui.label(
        RichText::new(format!("{} results for \"{}\"", count, query)).color(style::TEXT_MUTED),
    );
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    if app.state.search.results.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No messages found")
                    .size(16.0)
                    .color(style::TEXT_MUTED),
            );
        });
    } else {
        let results = app.state.search.results.clone();
        super::inbox::render_message_table(app, ui, &results);
    }
}

fn handle_suggestion_keyboard(app: &mut Hoot, ctx: &egui::Context) {
    let count = app.state.search.results.len().min(MAX_SUGGESTIONS);
    if count == 0 {
        return;
    }

    let navigate = ctx.input(|i| {
        if i.key_pressed(egui::Key::ArrowDown) {
            Some(true)
        } else if i.key_pressed(egui::Key::ArrowUp) {
            Some(false)
        } else {
            None
        }
    });

    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if escape {
        app.state.search.clear();
        return;
    }

    if let Some(down) = navigate {
        let current = app.state.search.selected_suggestion;
        app.state.search.selected_suggestion = Some(match current {
            None => 0,
            Some(idx) if down => (idx + 1) % count,
            Some(0) => count - 1,
            Some(idx) => idx - 1,
        });
    }
}

fn render_suggestion_dropdown(app: &mut Hoot, ctx: &egui::Context, text_edit_rect: egui::Rect) {
    let results: Vec<TableEntry> = app
        .state
        .search
        .results
        .iter()
        .take(MAX_SUGGESTIONS)
        .cloned()
        .collect();
    let selected = app.state.search.selected_suggestion;

    let dropdown_width = text_edit_rect.width();
    // Small gap below the text edit so the dropdown never overlaps
    let dropdown_pos = egui::pos2(text_edit_rect.left(), text_edit_rect.bottom() + 2.0);

    let mut clicked_id: Option<String> = None;

    egui::Area::new(egui::Id::new("search_suggestions_dropdown"))
        .fixed_pos(dropdown_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            Frame::new()
                .fill(ui.visuals().window_fill)
                .stroke(Stroke::new(1.0, style::CARD_STROKE))
                .corner_radius(6)
                .inner_margin(Margin::symmetric(0, 4))
                .shadow(egui::epaint::Shadow {
                    color: egui::Color32::from_black_alpha(20),
                    spread: 0,
                    blur: 8,
                    offset: [0, 2],
                })
                .show(ui, |ui| {
                    ui.set_width(dropdown_width);

                    for (idx, entry) in results.iter().enumerate() {
                        let is_selected = selected == Some(idx);
                        if render_suggestion_item(ui, entry, is_selected, dropdown_width) {
                            clicked_id = Some(entry.id.clone());
                        }
                    }
                });
        });

    // Dismiss dropdown when clicking outside
    let dismiss = ctx.input(|i| {
        if i.pointer.primary_clicked() {
            if let Some(pos) = i.pointer.interact_pos() {
                let dropdown_height = (results.len() as f32 * ITEM_HEIGHT) + 8.0;
                let dropdown_rect = egui::Rect::from_min_size(
                    dropdown_pos,
                    egui::vec2(dropdown_width, dropdown_height),
                );
                return !dropdown_rect.contains(pos) && !text_edit_rect.contains(pos);
            }
        }
        false
    });
    if dismiss {
        app.state.search.clear();
    }

    if let Some(id) = clicked_id {
        navigate_to_post(app, &id);
    }
}

/// Renders a single suggestion item. Returns `true` if clicked.
fn render_suggestion_item(
    ui: &mut egui::Ui,
    entry: &TableEntry,
    is_selected: bool,
    width: f32,
) -> bool {
    let desired_size = egui::vec2(width, ITEM_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    // Background highlight
    if is_selected || response.hovered() {
        let fill = if is_selected {
            style::ACCENT_LIGHT
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        ui.painter()
            .rect_filled(rect.shrink(1.0), egui::CornerRadius::same(4), fill);
    }

    let content_rect = rect.shrink2(egui::vec2(10.0, 5.0));
    let text_color = ui.visuals().text_color();

    // Subject line
    let subject = if entry.subject.is_empty() {
        "(No Subject)".to_string()
    } else {
        truncate_text(&entry.subject, 50)
    };
    ui.painter().text(
        content_rect.left_top(),
        egui::Align2::LEFT_TOP,
        subject,
        egui::FontId::proportional(13.0),
        text_color,
    );

    // Content preview
    let preview = sanitize_preview(&entry.content);
    ui.painter().text(
        content_rect.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        truncate_text(&preview, 60),
        egui::FontId::proportional(11.0),
        style::TEXT_MUTED,
    );

    // Timestamp
    ui.painter().text(
        content_rect.right_top(),
        egui::Align2::RIGHT_TOP,
        style::format_timestamp(entry.created_at),
        egui::FontId::proportional(11.0),
        style::TEXT_MUTED,
    );

    response.clicked()
}

fn navigate_to_post(app: &mut Hoot, event_id: &str) {
    app.focused_post = event_id.to_string();
    app.page = Page::Post;
    app.show_trashed_post = false;
    app.state.search.clear();
}

fn execute_search(app: &mut Hoot) {
    let query = app.state.search.query.trim().to_string();
    if query.is_empty() {
        app.state.search.results.clear();
        return;
    }

    match app.db.search_messages(&query) {
        Ok(results) => {
            app.state.search.results = results;
            app.state.search.last_executed_query = query;
        }
        Err(e) => {
            tracing::error!("Search failed: {}", e);
            app.state.search.results.clear();
        }
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

/// Collapse all whitespace (newlines, tabs, runs of spaces) into single spaces and trim.
fn sanitize_preview(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
