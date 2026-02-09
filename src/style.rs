use eframe::egui::{self, Color32, Rounding, Stroke, Vec2};
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
    let rounding = Rounding::same(6.0);
    visuals.widgets.noninteractive.rounding = rounding;
    visuals.widgets.inactive.rounding = rounding;
    visuals.widgets.hovered.rounding = rounding;
    visuals.widgets.active.rounding = rounding;
    visuals.widgets.open.rounding = rounding;
    visuals.window_rounding = Rounding::same(10.0);
    visuals.menu_rounding = Rounding::same(8.0);

    // Selection highlight uses accent
    visuals.selection.bg_fill = ACCENT_LIGHT;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    // Softer window shadow
    visuals.window_shadow = Shadow {
        offset: Vec2::new(0.0, 4.0),
        blur: 12.0,
        spread: 0.0,
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
