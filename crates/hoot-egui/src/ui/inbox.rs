use eframe::egui::{
    self, CornerRadius, FontId, Frame, Margin, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

use crate::profile_metadata::get_profile_metadata;
use crate::style;
use crate::Hoot;
use crate::Page;
use crate::TableEntry;
use hoot_backend::RelayConnectionStatus;

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    // Reserve the status bar at the bottom using a BottomPanel so it stays fixed
    egui::TopBottomPanel::bottom("inbox_status_bar")
        .exact_height(style::STATUS_BAR_HEIGHT)
        .frame(egui::Frame::new().fill(style::SURFACE))
        .show_inside(ui, |ui| {
            render_status_bar(app, ui);
        });

    // Main content area with the message list
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(style::BG))
        .show_inside(ui, |ui| {
            if app.table_entries.is_empty() {
                // Empty state: centered text
                let available = ui.available_size();
                ui.add_space(available.y / 2.0 - 12.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No messages yet.")
                            .size(14.0)
                            .color(style::TEXT2),
                    );
                });
            } else {
                let entries = app.table_entries.to_vec();
                render_message_list(app, ui, &entries);
            }
        });
}

/// Render the custom message list with painter-based rows.
pub fn render_message_list(app: &mut Hoot, ui: &mut egui::Ui, entries: &[TableEntry]) {
    // HTML: .message-list { padding: 8px } — gives rows breathing room so
    // rounded-corner hover states don't clip against the container edges.
    Frame::new()
        .inner_margin(Margin {
            left: 8,
            right: 8,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let available_width = ui.available_width();

                    for entry in entries {
                        render_message_row(app, ui, entry, available_width);
                        ui.add_space(2.0);
                    }
                });
        });
}

