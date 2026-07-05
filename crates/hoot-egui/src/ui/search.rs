use crate::{style, Hoot, Page, TableEntry};
use eframe::egui::{self, Frame, Margin, RichText, Sense, Stroke, StrokeKind};
use std::time::Instant;

const DEBOUNCE_MS: u64 = 150;
const MAX_SUGGESTIONS: usize = 8;
const ITEM_HEIGHT: f32 = 44.0;

pub fn render_global_search_bar(app: &mut Hoot, ctx: &egui::Context) {
    let mut committed = false;
    let mut text_edit_rect = egui::Rect::NOTHING;

    egui::TopBottomPanel::top("search_bar")
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(style::SURFACE)
                .inner_margin(Margin::same(0))
                .stroke(Stroke::NONE),
        )
        .show(ctx, |ui| {
            // Outer horizontal row with 10px padding all sides
            let panel_rect = ui.available_rect_before_wrap();

            Frame::new()
                .fill(style::SURFACE)
                .inner_margin(Margin {
                    left: 10,
                    right: 10,
                    top: 10,
                    bottom: 10,
                })
                .stroke(Stroke::NONE)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

                        // ── Search input (left, flex) ─────────────────────────────
                        let button_area = 2.0 * 30.0 + 8.0; // two 30px buttons + 8px gap between them
                        let spacing = ui.spacing().item_spacing.x;
                        let available = ui.available_width();
                        let input_width = available - button_area - spacing;

                        // Allocate space for the search input frame
                        let (input_outer_rect, _) =
                            ui.allocate_exact_size(egui::vec2(input_width, 30.0), Sense::hover());

                        // Check if the text edit has focus (we need to query this after allocating)
                        let search_id = egui::Id::new("search_text_edit");
                        let has_focus = ctx.memory(|m| m.has_focus(search_id));

                        // Draw background frame for the search input
                        let input_bg = if has_focus {
                            style::SURFACE
                        } else {
                            style::SURFACE2
                        };
                        let input_border = if has_focus {
                            style::ACCENT
                        } else {
                            style::border_strong()
                        };

                        let painter = ui.painter();
                        painter.rect_filled(
                            input_outer_rect,
                            egui::CornerRadius::same(8),
                            input_bg,
                        );
                        painter.rect_stroke(
                            input_outer_rect,
                            egui::CornerRadius::same(8),
                            Stroke::new(1.0, input_border),
                            StrokeKind::Inside,
                        );

                        // If focused, draw a soft accent outer glow approximation
                        if has_focus {
                            painter.rect_stroke(
                                input_outer_rect.expand(1.5),
                                egui::CornerRadius::same(9),
                                Stroke::new(2.0, style::accent_soft()),
                                StrokeKind::Outside,
                            );
                        }

                        // Draw the 🔍 icon inside, ~10px from left
                        let icon_pos =
                            egui::pos2(input_outer_rect.left() + 11.0, input_outer_rect.center().y);
                        painter.text(
                            icon_pos,
                            egui::Align2::LEFT_CENTER,
                            "🔍",
                            egui::FontId::proportional(13.0),
                            style::TEXT3,
                        );

                        // Place the TextEdit inside the frame with left padding for the icon
                        // inner area: 32px left offset, 7px top/bottom, 12px right
                        let inner_rect = egui::Rect::from_min_max(
                            egui::pos2(
                                input_outer_rect.left() + 32.0,
                                input_outer_rect.top() + 7.0,
                            ),
                            egui::pos2(
                                input_outer_rect.right() - 12.0,
                                input_outer_rect.bottom() - 7.0,
                            ),
                        );

                        let mut child_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(inner_rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );

                        let text_edit = child_ui.add_sized(
                            inner_rect.size(),
                            egui::TextEdit::singleline(&mut app.state.search.query)
                                .id(search_id)
                                .hint_text(
                                    egui::RichText::new("Search messages\u{2026}")
                                        .color(style::TEXT3)
                                        .size(13.0),
                                )
                                .frame(false)
                                .font(egui::FontId::proportional(13.0))
                                .text_color(style::TEXT)
                                .margin(egui::vec2(0.0, 0.0)),
                        );

                        text_edit_rect = input_outer_rect;

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

                        // ── Right-side icon buttons (30×30 each) ─────────────────
                        // Filter button
                        render_icon_button(ui, "\u{2261}", "filter_button"); // ≡

                        // Refresh / sync button
                        if render_icon_button(ui, "\u{27F3}", "refresh_button") {
                            // ⟳ - trigger refresh for the current page
                            match app.page {
                                Page::Inbox => app.refresh_inbox(),
                                Page::Drafts => app.refresh_drafts(),
                                Page::Trash => app.refresh_trash(),
                                Page::Requests => app.refresh_requests(),
                                Page::Junk => app.refresh_junk(),
                                _ => {}
                            }
                        }
                    });
                });

            // ── Bottom border line ────────────────────────────────────────────
            let bottom_y = ui.min_rect().bottom();
            let painter = ui.painter();
            painter.line_segment(
                [
                    egui::pos2(panel_rect.left(), bottom_y),
                    egui::pos2(panel_rect.right(), bottom_y),
                ],
                Stroke::new(1.0, style::border()),
            );
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

/// Render a 30×30 icon button with styled background and border.
/// Returns true if clicked.
fn render_icon_button(ui: &mut egui::Ui, icon: &str, id_str: &str) -> bool {
    let btn_size = egui::vec2(30.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(btn_size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    let is_hovered = response.hovered();
    let is_pressed = response.is_pointer_button_down_on();

    let bg = style::SURFACE2;
    let border_color = style::border_strong();
    let text_color = if is_hovered {
        style::TEXT
    } else {
        style::TEXT2
    };

    let painter = ui.painter();

    // Background
    let fill = if is_pressed { style::SURFACE } else { bg };
    painter.rect_filled(rect, egui::CornerRadius::same(7), fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(7),
        Stroke::new(1.0, border_color),
        StrokeKind::Inside,
    );

    // Icon centered
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(15.0),
        text_color,
    );

    // Suppress "unused variable" warning — id_str used for tooltip or future keying
    let _ = id_str;

    response.clicked()
}

/// Render the full-page search results view (called from the central panel).
pub fn render_search_results(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.add_space(8.0);

    let query = app.state.search.last_executed_query.clone();
    let count = app.state.search.results.len();

    ui.label(RichText::new(format!("{} results for \"{}\"", count, query)).color(style::TEXT2));
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    if app.state.search.results.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No messages found")
                    .size(16.0)
                    .color(style::TEXT2),
            );
        });
    } else {
        let results = app.state.search.results.clone();
        super::inbox::render_message_list(app, ui, &results);
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
                .stroke(Stroke::new(1.0, style::border_strong()))
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
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    // Background highlight
    if is_selected || response.hovered() {
        let fill = if is_selected {
            style::accent_soft()
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
        style::TEXT2,
    );

    // Timestamp
    ui.painter().text(
        content_rect.right_top(),
        egui::Align2::RIGHT_TOP,
        style::format_timestamp(entry.created_at),
        egui::FontId::proportional(11.0),
        style::TEXT2,
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

    match app.backend.search_messages(query.clone()) {
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
