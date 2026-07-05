use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Margin, Pos2, Stroke, StrokeKind, Vec2,
};
use eframe::epaint::Shadow;

// ── Color Tokens ─────────────────────────────────────────────────────────────

/// Page background — warm off-white #f5f4f2
pub const BG: Color32 = Color32::from_rgb(245, 244, 242);
/// Card / sidebar surface #fdfcfa
pub const SURFACE: Color32 = Color32::from_rgb(253, 252, 250);
/// Input / chip / hover surface #f0eeeb
pub const SURFACE2: Color32 = Color32::from_rgb(240, 238, 235);
/// Light border rgba(0,0,0,0.07)
pub fn border() -> Color32 {
    Color32::from_rgba_unmultiplied(0, 0, 0, 18)
}
/// Strong border rgba(0,0,0,0.12)
pub fn border_strong() -> Color32 {
    Color32::from_rgba_unmultiplied(0, 0, 0, 31)
}

/// Primary text #1a1917
pub const TEXT: Color32 = Color32::from_rgb(26, 25, 23);
/// Secondary text / read messages #6b6762
pub const TEXT2: Color32 = Color32::from_rgb(107, 103, 98);
/// Tertiary text / timestamps / labels #a8a49f
pub const TEXT3: Color32 = Color32::from_rgb(168, 164, 159);

/// Purple accent #7c3aed
pub const ACCENT: Color32 = Color32::from_rgb(124, 58, 237);
/// Accent soft background rgba(124,58,237,0.08)
pub fn accent_soft() -> Color32 {
    Color32::from_rgba_unmultiplied(124, 58, 237, 20)
}
/// Success / online green #16a34a
pub const GREEN: Color32 = Color32::from_rgb(22, 163, 74);
/// Warning amber #d97706
pub const AMBER: Color32 = Color32::from_rgb(217, 119, 6);

// Badge colors
pub fn badge_nostr_bg() -> Color32 {
    Color32::from_rgba_unmultiplied(37, 99, 235, 20)
}
pub const BADGE_NOSTR_TEXT: Color32 = ACCENT;
pub fn badge_pgp_bg() -> Color32 {
    Color32::from_rgba_unmultiplied(22, 163, 74, 20)
}
pub const BADGE_PGP_TEXT: Color32 = GREEN;
pub fn badge_smtp_bg() -> Color32 {
    Color32::from_rgba_unmultiplied(0, 0, 0, 13)
}
pub const BADGE_SMTP_TEXT: Color32 = TEXT2;

// Avatar gradient approximations (solid first stop)
pub const AV_BLUE_BG: Color32 = Color32::from_rgb(219, 234, 254);
pub const AV_BLUE_TEXT: Color32 = Color32::from_rgb(29, 78, 216);
pub const AV_GREEN_BG: Color32 = Color32::from_rgb(220, 252, 231);
pub const AV_GREEN_TEXT: Color32 = Color32::from_rgb(21, 128, 61);
pub const AV_AMBER_BG: Color32 = Color32::from_rgb(254, 243, 199);
pub const AV_AMBER_TEXT: Color32 = Color32::from_rgb(146, 64, 14);
pub const AV_PURPLE_BG: Color32 = Color32::from_rgb(237, 233, 254);
pub const AV_PURPLE_TEXT: Color32 = Color32::from_rgb(109, 40, 217);
pub const AV_DEFAULT_BG: Color32 = Color32::from_rgb(226, 224, 219);

// ── Layout ───────────────────────────────────────────────────────────────────

pub const SIDEBAR_WIDTH: f32 = 232.0;
pub const AVATAR_SIZE: f32 = 34.0;
pub const AVATAR_RADIUS: f32 = 9.0;
pub const STATUS_BAR_HEIGHT: f32 = 28.0;
pub const INBOX_ROW_HEIGHT: f32 = 96.0;

// ── Shadows ──────────────────────────────────────────────────────────────────

pub fn shadow_sm() -> Shadow {
    Shadow {
        offset: [0, 1],
        blur: 2,
        spread: 0,
        color: Color32::from_black_alpha(15),
    }
}
pub fn shadow_md() -> Shadow {
    Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(20),
    }
}
pub fn shadow_lg() -> Shadow {
    Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(26),
    }
}

// ── Theme Application ─────────────────────────────────────────────────────────

pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.dark_mode = false;

    let rounding = CornerRadius::same(8);
    visuals.widgets.noninteractive.corner_radius = rounding;
    visuals.widgets.inactive.corner_radius = rounding;
    visuals.widgets.hovered.corner_radius = rounding;
    visuals.widgets.active.corner_radius = rounding;
    visuals.widgets.open.corner_radius = rounding;

    visuals.selection.bg_fill = accent_soft();
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;

    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border());
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, border_strong());
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_black_alpha(46));
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);

    visuals.widgets.inactive.weak_bg_fill = SURFACE2;
    visuals.widgets.inactive.bg_fill = SURFACE2;
    visuals.widgets.hovered.weak_bg_fill = SURFACE;
    visuals.widgets.hovered.bg_fill = SURFACE;

    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT2);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT2);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);

    visuals.window_shadow = shadow_md();

    // Set extreme_bg_color (scrollbar track, text input bg) to a warm light color
    visuals.extreme_bg_color = SURFACE2;
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);

    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.item_spacing = Vec2::new(8.0, 4.0);
        style.spacing.window_margin = Margin::same(0);
        style.spacing.scroll.bar_width = 4.0;
        style.spacing.scroll.bar_inner_margin = 2.0;
    });
}

pub fn pointer(response: egui::Response) -> egui::Response {
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ── Timestamp Formatting ──────────────────────────────────────────────────────

pub fn format_timestamp(epoch_secs: i64) -> String {
    use chrono::{DateTime, Datelike, Local};

    let dt: DateTime<Local> = match DateTime::from_timestamp(epoch_secs, 0) {
        Some(utc) => utc.with_timezone(&Local),
        None => return epoch_secs.to_string(),
    };

    let now: DateTime<Local> = Local::now();
    let today = now.date_naive();
    let msg_date = dt.date_naive();

    if msg_date == today {
        dt.format("%-I:%M %p").to_string()
    } else if msg_date == today.pred_opt().unwrap_or(today) {
        "Yesterday".to_string()
    } else if (today - msg_date).num_days() < 7 {
        dt.format("%A").to_string()
    } else if dt.year() == now.year() {
        dt.format("%b %-d").to_string()
    } else {
        dt.format("%b %-d, %Y").to_string()
    }
}

// ── Shared Widget Helpers ─────────────────────────────────────────────────────

/// Derive up to 2 initials from a display name.
pub fn initials_for_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return "?".to_string();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.len() == 1 {
        let mut chars = words[0].chars();
        match (chars.next(), chars.next()) {
            (Some(a), Some(b)) => format!("{}{}", a, b).to_uppercase(),
            (Some(a), None) => a.to_uppercase().to_string(),
            _ => "?".to_string(),
        }
    } else {
        let a: String = words[0].chars().take(1).collect();
        let b: String = words[words.len() - 1].chars().take(1).collect();
        format!("{}{}", a, b).to_uppercase()
    }
}

/// Pick a consistent avatar background/text color pair from initials hash.
pub fn avatar_color_for_initials(initials: &str) -> (Color32, Color32) {
    let h = initials
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    match h % 5 {
        0 => (AV_BLUE_BG, AV_BLUE_TEXT),
        1 => (AV_GREEN_BG, AV_GREEN_TEXT),
        2 => (AV_AMBER_BG, AV_AMBER_TEXT),
        3 => (AV_PURPLE_BG, AV_PURPLE_TEXT),
        _ => (AV_DEFAULT_BG, TEXT2),
    }
}

/// Paint a protocol badge at a given position using raw painter calls.
/// Returns the width of the painted badge (for chaining multiple badges).
pub fn paint_badge_at(painter: &egui::Painter, pos: egui::Pos2, kind: BadgeKind) -> f32 {
    let (label, bg, fg) = match kind {
        BadgeKind::Nostr => ("nostr", badge_nostr_bg(), BADGE_NOSTR_TEXT),
        BadgeKind::Pgp => ("pgp", badge_pgp_bg(), BADGE_PGP_TEXT),
        BadgeKind::Smtp => ("smtp", badge_smtp_bg(), BADGE_SMTP_TEXT),
    };
    let galley = painter.layout_no_wrap(label.to_string(), FontId::proportional(10.0), fg);
    let text_size = galley.size();
    let pad_x = 7.0;
    let pad_y = 2.0;
    let badge_w = text_size.x + 2.0 * pad_x;
    let badge_h = text_size.y + 2.0 * pad_y;
    let rect = egui::Rect::from_min_size(pos, egui::vec2(badge_w, badge_h));
    painter.rect_filled(rect, CornerRadius::same(4), bg);
    painter.galley(egui::Pos2::new(pos.x + pad_x, pos.y + pad_y), galley, fg);
    badge_w
}

/// Paint an avatar into an existing rect using raw painter calls.
/// Shared by both `render_avatar` (egui layout) and custom-painter rows (inbox).
pub fn paint_avatar(
    painter: &egui::Painter,
    rect: egui::Rect,
    initials: &str,
    image: Option<&egui::TextureHandle>,
) {
    let cr = CornerRadius::same(AVATAR_RADIUS.round() as u8);
    if let Some(tex) = image {
        painter.image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(
            rect,
            cr,
            Stroke::new(1.0, border_strong()),
            StrokeKind::Outside,
        );
    } else {
        let (bg, fg) = avatar_color_for_initials(initials);
        painter.rect_filled(rect, cr, bg);
        painter.rect_stroke(
            rect,
            cr,
            Stroke::new(1.0, border_strong()),
            StrokeKind::Outside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            initials,
            FontId::proportional(12.0),
            fg,
        );
    }
}

