use eframe::egui::{self, Color32, CornerRadius, Stroke, Vec2};
use eframe::epaint::Shadow;

// ── Colors ──────────────────────────────────────────────────────────────

pub const ACCENT: Color32 = Color32::from_rgb(149, 117, 205);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(232, 224, 245);
pub const SIDEBAR_BG: Color32 = Color32::from_rgb(245, 243, 248);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(140, 140, 150);
pub const CARD_BG: Color32 = Color32::WHITE;
pub const CARD_STROKE: Color32 = Color32::from_rgb(220, 218, 225);

// ── Layout ──────────────────────────────────────────────────────────────

pub const SIDEBAR_WIDTH: f32 = 220.0;
pub const INBOX_ROW_HEIGHT: f32 = 40.0;
pub const AVATAR_SIZE: f32 = 48.0;

// ── Theme ───────────────────────────────────────────────────────────────

pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.dark_mode = false;

    // Rounded widgets everywhere
    let rounding = CornerRadius::same(6);
    visuals.widgets.noninteractive.corner_radius = rounding;
    visuals.widgets.inactive.corner_radius = rounding;
    visuals.widgets.hovered.corner_radius = rounding;
    visuals.widgets.active.corner_radius = rounding;
    visuals.widgets.open.corner_radius = rounding;

    // Selection highlight uses accent
    visuals.selection.bg_fill = ACCENT_LIGHT;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    // Softer window shadow
    visuals.window_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(30),
    };

    // Softer widget borders
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(225, 223, 230));
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(210, 208, 215));

    // Slightly warmer panel background
    visuals.panel_fill = Color32::from_rgb(252, 251, 254);
    visuals.window_fill = Color32::from_rgb(255, 255, 255);

    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.spacing.button_padding = Vec2::new(8.0, 3.0);
    });
}

// ── Helpers ──────────────────────────────────────────────────────────────

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
        dt.format("%A").to_string() // "Monday", "Tuesday", etc.
    } else if dt.year() == now.year() {
        dt.format("%b %-d").to_string() // "Jan 15"
    } else {
        dt.format("%b %-d, %Y").to_string() // "Jan 15, 2024"
    }
}