/// Render a single message row using custom painting.
fn render_message_row(app: &mut Hoot, ui: &mut egui::Ui, entry: &TableEntry, available_width: f32) {
    let row_height = style::INBOX_ROW_HEIGHT;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(available_width, row_height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if !ui.is_rect_visible(rect) {
        return;
    }

    // For now, treat all messages as read (TableEntry has no read field yet).
    // TODO: wire up read state from DB once tracking is implemented.
    let is_unread = false;

    let painter = ui.painter();

    // Background: highlight on hover
    let bg_color = if response.hovered() {
        style::SURFACE
    } else {
        style::BG
    };
    painter.rect_filled(rect, CornerRadius::same(10), bg_color);

    // Hover shadow — paint a subtle border on hover to simulate shadow
    if response.hovered() {
        painter.rect_stroke(
            rect,
            CornerRadius::same(10),
            Stroke::new(1.0, style::border_strong()),
            StrokeKind::Outside,
        );
    }

    // Layout constants
    let pad_x: f32 = 14.0;
    let pad_y: f32 = 12.0;
    let unread_dot_size: f32 = 5.0;
    let unread_dot_margin: f32 = 3.0; // 3px from left edge
    let avatar_size: f32 = style::AVATAR_SIZE; // 34px
    let avatar_gap: f32 = 12.0;

    // Left edge start for content (after unread dot space)
    let content_left = rect.left() + pad_x;

    // Unread indicator dot
    if is_unread {
        let dot_x = rect.left() + unread_dot_margin + unread_dot_size / 2.0;
        let dot_y = rect.center().y;
        painter.circle_filled(
            Pos2::new(dot_x, dot_y),
            unread_dot_size / 2.0,
            style::ACCENT,
        );
    }

    // Avatar position — start after left padding
    let avatar_left = content_left;
    let avatar_top = rect.top() + (row_height - avatar_size) / 2.0;
    let avatar_rect =
        Rect::from_min_size(Pos2::new(avatar_left, avatar_top), Vec2::splat(avatar_size));

    // Resolve sender name for initials
    let _ = get_profile_metadata(app, entry.pubkey.clone());
    let sender_name = app
        .resolve_name(&entry.pubkey)
        .unwrap_or_else(|| entry.pubkey.to_string());

    // Compute initials and paint avatar via shared component
    let initials = style::initials_for_name(&sender_name);
    style::paint_avatar(painter, avatar_rect, &initials, None);

    // Text column starts after avatar + gap
    let text_left = avatar_left + avatar_size + avatar_gap;
    let text_right = rect.right() - pad_x;
    let text_top = rect.top() + pad_y;

    // Row heights use actual galley sizes to prevent misalignment and clipping
    let row1_y = text_top;

    // Timestamp — right-aligned on row 1
    let timestamp = style::format_timestamp(entry.created_at);
    let timestamp_font = FontId::proportional(11.5);
    let timestamp_galley =
        painter.layout_no_wrap(timestamp.clone(), timestamp_font.clone(), style::TEXT3);
    let timestamp_width = timestamp_galley.size().x;
    let timestamp_x = text_right - timestamp_width;
    painter.galley(
        Pos2::new(timestamp_x, row1_y),
        timestamp_galley,
        style::TEXT3,
    );

    // Sender name — row 1, left-aligned, truncated before timestamp
    let sender_color = if is_unread { style::TEXT } else { style::TEXT2 };
    let sender_font = FontId::proportional(13.5);
    let sender_max_width = (timestamp_x - text_left - 8.0).max(0.0);
    let sender_galley = painter.layout(
        sender_name.clone(),
        sender_font,
        sender_color,
        sender_max_width,
    );
    let sender_pos = Pos2::new(text_left, row1_y);
    painter.galley(sender_pos, sender_galley.clone(), sender_color);

    // Subject — row 2
    let row2_y = row1_y + sender_galley.size().y + 2.0;
    let subject_color = if is_unread { style::TEXT } else { style::TEXT2 };
    let subject_font = FontId::proportional(13.0);
    let subject_max_width = (text_right - text_left).max(0.0);
    let subject_text = if entry.subject.is_empty() {
        "(no subject)".to_string()
    } else {
        entry.subject.clone()
    };
    let subject_galley =
        painter.layout(subject_text, subject_font, subject_color, subject_max_width);
    painter.galley(
        Pos2::new(text_left, row2_y),
        subject_galley.clone(),
        subject_color,
    );

    // Preview — row 3, truncated
    let row3_y = row2_y + subject_galley.size().y + 2.0;
    let preview_max_width = (text_right - text_left).max(0.0);
    let preview_text = make_preview(&entry.content);
    let preview_galley = painter.layout(
        preview_text,
        FontId::proportional(12.5),
        style::TEXT3,
        preview_max_width,
    );
    painter.galley(
        Pos2::new(text_left, row3_y),
        preview_galley.clone(),
        style::TEXT3,
    );

    // Badge row — 5px below preview, matching HTML .message-footer margin-top: 5px
    let badge_y = row3_y + preview_galley.size().y + 5.0;
    style::paint_badge_at(
        painter,
        Pos2::new(text_left, badge_y),
        style::BadgeKind::Nostr,
    );

    // Handle click
    if response.clicked() {
        app.focused_post = entry.id.clone();
        app.page = Page::Post;
        app.show_trashed_post = false;
    }
}

/// Create a short preview snippet from message content.
/// Strips whitespace/newlines and truncates to a reasonable length.
fn make_preview(content: &str) -> String {
    let collapsed: String = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.len() > 120 {
        format!("{}…", &collapsed[..117])
    } else {
        collapsed
    }
}

/// Render the status bar at the bottom of the inbox.
fn render_status_bar(app: &mut Hoot, ui: &mut egui::Ui) {
    // Draw top border line
    let rect = ui.max_rect();
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(1.0, style::border()),
    );

    ui.horizontal_centered(|ui| {
        ui.add_space(12.0);

        // Count total and connected relays via the backend boundary.
        let statuses = app.backend.relay_statuses().unwrap_or_default();
        let total_relays = statuses.len();
        let connected_relays = statuses
            .iter()
            .filter(|relay| matches!(relay.status, RelayConnectionStatus::Connected))
            .count();

        let relay_text = format!("{}/{} relays", connected_relays, total_relays);
        ui.label(
            egui::RichText::new(relay_text)
                .size(11.0)
                .color(style::TEXT3),
        );
    });
}