/// Render a 34×34 rounded avatar with initials (or image if provided).
pub fn render_avatar(ui: &mut egui::Ui, initials: &str, image: Option<&egui::TextureHandle>) {
    let size = Vec2::splat(AVATAR_SIZE);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_avatar(ui.painter(), rect, initials, image);
    }
}

/// Badge protocol type.
#[derive(Clone, Copy)]
pub enum BadgeKind {
    Nostr,
    Pgp,
    Smtp,
}

/// Render a protocol badge inline.
pub fn render_badge(ui: &mut egui::Ui, kind: BadgeKind) {
    let (label, bg, fg) = match kind {
        BadgeKind::Nostr => ("nostr", badge_nostr_bg(), BADGE_NOSTR_TEXT),
        BadgeKind::Pgp => ("pgp", badge_pgp_bg(), BADGE_PGP_TEXT),
        BadgeKind::Smtp => ("smtp", badge_smtp_bg(), BADGE_SMTP_TEXT),
    };
    let frame = egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin {
            left: 7,
            right: 7,
            top: 2,
            bottom: 2,
        });
    frame.show(ui, |ui| {
        ui.label(egui::RichText::new(label).size(10.0).color(fg).strong());
    });
}

/// Boxed TextEdit — rounded border that glows accent on focus, matching the search bar style.
/// Requires a stable `id` so focus state can be queried before painting.
pub fn boxed_text_edit(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    hint: impl Into<egui::WidgetText>,
    font_size: f32,
) -> egui::Response {
    boxed_text_edit_impl(ui, id, text, hint, font_size, false)
}

/// Boxed TextEdit for secrets (masked).
pub fn boxed_password_text_edit(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    hint: impl Into<egui::WidgetText>,
    font_size: f32,
) -> egui::Response {
    boxed_text_edit_impl(ui, id, text, hint, font_size, true)
}

fn boxed_text_edit_impl(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    hint: impl Into<egui::WidgetText>,
    font_size: f32,
    password: bool,
) -> egui::Response {
    let height = 30.0;
    let (outer_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );

    let has_focus = ui.ctx().memory(|m| m.has_focus(id));
    let bg = if has_focus { SURFACE } else { SURFACE2 };
    let border_color = if has_focus { ACCENT } else { border_strong() };

    let painter = ui.painter();
    painter.rect_filled(outer_rect, CornerRadius::same(8), bg);
    painter.rect_stroke(
        outer_rect,
        CornerRadius::same(8),
        Stroke::new(1.0, border_color),
        StrokeKind::Inside,
    );
    if has_focus {
        painter.rect_stroke(
            outer_rect.expand(1.5),
            CornerRadius::same(9),
            Stroke::new(2.0, accent_soft()),
            StrokeKind::Outside,
        );
    }

    let inner_rect = outer_rect.shrink2(egui::vec2(10.0, 5.0));
    let mut child_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child_ui.add_sized(
        inner_rect.size(),
        egui::TextEdit::singleline(text)
            .id(id)
            .hint_text(hint)
            .password(password)
            .frame(false)
            .font(FontId::proportional(font_size))
            .text_color(TEXT)
            .margin(egui::vec2(0.0, 0.0)),
    )
}

/// Underline-style TextEdit — no box border, just a line below.
pub fn underline_text_edit(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: impl Into<egui::WidgetText>,
    font_size: f32,
) -> egui::Response {
    underline_text_edit_impl(ui, text, hint, font_size, false)
}

fn underline_text_edit_impl(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: impl Into<egui::WidgetText>,
    font_size: f32,
    password: bool,
) -> egui::Response {
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .password(password)
            .frame(false)
            .font(FontId::proportional(font_size))
            .text_color(TEXT)
            .desired_width(f32::INFINITY),
    );
    let rect = response.rect;
    let y = rect.bottom() + 3.0;
    let (color, width) = if response.has_focus() {
        (ACCENT, 2.0)
    } else {
        (border_strong(), 1.0)
    };
    ui.painter().line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(width, color),
    );
    response
}

// Compatibility aliases for split-layout UI modules that still use the pre-redesign names.
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(232, 224, 245);
pub const SIDEBAR_BG: Color32 = SURFACE;
pub const TEXT_MUTED: Color32 = TEXT3;
pub const CARD_BG: Color32 = SURFACE;
pub const CARD_STROKE: Color32 = Color32::from_rgb(220, 218, 225);
