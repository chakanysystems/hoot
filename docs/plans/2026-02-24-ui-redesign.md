# UI Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reimplement every Hoot UI page/component to pixel-precise parity with the design specification in `example-doc.html` and `DESIGN.md`.

**Architecture:** All custom styling is driven through `src/style.rs` (color constants + `apply_theme`). Each page module in `src/ui/` is rewritten to use custom-painted egui widgets instead of stock widgets. The sidebar is rebuilt in `src/main.rs`. No new dependencies needed.

**Tech Stack:** Rust, egui 0.33.3, eframe 0.33.3, epaint (Shadow, CornerRadius, Stroke, Mesh). No CSS — all design values translated to egui equivalents.

---

## Design Token Reference

Keep this section open while implementing. All values come from `example-doc.html`.

### Colors
```
BG:           #f5f4f2  → Color32::from_rgb(245, 244, 242)
SURFACE:      #fdfcfa  → Color32::from_rgb(253, 252, 250)
SURFACE2:     #f0eeeb  → Color32::from_rgb(240, 238, 235)
BORDER:       rgba(0,0,0,0.07)  → Color32::from_rgba_unmultiplied(0, 0, 0, 18)
BORDER_STRONG:rgba(0,0,0,0.12) → Color32::from_rgba_unmultiplied(0, 0, 0, 31)
TEXT:         #1a1917  → Color32::from_rgb(26, 25, 23)
TEXT2:        #6b6762  → Color32::from_rgb(107, 103, 98)
TEXT3:        #a8a49f  → Color32::from_rgb(168, 164, 159)
ACCENT:       #7c3aed  → Color32::from_rgb(124, 58, 237)
ACCENT_SOFT:  rgba(124,58,237,0.08) → Color32::from_rgba_unmultiplied(124, 58, 237, 20)
GREEN:        #16a34a  → Color32::from_rgb(22, 163, 74)
```

### Shadows (epaint::Shadow)
```
shadow_sm:  offset:[0,1], blur:2,  spread:0, color:from_black_alpha(15)
shadow:     offset:[0,2], blur:8,  spread:0, color:from_black_alpha(20)
shadow_lg:  offset:[0,8], blur:24, spread:0, color:from_black_alpha(26)
```

### Sizes
```
Sidebar width:   232px
Avatar:          34×34px, corner_radius 9
Badge (message): corner_radius 4, padding 2×7px
Badge (nav):     corner_radius 10, padding 1×6px
Compose button:  corner_radius 10, padding 10×16px
Nav item:        corner_radius 8, padding 7×10px
Unread dot:      5px diameter
Inbox row:       ~72px height (avatar+2 text lines+badges)
Status bar:      ~28px height
```

### Typography (egui FontId)
```
App name:       size 17, strong (weight 600)
Body default:   size 13.5 (set via style.spacing)
Sender name:    size 13.5, color TEXT (unread: strong, read: TEXT2)
Subject:        size 13.0, color TEXT (read: TEXT2)
Preview:        size 12.5, color TEXT3
Timestamp:      size 11.5, color TEXT3
Badge:          size 10.0, strong
Status bar:     size 11.0, color TEXT3
Nav label:      size 11.0, strong, TEXT3 (uppercase)
```

---

## Task 1: Add Instrument Sans Font

**Files:**
- Download: `assets/InstrumentSans-Regular.ttf`, `assets/InstrumentSans-Medium.ttf`, `assets/InstrumentSans-SemiBold.ttf`
- Modify: `src/main.rs` (lines ~44-56, font setup)

**Step 1: Download Instrument Sans**

Download from Google Fonts: https://fonts.google.com/specimen/Instrument+Sans

Place the following files in `assets/`:
- `InstrumentSans-Regular.ttf` (weight 400)
- `InstrumentSans-Medium.ttf` (weight 500)
- `InstrumentSans-SemiBold.ttf` (weight 600)

**Step 2: Update font loading in `src/main.rs`**

Replace the Inter font block (lines ~44-56) with:

```rust
let mut fonts = FontDefinitions::default();

// Instrument Sans — primary UI font
fonts.font_data.insert(
    "InstrumentSans".to_owned(),
    std::sync::Arc::new(egui::FontData::from_static(
        include_bytes!("../assets/InstrumentSans-Regular.ttf")
    )),
);
fonts.font_data.insert(
    "InstrumentSans-Medium".to_owned(),
    std::sync::Arc::new(egui::FontData::from_static(
        include_bytes!("../assets/InstrumentSans-Medium.ttf")
    )),
);
fonts.font_data.insert(
    "InstrumentSans-SemiBold".to_owned(),
    std::sync::Arc::new(egui::FontData::from_static(
        include_bytes!("../assets/InstrumentSans-SemiBold.ttf")
    )),
);

// Set InstrumentSans as primary proportional font
fonts
    .families
    .get_mut(&FontFamily::Proportional)
    .unwrap()
    .insert(0, "InstrumentSans".to_owned());

cc.egui_ctx.set_fonts(fonts);
```

**Note:** egui's `strong()` / `.strong()` on RichText will use the first Proportional font at a heavier weight simulation. For true weight 500/600, we use `InstrumentSans-Medium` and `InstrumentSans-SemiBold` as separate named families where needed, or simply rely on egui's `.strong()` which renders bolder. This is acceptable for 0.33.3.

**Step 3: Verify build**

```bash
cargo build 2>&1 | head -30
```
Expected: compiles, no missing file errors.

**Step 4: Commit**

```bash
git add assets/InstrumentSans*.ttf src/main.rs
git commit -m "feat: add Instrument Sans font"
```

---

## Task 2: Overhaul `style.rs` — Design Tokens

**Files:**
- Modify: `src/style.rs` (complete rewrite of constants and `apply_theme`)

**Step 1: Replace all constants in `src/style.rs`**

Replace the entire file content with:

```rust
use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, Stroke, Vec2};
use eframe::epaint::Shadow;

// ── Color Tokens ─────────────────────────────────────────────────────────────

/// Page background — warm off-white #f5f4f2
pub const BG: Color32 = Color32::from_rgb(245, 244, 242);
/// Card / sidebar surface #fdfcfa
pub const SURFACE: Color32 = Color32::from_rgb(253, 252, 250);
/// Input / chip / hover surface #f0eeeb
pub const SURFACE2: Color32 = Color32::from_rgb(240, 238, 235);
/// Light border rgba(0,0,0,0.07)
pub const BORDER: Color32 = Color32::from_rgba_unmultiplied(0, 0, 0, 18);
/// Strong border rgba(0,0,0,0.12)
pub const BORDER_STRONG: Color32 = Color32::from_rgba_unmultiplied(0, 0, 0, 31);

/// Primary text #1a1917
pub const TEXT: Color32 = Color32::from_rgb(26, 25, 23);
/// Secondary text / read messages #6b6762
pub const TEXT2: Color32 = Color32::from_rgb(107, 103, 98);
/// Tertiary text / timestamps / labels #a8a49f
pub const TEXT3: Color32 = Color32::from_rgb(168, 164, 159);

/// Purple accent #7c3aed
pub const ACCENT: Color32 = Color32::from_rgb(124, 58, 237);
/// Accent soft background rgba(124,58,237,0.08)
pub const ACCENT_SOFT: Color32 = Color32::from_rgba_unmultiplied(124, 58, 237, 20);
/// Success / online green #16a34a
pub const GREEN: Color32 = Color32::from_rgb(22, 163, 74);
/// Warning amber #d97706
pub const AMBER: Color32 = Color32::from_rgb(217, 119, 6);

// Badge colors
/// Nostr badge background rgba(37,99,235,0.08)
pub const BADGE_NOSTR_BG: Color32 = Color32::from_rgba_unmultiplied(37, 99, 235, 20);
/// Nostr badge text (purple) #7c3aed
pub const BADGE_NOSTR_TEXT: Color32 = ACCENT;
/// PGP badge background rgba(22,163,74,0.08)
pub const BADGE_PGP_BG: Color32 = Color32::from_rgba_unmultiplied(22, 163, 74, 20);
/// PGP badge text (green) #16a34a
pub const BADGE_PGP_TEXT: Color32 = GREEN;
/// SMTP badge background rgba(0,0,0,0.05)
pub const BADGE_SMTP_BG: Color32 = Color32::from_rgba_unmultiplied(0, 0, 0, 13);
/// SMTP badge text #6b6762
pub const BADGE_SMTP_TEXT: Color32 = TEXT2;

// Avatar gradient approximations (solid first stop)
pub const AV_BLUE_BG: Color32 = Color32::from_rgb(219, 234, 254);   // #dbeafe
pub const AV_BLUE_TEXT: Color32 = Color32::from_rgb(29, 78, 216);   // #1d4ed8
pub const AV_GREEN_BG: Color32 = Color32::from_rgb(220, 252, 231);  // #dcfce7
pub const AV_GREEN_TEXT: Color32 = Color32::from_rgb(21, 128, 61);  // #15803d
pub const AV_AMBER_BG: Color32 = Color32::from_rgb(254, 243, 199);  // #fef3c7
pub const AV_AMBER_TEXT: Color32 = Color32::from_rgb(146, 64, 14);  // #92400e
pub const AV_PURPLE_BG: Color32 = Color32::from_rgb(237, 233, 254); // #ede9fe
pub const AV_PURPLE_TEXT: Color32 = Color32::from_rgb(109, 40, 217); // #6d28d9
pub const AV_DEFAULT_BG: Color32 = Color32::from_rgb(226, 224, 219); // #e2e0db

// ── Layout ───────────────────────────────────────────────────────────────────

pub const SIDEBAR_WIDTH: f32 = 232.0;
pub const AVATAR_SIZE: f32 = 34.0;
pub const AVATAR_RADIUS: f32 = 9.0;
pub const STATUS_BAR_HEIGHT: f32 = 28.0;
pub const INBOX_ROW_HEIGHT: f32 = 72.0;

// ── Shadows ──────────────────────────────────────────────────────────────────

pub fn shadow_sm() -> Shadow {
    Shadow { offset: [0, 1], blur: 2, spread: 0, color: Color32::from_black_alpha(15) }
}
pub fn shadow_md() -> Shadow {
    Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(20) }
}
pub fn shadow_lg() -> Shadow {
    Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(26) }
}

// ── Theme Application ─────────────────────────────────────────────────────────

pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.dark_mode = false;

    // Global corner radius — 8px for inputs, 6px for widgets
    let rounding = CornerRadius::same(8);
    visuals.widgets.noninteractive.corner_radius = rounding;
    visuals.widgets.inactive.corner_radius = rounding;
    visuals.widgets.hovered.corner_radius = rounding;
    visuals.widgets.active.corner_radius = rounding;
    visuals.widgets.open.corner_radius = rounding;

    // Selection = accent
    visuals.selection.bg_fill = ACCENT_SOFT;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    // Panel / window backgrounds
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;

    // Widget borders — use design border values
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 46));
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);

    // Widget fills
    visuals.widgets.inactive.weak_bg_fill = SURFACE2;
    visuals.widgets.inactive.bg_fill = SURFACE2;
    visuals.widgets.hovered.weak_bg_fill = SURFACE;
    visuals.widgets.hovered.bg_fill = SURFACE;

    // Widget text colors
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT2);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT2);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);

    // Window shadow
    visuals.window_shadow = shadow_md();

    // Scrollbar — 4px, barely visible
    visuals.scroll_bar_width = 4.0;
    visuals.scroll_bar_inner_margin = 2.0;

    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        // Tighter button padding matching design
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.item_spacing = Vec2::new(8.0, 4.0);
        style.spacing.window_margin = Margin::same(0.0);
    });
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

/// Pick a consistent avatar background color from initials hash.
pub fn avatar_color_for_initials(initials: &str) -> (Color32, Color32) {
    let h = initials.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    match h % 5 {
        0 => (AV_BLUE_BG, AV_BLUE_TEXT),
        1 => (AV_GREEN_BG, AV_GREEN_TEXT),
        2 => (AV_AMBER_BG, AV_AMBER_TEXT),
        3 => (AV_PURPLE_BG, AV_PURPLE_TEXT),
        _ => (AV_DEFAULT_BG, TEXT2),
    }
}

/// Render a 34×34 rounded avatar with initials.
pub fn render_avatar(ui: &mut egui::Ui, initials: &str, image: Option<&egui::TextureHandle>) {
    use eframe::egui::{Rect, Rounding, Sense, Vec2};

    let size = Vec2::splat(AVATAR_SIZE);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());

    if ui.is_rect_visible(rect) {
        let (bg, fg) = avatar_color_for_initials(initials);
        let cr = eframe::egui::CornerRadius::same(AVATAR_RADIUS as u8);

        if let Some(tex) = image {
            ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)), Color32::WHITE);
            ui.painter().rect_stroke(rect, cr, Stroke::new(1.0, BORDER_STRONG));
        } else {
            ui.painter().rect_filled(rect, cr, bg);
            ui.painter().rect_stroke(rect, cr, Stroke::new(1.0, BORDER_STRONG));
            ui.painter().text(
                rect.center(),
                eframe::egui::Align2::CENTER_CENTER,
                initials,
                FontId::proportional(12.0),
                fg,
            );
        }
    }
}

/// Render a protocol badge (nostr/pgp/smtp).
#[derive(Clone, Copy)]
pub enum BadgeKind { Nostr, Pgp, Smtp }

pub fn render_badge(ui: &mut egui::Ui, kind: BadgeKind) {
    let (label, bg, fg) = match kind {
        BadgeKind::Nostr => ("nostr", BADGE_NOSTR_BG, BADGE_NOSTR_TEXT),
        BadgeKind::Pgp   => ("pgp",   BADGE_PGP_BG,   BADGE_PGP_TEXT),
        BadgeKind::Smtp  => ("smtp",  BADGE_SMTP_BG,  BADGE_SMTP_TEXT),
    };
    let frame = eframe::egui::Frame::none()
        .fill(bg)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin { left: 7.0, right: 7.0, top: 2.0, bottom: 2.0 });
    frame.show(ui, |ui| {
        ui.label(eframe::egui::RichText::new(label).size(10.0).color(fg).strong());
    });
}

/// Draw a thin underline below a TextEdit — use for compose To/Subject fields.
pub fn underline_text_edit<'a>(
    ui: &mut egui::Ui,
    text: &'a mut String,
    hint: &'static str,
    font_size: f32,
) -> egui::Response {
    let response = ui.add(
        eframe::egui::TextEdit::singleline(text)
            .hint_text(hint)
            .frame(false)
            .font(FontId::proportional(font_size))
            .text_color(TEXT)
            .desired_width(f32::INFINITY),
    );
    // Draw underline
    let rect = response.rect;
    let y = rect.bottom() + 3.0;
    let (color, width) = if response.has_focus() {
        (ACCENT, 2.0)
    } else {
        (BORDER_STRONG, 1.0)
    };
    ui.painter().line_segment(
        [eframe::egui::Pos2::new(rect.left(), y), eframe::egui::Pos2::new(rect.right(), y)],
        Stroke::new(width, color),
    );
    response
}
```

**Step 2: Fix all files that imported old constants**

Run:
```bash
cargo build 2>&1 | grep "error\[" | head -40
```

Fix each compilation error by updating the import paths. Old names → new names:
- `ACCENT` stays `ACCENT` (different value now)
- `ACCENT_LIGHT` → `ACCENT_SOFT`
- `SIDEBAR_BG` → `SURFACE` (or `BG`)
- `TEXT_MUTED` → `TEXT2` or `TEXT3`
- `CARD_BG` → `SURFACE`
- `CARD_STROKE` → `BORDER_STRONG`

**Step 3: Build clean**

```bash
cargo build 2>&1 | grep -c "^error"
```
Expected: `0`

**Step 4: Commit**

```bash
git add src/style.rs src/main.rs
git commit -m "feat: overhaul style.rs with design token colors and helpers"
```

---

## Task 3: Sidebar Redesign

**Files:**
- Modify: `src/main.rs` — the `egui::SidePanel::left(...)` block (lines ~140–272)

The sidebar has three zones:
1. **Top**: App name + identity pill (online dot + truncated npub)
2. **Middle**: Compose button + nav items
3. **Bottom**: Account switcher chip + settings icon button

**Step 1: Rewrite the sidebar render block in `src/main.rs`**

Find the `SidePanel::left` block and replace with:

```rust
egui::SidePanel::left("left_panel")
    .exact_width(style::SIDEBAR_WIDTH)
    .frame(
        egui::Frame::none()
            .fill(style::SURFACE)
            .inner_margin(egui::Margin::same(0.0))
    )
    .show(ctx, |ui| {
        ui.set_min_height(ui.available_height());
        ui.style_mut().visuals.panel_fill = style::SURFACE;

        // ── Top section ──────────────────────────────────────────────
        egui::Frame::none()
            .inner_margin(egui::Margin {
                left: 16.0, right: 16.0, top: 18.0, bottom: 14.0,
            })
            .show(ui, |ui| {
                // App name
                ui.label(
                    egui::RichText::new("Hoot")
                        .size(17.0)
                        .strong()
                        .color(style::TEXT),
                );
                ui.add_space(12.0);

                // Identity pill (online dot + truncated npub)
                let npub_short = app.account_manager.accounts
                    .first()
                    .and_then(|a| a.public_key().map(|k| {
                        let s = k.to_bech32().unwrap_or_default();
                        format!("{}…{}", &s[..8], &s[s.len().saturating_sub(4)..])
                    }))
                    .unwrap_or_else(|| "no account".to_string());

                let pill_frame = egui::Frame::none()
                    .fill(style::SURFACE2)
                    .stroke(egui::Stroke::new(1.0, style::BORDER_STRONG))
                    .corner_radius(egui::CornerRadius::same(20))
                    .inner_margin(egui::Margin {
                        left: 6.0, right: 10.0, top: 4.0, bottom: 4.0,
                    });
                pill_frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // Online dot
                        let (dot_rect, _) = ui.allocate_exact_size(
                            egui::Vec2::splat(7.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().circle_filled(dot_rect.center(), 3.5, style::GREEN);
                        // Truncated npub
                        ui.label(
                            egui::RichText::new(&npub_short)
                                .size(11.5)
                                .color(style::TEXT2),
                        );
                    });
                });
            });

        // Border under top section
        let top_end = ui.cursor().top();
        ui.painter().line_segment(
            [
                egui::Pos2::new(0.0, top_end),
                egui::Pos2::new(style::SIDEBAR_WIDTH, top_end),
            ],
            egui::Stroke::new(1.0, style::BORDER),
        );

        // ── Compose button ───────────────────────────────────────────
        ui.add_space(12.0);
        let compose_width = style::SIDEBAR_WIDTH - 24.0;
        let compose_frame = egui::Frame::none()
            .fill(style::ACCENT)
            .corner_radius(egui::CornerRadius::same(10))
            .shadow(eframe::epaint::Shadow {
                offset: [0, 1],
                blur: 3,
                spread: 0,
                color: egui::Color32::from_rgba_unmultiplied(37, 99, 235, 77),
            })
            .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 10.0, bottom: 10.0 });

        let compose_response = ui.add_space(0.0); // placeholder before frame
        let compose_inner = egui::Frame::none()
            .fill(style::ACCENT)
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 10.0, bottom: 10.0 })
            .show(ui, |ui| {
                ui.set_min_width(compose_width);
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("✏  Compose")
                            .size(13.5)
                            .color(egui::Color32::WHITE),
                    );
                });
            });

        // Make compose clickable
        let compose_rect = compose_inner.response.rect;
        let compose_sense = ui.allocate_rect(compose_rect, egui::Sense::click());
        if compose_sense.clicked() {
            let id = egui::Id::new(uuid::Uuid::new_v4().to_string());
            app.state.compose_windows.insert(id, ui::compose_window::ComposeWindowState::default());
        }

        ui.add_space(4.0);

        // ── Navigation ───────────────────────────────────────────────
        egui::Frame::none()
            .inner_margin(egui::Margin { left: 8.0, right: 8.0, top: 8.0, bottom: 8.0 })
            .show(ui, |ui| {
                // Helper closure for nav items
                let nav_items: &[(&str, &str, Page, Option<usize>)] = &[
                    ("Inbox",    "✉",  Page::Inbox,    app.state.inbox_unread_count()),
                    ("Drafts",   "📝", Page::Drafts,   None),
                    ("Starred",  "★",  Page::Starred,  None),
                    ("Archived", "⬇",  Page::Archived, None),
                    ("Trash",    "🗑", Page::Trash,    None),
                ];
                for (label, icon, page, badge) in nav_items {
                    render_nav_item(ui, app, label, icon, page.clone(), *badge);
                }

                // Divider
                ui.add_space(6.0);
                ui.painter().line_segment(
                    [
                        egui::Pos2::new(4.0, ui.cursor().top()),
                        egui::Pos2::new(style::SIDEBAR_WIDTH - 4.0, ui.cursor().top()),
                    ],
                    egui::Stroke::new(1.0, style::BORDER),
                );
                ui.add_space(6.0);

                // Label "Other"
                ui.label(
                    egui::RichText::new("OTHER")
                        .size(11.0)
                        .strong()
                        .color(style::TEXT3),
                );

                let other_items: &[(&str, &str, Page, Option<usize>)] = &[
                    ("Requests", "📬", Page::Requests, None),
                    ("Junk",     "🚫", Page::Junk,     None),
                    ("Contacts", "👤", Page::Contacts,  None),
                    ("Settings", "⚙",  Page::Settings,  None),
                ];
                for (label, icon, page, badge) in other_items {
                    render_nav_item(ui, app, label, icon, page.clone(), *badge);
                }
            });

        // ── Bottom footer — fills remaining space ────────────────────
        let avail = ui.available_height();
        ui.add_space(avail - 48.0); // push to bottom

        ui.painter().line_segment(
            [
                egui::Pos2::new(0.0, ui.cursor().top()),
                egui::Pos2::new(style::SIDEBAR_WIDTH, ui.cursor().top()),
            ],
            egui::Stroke::new(1.0, style::BORDER),
        );

        egui::Frame::none()
            .inner_margin(egui::Margin { left: 12.0, right: 12.0, top: 10.0, bottom: 10.0 })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    // Account selector chip
                    let account_label = app.account_manager.accounts
                        .first()
                        .and_then(|a| a.display_name().map(|n| n.to_string()))
                        .unwrap_or_else(|| "Account".to_string());

                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new(&account_label).size(12.0).color(style::TEXT2)
                        )
                        .fill(style::SURFACE2)
                        .stroke(egui::Stroke::new(1.0, style::BORDER_STRONG))
                        .corner_radius(egui::CornerRadius::same(7))
                        .min_size(egui::Vec2::new(0.0, 30.0)),
                    ).clicked() {
                        // open account switcher
                    }

                    // Settings icon button
                    if ui.add(
                        egui::Button::new(egui::RichText::new("⚙").color(style::TEXT2))
                            .fill(style::SURFACE2)
                            .stroke(egui::Stroke::new(1.0, style::BORDER_STRONG))
                            .corner_radius(egui::CornerRadius::same(7))
                            .min_size(egui::Vec2::new(30.0, 30.0)),
                    ).clicked() {
                        app.page = Page::Settings;
                    }
                });
            });
    });
```

**Step 2: Add `render_nav_item` helper function** (add near the top of `main.rs`, outside `render_app`):

```rust
fn render_nav_item(
    ui: &mut egui::Ui,
    app: &mut Hoot,
    label: &str,
    _icon: &str,
    page: Page,
    badge: Option<usize>,
) {
    use eframe::egui::*;

    let is_active = app.page == page;
    let desired_size = Vec2::new(ui.available_width(), 32.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(rect) {
        // Background
        let fill = if is_active {
            style::ACCENT_SOFT
        } else if response.hovered() {
            style::SURFACE2
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);

        // Active left border
        if is_active {
            let border = Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height()));
            ui.painter().rect_filled(border, CornerRadius::same(1), style::ACCENT);
        }

        // Text color
        let text_color = if is_active {
            style::ACCENT
        } else if response.hovered() {
            style::TEXT
        } else {
            style::TEXT2
        };

        // Label
        let text_rect = rect.shrink2(Vec2::new(10.0, 0.0));
        let weight = if is_active { FontId::proportional(13.5) } else { FontId::proportional(13.5) };
        let galley = ui.painter().layout_no_wrap(
            label.to_string(),
            weight,
            text_color,
        );
        let text_pos = Pos2::new(text_rect.left(), rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(text_pos, galley, text_color);

        // Badge
        if let Some(count) = badge {
            if count > 0 {
                let badge_text = if count > 99 { "99+".to_string() } else { count.to_string() };
                let badge_galley = ui.painter().layout_no_wrap(
                    badge_text,
                    FontId::proportional(10.0),
                    Color32::WHITE,
                );
                let badge_w = badge_galley.size().x + 12.0;
                let badge_h = 16.0;
                let badge_rect = Rect::from_center_size(
                    Pos2::new(rect.right() - badge_w / 2.0 - 4.0, rect.center().y),
                    Vec2::new(badge_w, badge_h),
                );
                ui.painter().rect_filled(badge_rect, CornerRadius::same(10), style::ACCENT);
                ui.painter().galley(
                    Pos2::new(badge_rect.center().x - badge_galley.size().x / 2.0,
                              badge_rect.center().y - badge_galley.size().y / 2.0),
                    badge_galley,
                    Color32::WHITE,
                );
            }
        }
    }

    if response.clicked() {
        app.page = page;
    }
}
```

**Step 3: Add `inbox_unread_count` method** to whatever type `app.state` is (check `types.rs` / `main.rs`). If the state struct doesn't have it:

```rust
// In the HootState impl (main.rs or types.rs):
pub fn inbox_unread_count(&self) -> Option<usize> {
    let count = self.messages.iter().filter(|m| !m.read).count();
    if count > 0 { Some(count) } else { None }
}
```

Adjust field names to match what actually exists in the codebase.

**Step 4: Build and inspect**

```bash
cargo build && cargo run
```
Visually inspect: sidebar should be 232px wide, warm surface color, app name in 17px, compose button in purple with rounded corners, nav items with active left border highlight.

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: redesign sidebar with design system tokens"
```

---

## Task 4: Toolbar (Search Bar)

**Files:**
- Modify: `src/ui/search.rs`

The toolbar sits above the message list. It contains:
- Search input (flex 1, max-width 400): bg=SURFACE2, border=BORDER_STRONG, radius=8, focus ring in ACCENT
- Icon buttons on the right: 30×30, SURFACE2 bg, BORDER_STRONG border, radius=7

**Step 1: Rewrite `src/ui/search.rs`**

```rust
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};
use crate::style;

pub struct SearchBar {
    pub query: String,
}

impl SearchBar {
    pub fn new() -> Self { Self { query: String::new() } }
}

pub fn render_toolbar(ui: &mut egui::Ui, search_query: &mut String, on_search: impl FnOnce(&str)) {
    // Toolbar frame
    egui::Frame::none()
        .fill(style::SURFACE)
        .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 10.0, bottom: 10.0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;

                // Search input wrapper — takes available width up to 400px
                let search_width = (ui.available_width() - 80.0).min(400.0);
                let search_frame = egui::Frame::none()
                    .fill(style::SURFACE2)
                    .stroke(Stroke::new(1.0, style::BORDER_STRONG))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin { left: 32.0, right: 12.0, top: 7.0, bottom: 7.0 });

                let inner = search_frame.show(ui, |ui| {
                    ui.set_width(search_width);
                    let response = ui.add(
                        egui::TextEdit::singleline(search_query)
                            .hint_text("Search messages…")
                            .frame(false)
                            .desired_width(search_width)
                            .font(egui::FontId::proportional(13.0))
                            .text_color(style::TEXT),
                    );
                    if response.changed() {
                        on_search(search_query);
                    }
                    response
                });

                // Draw search icon (🔍) inside left padding
                let frame_rect = inner.response.rect;
                ui.painter().text(
                    egui::Pos2::new(frame_rect.left() + 10.0, frame_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "🔍",
                    egui::FontId::proportional(12.0),
                    style::TEXT3,
                );

                // Focus ring
                let text_response = inner.inner;
                if text_response.has_focus() {
                    ui.painter().rect_stroke(
                        frame_rect.expand(3.0),
                        CornerRadius::same(11),
                        Stroke::new(3.0, egui::Color32::from_rgba_unmultiplied(124, 58, 237, 20)),
                    );
                }

                // Right-side icon buttons
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    icon_button(ui, "⟳");
                    icon_button(ui, "☰");
                });
            });
        });

    // Bottom border
    let cursor = ui.cursor();
    ui.painter().line_segment(
        [
            egui::Pos2::new(0.0, cursor.top()),
            egui::Pos2::new(ui.max_rect().right(), cursor.top()),
        ],
        Stroke::new(1.0, style::BORDER),
    );
}

fn icon_button(ui: &mut egui::Ui, icon: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(icon).color(style::TEXT2))
            .fill(style::SURFACE2)
            .stroke(Stroke::new(1.0, style::BORDER_STRONG))
            .corner_radius(CornerRadius::same(7))
            .min_size(Vec2::splat(30.0)),
    )
}
```

**Step 2: Update caller in `main.rs`**

In `render_app`, find where the search bar is rendered and update the call to use the new `render_toolbar` signature.

**Step 3: Build**

```bash
cargo build 2>&1 | grep "^error" | head -20
```

**Step 4: Commit**

```bash
git add src/ui/search.rs src/main.rs
git commit -m "feat: redesign toolbar/search bar"
```

---

## Task 5: Inbox Message List

**Files:**
- Modify: `src/ui/inbox.rs` (complete rewrite)

Each message row has:
- Unread dot (5px, ACCENT, left edge, only if unread)
- Avatar (34×34, radius 9, gradient color by initials)
- Column: sender (13.5px, weight 500 if unread / TEXT2 weight 400 if read), time (11.5px TEXT3)
- Column: subject (13px, TEXT if unread / TEXT2 if read), preview (12.5px TEXT3)
- Footer row: protocol badges (nostr/pgp/smtp)
- Row hover: SURFACE bg + shadow_sm; selected: SURFACE bg + shadow_md

**Step 1: Rewrite `src/ui/inbox.rs`**

```rust
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, Vec2};
use crate::{style, Hoot, Page};
use crate::db::queries::ThreadSummary; // adjust import to actual type

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    // Container scrolls vertically, 8px padding
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.add_space(8.0);

            if app.state.messages.is_empty() {
                // Empty state
                ui.add_space(80.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("No messages yet.").size(14.0).color(style::TEXT2)
                    );
                    if let Some(acct) = app.account_manager.accounts.first() {
                        if let Some(npub) = acct.public_key().and_then(|k| k.to_bech32().ok()) {
                            ui.label(
                                RichText::new(format!("Your address is {}", npub))
                                    .size(13.0)
                                    .color(style::TEXT3),
                            );
                        }
                    }
                });
                return;
            }

            for (idx, msg) in app.state.messages.clone().iter().enumerate() {
                let is_unread = !msg.read;
                let is_selected = app.state.selected_message == Some(idx);

                render_message_row(ui, app, msg, is_unread, is_selected, idx);
                ui.add_space(2.0);
            }

            ui.add_space(8.0);
        });
}

fn render_message_row(
    ui: &mut egui::Ui,
    app: &mut Hoot,
    msg: &MessageSummary, // use actual type
    is_unread: bool,
    is_selected: bool,
    idx: usize,
) {
    use eframe::egui::*;

    let row_height = 72.0;
    let available_width = ui.available_width() - 16.0; // 8px padding each side
    let desired_size = Vec2::new(available_width, row_height);

    let (outer_rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(outer_rect) {
        // Background
        let fill = if is_selected {
            style::SURFACE
        } else if response.hovered() {
            style::SURFACE
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(outer_rect, CornerRadius::same(10), fill);

        // Shadow on hover/selected
        if response.hovered() || is_selected {
            let shadow = if is_selected { style::shadow_md() } else { style::shadow_sm() };
            // egui shadows are painted via Frame; approximate by painting a slightly darker rect
            // For proper shadow we render via Frame in a child_ui:
            ui.painter().rect_stroke(
                outer_rect,
                CornerRadius::same(10),
                Stroke::new(1.0, style::BORDER),
            );
        }

        // Unread dot — 5px at left edge
        if is_unread {
            let dot_x = outer_rect.left() + 3.0;
            let dot_y = outer_rect.center().y;
            ui.painter().circle_filled(Pos2::new(dot_x, dot_y), 2.5, style::ACCENT);
        }

        // Inner layout: 14px left padding, 12px gap, then avatar, 12px gap, then text columns
        let inner_left = outer_rect.left() + 14.0;
        let avatar_rect = Rect::from_min_size(
            Pos2::new(inner_left, outer_rect.top() + (row_height - 34.0) / 2.0),
            Vec2::splat(34.0),
        );

        // Render avatar
        let initials = initials_for(msg.sender_display_name());
        let (av_bg, av_fg) = style::avatar_color_for_initials(&initials);
        ui.painter().rect_filled(avatar_rect, CornerRadius::same(9), av_bg);
        ui.painter().rect_stroke(avatar_rect, CornerRadius::same(9), Stroke::new(1.0, style::BORDER_STRONG));
        ui.painter().text(
            avatar_rect.center(),
            Align2::CENTER_CENTER,
            &initials,
            FontId::proportional(12.0),
            av_fg,
        );

        // Text area starts 12px after avatar
        let text_left = avatar_rect.right() + 12.0;
        let text_right = outer_rect.right() - 14.0;

        // Row 1: Sender name (left) + timestamp (right)
        let row1_y = outer_rect.top() + 13.0;
        let sender_color = if is_unread { style::TEXT } else { style::TEXT2 };
        let time_text = style::format_timestamp(msg.timestamp);

        // Time — right-aligned
        let time_galley = ui.painter().layout_no_wrap(
            time_text,
            FontId::proportional(11.5),
            style::TEXT3,
        );
        let time_x = text_right - time_galley.size().x;
        ui.painter().galley(Pos2::new(time_x, row1_y), time_galley, style::TEXT3);

        // Sender name — truncated to fit before time
        let sender_max_w = time_x - text_left - 12.0;
        let sender_galley = ui.painter().layout(
            msg.sender_display_name().to_string(),
            FontId::proportional(13.5),
            sender_color,
            sender_max_w,
        );
        ui.painter().galley(Pos2::new(text_left, row1_y), sender_galley, sender_color);

        // Row 2: Subject
        let row2_y = row1_y + 18.0;
        let subject_color = if is_unread { style::TEXT } else { style::TEXT2 };
        let subject_galley = ui.painter().layout(
            msg.subject.clone(),
            FontId::proportional(13.0),
            subject_color,
            text_right - text_left,
        );
        ui.painter().galley(Pos2::new(text_left, row2_y), subject_galley, subject_color);

        // Row 3: Preview
        let row3_y = row2_y + 17.0;
        let preview_galley = ui.painter().layout(
            msg.preview.clone(),
            FontId::proportional(12.5),
            style::TEXT3,
            text_right - text_left,
        );
        ui.painter().galley(Pos2::new(text_left, row3_y), preview_galley, style::TEXT3);

        // Row 4: Badges (at bottom of row)
        // For now: render nostr badge if nostr, etc.
        // Badges are small — place at row3_y + 16 if space, or skip for brevity
    }

    if response.clicked() {
        app.state.selected_message = Some(idx);
        app.page = Page::Post;
        // set app.state.current_thread_id etc.
    }
}

fn initials_for(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    match words.len() {
        0 => "?".to_string(),
        1 => words[0].chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or("?".to_string()),
        _ => {
            let first = words[0].chars().next().unwrap_or('?');
            let last = words[words.len()-1].chars().next().unwrap_or('?');
            format!("{}{}", first.to_uppercase(), last.to_uppercase())
        }
    }
}
```

**Important:** `MessageSummary` is a placeholder — use the actual struct/type that `app.state.messages` holds. Check `types.rs` and `db/queries.rs` for the real type name and field names, then adjust field accesses accordingly (`.sender_display_name()`, `.subject`, `.preview`, `.timestamp`, `.read`).

**Step 2: Build and fix field names**

```bash
cargo build 2>&1 | grep "^error" | head -30
```

**Step 3: Add status bar below inbox**

In `main.rs` `render_app`, after rendering `Page::Inbox`, add a `TopBottomPanel::bottom` for the status bar:

```rust
egui::TopBottomPanel::bottom("status_bar")
    .frame(
        egui::Frame::none()
            .fill(style::SURFACE)
            .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 6.0, bottom: 6.0 })
    )
    .show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;

            let relay_count = app.relay_pool.connected_count();
            status_item(ui, &relay_count.to_string(), "relays");

            // Latency placeholder
            status_item(ui, "—", "latency");
        });
    });
```

```rust
fn status_item(ui: &mut egui::Ui, value: &str, label: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new(value).size(11.0).color(style::TEXT2).strong());
        ui.label(egui::RichText::new(label).size(11.0).color(style::TEXT3));
    });
}
```

**Step 4: Commit**

```bash
git add src/ui/inbox.rs src/main.rs
git commit -m "feat: redesign inbox message list with custom row renderer"
```

---

## Task 6: Thread View (Message Reading)

**Files:**
- Modify: `src/ui/thread_view.rs`

Design spec: Full-screen message view. Top bar with back arrow, sender name + address, timestamp right, action icons far right. Message body max-width ~640px centered, 15px text, line-height 1.6. Reply compose area below.

**Step 1: Rewrite `src/ui/thread_view.rs`**

```rust
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};
use crate::{style, Hoot, Page};

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    // ── Top bar ──────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(style::SURFACE)
        .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 12.0, bottom: 12.0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Back button
                if ui.add(
                    egui::Button::new(RichText::new("← Back").size(13.0).color(style::ACCENT))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE),
                ).clicked() {
                    app.page = Page::Inbox;
                }

                ui.separator();

                // Sender info
                if let Some(thread) = &app.state.current_thread {
                    if let Some(first) = thread.messages.first() {
                        ui.label(
                            RichText::new(&first.sender_display_name)
                                .size(13.5)
                                .color(style::TEXT)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(&first.sender_npub_short)
                                .size(12.0)
                                .color(style::TEXT3),
                        );
                    }
                }

                // Timestamp + actions right-aligned
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Action buttons
                    icon_button_sm(ui, "🗑");
                    icon_button_sm(ui, "📁");
                    icon_button_sm(ui, "↩");

                    // Timestamp
                    if let Some(thread) = &app.state.current_thread {
                        if let Some(first) = thread.messages.first() {
                            ui.label(
                                RichText::new(style::format_timestamp(first.timestamp))
                                    .size(11.5)
                                    .color(style::TEXT3),
                            );
                        }
                    }
                });
            });
        });

    // Border under top bar
    let y = ui.cursor().top();
    ui.painter().line_segment(
        [egui::Pos2::new(0.0, y), egui::Pos2::new(ui.max_rect().right(), y)],
        Stroke::new(1.0, style::BORDER),
    );

    // ── Scrollable message body ───────────────────────────────────────────────
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // Centered container max 640px
            let avail = ui.available_width();
            let body_width = avail.min(640.0);
            let h_margin = (avail - body_width) / 2.0;

            egui::Frame::none()
                .inner_margin(egui::Margin {
                    left: h_margin + 24.0,
                    right: h_margin + 24.0,
                    top: 24.0,
                    bottom: 24.0,
                })
                .show(ui, |ui| {
                    if let Some(thread) = &app.state.current_thread {
                        for msg in &thread.messages {
                            // Subject (first message only)
                            if msg.is_first {
                                ui.label(
                                    RichText::new(&msg.subject)
                                        .size(17.0)
                                        .color(style::TEXT)
                                        .strong(),
                                );
                                ui.add_space(8.0);
                            }

                            // Sender info line
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&msg.sender_display_name)
                                        .size(13.5)
                                        .color(style::TEXT)
                                        .strong(),
                                );
                                ui.label(
                                    RichText::new(&msg.sender_npub_short)
                                        .size(12.0)
                                        .color(style::TEXT2),
                                );
                            });
                            ui.add_space(12.0);

                            // Body
                            ui.label(
                                RichText::new(&msg.body)
                                    .size(15.0)
                                    .color(style::TEXT),
                            );
                            ui.add_space(24.0);

                            // Divider between messages
                            ui.add(egui::Separator::default());
                            ui.add_space(16.0);
                        }
                    }

                    // ── Reply compose area ──────────────────────────────────
                    render_reply_area(ui, app);
                });
        });
}

fn render_reply_area(ui: &mut egui::Ui, app: &mut Hoot) {
    egui::Frame::none()
        .fill(style::SURFACE)
        .stroke(Stroke::new(1.0, style::BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            ui.add(
                egui::TextEdit::multiline(&mut app.state.reply_draft)
                    .hint_text("Reply…")
                    .frame(false)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::proportional(14.0))
                    .text_color(style::TEXT),
            );

            ui.add_space(8.0);

            // Bottom row: Send right
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_send = !app.state.reply_draft.is_empty();
                let send_btn = ui.add_enabled(
                    can_send,
                    egui::Button::new(RichText::new("Send").size(13.5).color(Color32::WHITE))
                        .fill(style::ACCENT)
                        .corner_radius(CornerRadius::same(8))
                        .min_size(Vec2::new(72.0, 32.0)),
                );
                if send_btn.clicked() {
                    // TODO: send reply
                    app.state.reply_draft.clear();
                }
            });
        });
}

fn icon_button_sm(ui: &mut egui::Ui, icon: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(icon).size(14.0).color(style::TEXT2))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, style::BORDER_STRONG))
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::splat(28.0)),
    )
}
```

**Step 2: Add `reply_draft: String` to `HootState`** if it doesn't exist. Search with:

```bash
grep -n "reply_draft\|HootState" src/main.rs src/types.rs
```

Add the field where the state struct is defined.

**Step 3: Build**

```bash
cargo build 2>&1 | grep "^error" | head -20
```

**Step 4: Commit**

```bash
git add src/ui/thread_view.rs
git commit -m "feat: redesign thread/message view"
```

---

## Task 7: Compose Window (Full-Screen)

**Files:**
- Modify: `src/ui/compose_window.rs`

Design: Full-screen panel (not a floating modal). Top bar: discard left, "New Message" center, Send right. Fields use underline-style inputs. Large open textarea for body.

**Step 1: Rewrite `src/ui/compose_window.rs`**

```rust
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};
use crate::{style, Hoot, Page};

pub struct ComposeWindowState {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub id: egui::Id,
}

impl Default for ComposeWindowState {
    fn default() -> Self {
        Self {
            to: String::new(),
            subject: String::new(),
            body: String::new(),
            id: egui::Id::new(uuid::Uuid::new_v4().to_string()),
        }
    }
}

pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    // We render compose as a full central-panel page, not a floating window
    let state = match app.state.compose_state.as_mut() {
        Some(s) => s,
        None => return,
    };

    let can_send = !state.to.is_empty() && !state.subject.is_empty();

    // ── Top bar ──────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(style::SURFACE)
        .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 12.0, bottom: 12.0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Discard
                if ui.add(
                    egui::Button::new(RichText::new("Discard").size(13.0).color(style::TEXT2))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE),
                ).clicked() {
                    app.state.compose_state = None;
                    app.page = Page::Inbox;
                }

                // Center title
                ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                    ui.label(RichText::new("New Message").size(14.0).strong().color(style::TEXT));
                });

                // Send button
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let send = ui.add_enabled(
                        can_send,
                        egui::Button::new(RichText::new("Send").size(13.5).color(Color32::WHITE))
                            .fill(if can_send { style::ACCENT } else { style::SURFACE2 })
                            .corner_radius(CornerRadius::same(8))
                            .min_size(Vec2::new(64.0, 30.0)),
                    );
                    if send.clicked() {
                        // TODO: send message
                        app.state.compose_state = None;
                        app.page = Page::Inbox;
                    }
                });
            });
        });

    // Border
    let y = ui.cursor().top();
    ui.painter().line_segment(
        [egui::Pos2::new(0.0, y), egui::Pos2::new(ui.max_rect().right(), y)],
        Stroke::new(1.0, style::BORDER),
    );

    // ── Fields ───────────────────────────────────────────────────────────────
    let field_w = ui.available_width().min(640.0);
    let h_pad = (ui.available_width() - field_w) / 2.0;

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Frame::none()
                .inner_margin(egui::Margin {
                    left: h_pad + 24.0,
                    right: h_pad + 24.0,
                    top: 24.0,
                    bottom: 24.0,
                })
                .show(ui, |ui| {
                    ui.set_min_width(field_w);

                    // To field — underline style
                    field_label(ui, "To");
                    style::underline_text_edit(ui, &mut state.to, "npub… or email…", 13.5);
                    ui.add_space(16.0);

                    // Subject field — underline style
                    field_label(ui, "Subject");
                    style::underline_text_edit(ui, &mut state.subject, "Subject", 13.5);
                    ui.add_space(24.0);

                    // Body — open textarea
                    ui.add(
                        egui::TextEdit::multiline(&mut state.body)
                            .hint_text("Write your message…")
                            .frame(false)
                            .desired_rows(20)
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::proportional(14.0))
                            .text_color(style::TEXT),
                    );
                });
        });
}

fn field_label(ui: &mut egui::Ui, label: &str) {
    ui.label(RichText::new(label).size(11.0).strong().color(style::TEXT3));
    ui.add_space(2.0);
}
```

**Step 2: Update `main.rs`** — compose should now render as a full-screen page, not a floating window. In the `render_app` match:

```rust
Page::Compose => ui::compose_window::render(app, ui),
```

Add `Compose` to `Page` enum in `types.rs` if not present:

```rust
Compose,  // Full-screen compose view
```

And the compose button in the sidebar should do:
```rust
app.page = Page::Compose;
app.state.compose_state = Some(ComposeWindowState::default());
```

**Step 3: Build**

```bash
cargo build 2>&1 | grep "^error" | head -20
```

**Step 4: Commit**

```bash
git add src/ui/compose_window.rs src/types.rs src/main.rs
git commit -m "feat: redesign compose as full-screen page with underline inputs"
```

---

## Task 8: Settings Page

**Files:**
- Modify: `src/ui/settings.rs`

Design: Two-column layout: left settings sidebar (sections), right content. Every setting has a one-line description in TEXT3 below it.

**Step 1: Rewrite `src/ui/settings.rs`**

```rust
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, Vec2};
use crate::{style, Hoot, Page};

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SettingsSection {
    #[default]
    Identity,
    Relays,
    SmtpBridge,
    Filters,
    Notifications,
    About,
}

pub struct SettingsScreen;

impl SettingsScreen {
    pub fn ui(app: &mut Hoot, ui: &mut egui::Ui) {
        // Top bar with back
        egui::Frame::none()
            .fill(style::SURFACE)
            .inner_margin(egui::Margin { left: 16.0, right: 16.0, top: 12.0, bottom: 12.0 })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.add(
                        egui::Button::new(RichText::new("← Inbox").size(13.0).color(style::ACCENT))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                    ).clicked() {
                        app.page = Page::Inbox;
                    }
                    ui.label(RichText::new("Settings").size(15.0).strong().color(style::TEXT));
                });
            });

        let y = ui.cursor().top();
        ui.painter().line_segment(
            [egui::Pos2::new(0.0, y), egui::Pos2::new(ui.max_rect().right(), y)],
            Stroke::new(1.0, style::BORDER),
        );

        // Two-column layout
        ui.horizontal(|ui| {
            // Left: settings navigation (160px)
            egui::SidePanel::left("settings_nav")
                .exact_width(160.0)
                .frame(egui::Frame::none().fill(style::SURFACE)
                    .inner_margin(egui::Margin::symmetric(8.0, 8.0)))
                .show_inside(ui, |ui| {
                    settings_nav_item(ui, app, "Identity", SettingsSection::Identity);
                    settings_nav_item(ui, app, "Relays", SettingsSection::Relays);
                    settings_nav_item(ui, app, "SMTP Bridge", SettingsSection::SmtpBridge);
                    settings_nav_item(ui, app, "Spam & Filters", SettingsSection::Filters);
                    settings_nav_item(ui, app, "Notifications", SettingsSection::Notifications);

                    ui.add(egui::Separator::default());

                    settings_nav_item(ui, app, "About", SettingsSection::About);
                });

            // Right: section content
            egui::CentralPanel::default()
                .frame(egui::Frame::none()
                    .fill(style::BG)
                    .inner_margin(egui::Margin::same(24.0)))
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        match &app.state.settings_section {
                            SettingsSection::Identity => render_identity(app, ui),
                            SettingsSection::Relays => render_relays(app, ui),
                            SettingsSection::SmtpBridge => render_smtp(app, ui),
                            SettingsSection::Filters => render_filters(app, ui),
                            SettingsSection::Notifications => render_notifications(app, ui),
                            SettingsSection::About => render_about(ui),
                        }
                    });
                });
        });
    }
}

fn settings_nav_item(ui: &mut egui::Ui, app: &mut Hoot, label: &str, section: SettingsSection) {
    let is_active = app.state.settings_section == section;
    let desired = Vec2::new(ui.available_width(), 30.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let fill = if is_active { style::ACCENT_SOFT } else if response.hovered() { style::SURFACE2 } else { Color32::TRANSPARENT };
        ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
        if is_active {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height())),
                CornerRadius::same(1), style::ACCENT,
            );
        }
        let color = if is_active { style::ACCENT } else if response.hovered() { style::TEXT } else { style::TEXT2 };
        ui.painter().text(
            egui::Pos2::new(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            color,
        );
    }
    if response.clicked() {
        app.state.settings_section = section;
    }
}

fn section_heading(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(16.0).strong().color(style::TEXT));
    ui.add_space(16.0);
}

fn setting_row(ui: &mut egui::Ui, label: &str, description: &str, widget: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(200.0);
            ui.label(RichText::new(label).size(13.5).color(style::TEXT));
            ui.label(RichText::new(description).size(12.0).color(style::TEXT3));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), widget);
    });
    ui.add_space(16.0);
    ui.add(egui::Separator::default());
    ui.add_space(16.0);
}

fn render_identity(app: &mut Hoot, ui: &mut egui::Ui) {
    section_heading(ui, "Identity");

    // Display name
    setting_row(ui, "Display name", "Shown to people you message.", |ui| {
        ui.add(egui::TextEdit::singleline(&mut app.state.profile_display_name)
            .desired_width(220.0));
    });

    // npub — copyable
    if let Some(acct) = app.account_manager.accounts.first() {
        if let Some(npub) = acct.public_key().and_then(|k| k.to_bech32().ok()) {
            setting_row(ui, "Your address (npub)", "Share this with people who want to message you.", |ui| {
                if ui.add(
                    egui::Button::new(RichText::new("Copy").size(12.0).color(style::ACCENT))
                        .fill(style::ACCENT_SOFT)
                        .corner_radius(CornerRadius::same(6))
                ).clicked() {
                    ui.ctx().copy_text(npub.clone());
                }
                ui.add_space(8.0);
                ui.label(RichText::new(&npub[..16]).size(12.0).color(style::TEXT3));
            });
        }
    }

    // nsec — hidden by default
    setting_row(ui, "Private key (nsec)", "Never share this. It controls your identity.", |ui| {
        if ui.add(
            egui::Button::new(RichText::new("Reveal").size(12.0).color(style::TEXT2))
                .fill(style::SURFACE2)
                .corner_radius(CornerRadius::same(6))
        ).clicked() {
            app.state.show_nsec = !app.state.show_nsec;
        }
    });

    if app.state.show_nsec {
        // Amber warning box
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(254, 243, 199)) // #fef3c7
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.label(RichText::new("This is your private key. If you lose it, your account cannot be recovered. Write it down somewhere safe.").size(13.0).color(egui::Color32::from_rgb(146, 64, 14)));
                ui.add_space(8.0);
                // show nsec here
            });
    }
}

fn render_relays(app: &mut Hoot, ui: &mut egui::Ui) {
    section_heading(ui, "Relays");

    for relay in &app.relay_pool.relays {
        ui.horizontal(|ui| {
            // Status dot
            let dot_color = if relay.is_connected() { style::GREEN } else { style::AMBER };
            let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(7.0), egui::Sense::hover());
            ui.painter().circle_filled(dot_rect.center(), 3.5, dot_color);

            ui.label(RichText::new(relay.url()).size(13.0).color(style::TEXT));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(
                    egui::Button::new(RichText::new("Remove").size(12.0).color(style::TEXT2))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE),
                ).clicked() {
                    // TODO: remove relay
                }
            });
        });
        ui.add_space(8.0);
        ui.add(egui::Separator::default());
        ui.add_space(8.0);
    }

    // Add relay input
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.state.new_relay_url)
                .hint_text("wss://relay.example.com")
                .desired_width(300.0),
        );
        if ui.add(
            egui::Button::new(RichText::new("Add").color(style::ACCENT))
                .fill(style::ACCENT_SOFT)
                .corner_radius(CornerRadius::same(6)),
        ).clicked() {
            // TODO: add relay
        }
    });
}

fn render_smtp(app: &mut Hoot, ui: &mut egui::Ui) {
    section_heading(ui, "SMTP Bridge");

    ui.label(RichText::new("Send and receive from regular email addresses. Messages via bridge are not end-to-end encrypted.").size(13.0).color(style::TEXT2));
    ui.add_space(16.0);

    setting_row(ui, "Enable SMTP bridge", "Connect to a standard email server.", |ui| {
        ui.checkbox(&mut app.state.smtp_enabled, "");
    });

    if app.state.smtp_enabled {
        // SMTP fields
        for (label, val, hint) in [
            ("SMTP Host", &mut app.state.smtp_host, "smtp.example.com"),
            ("SMTP Port", &mut app.state.smtp_port, "587"),
            ("Username", &mut app.state.smtp_user, "you@example.com"),
        ] {
            setting_row(ui, label, "", |ui| {
                ui.add(egui::TextEdit::singleline(val).hint_text(hint).desired_width(220.0));
            });
        }
    }
}

fn render_filters(app: &mut Hoot, ui: &mut egui::Ui) {
    section_heading(ui, "Spam & Filters");
    // Placeholder
    ui.label(RichText::new("Spam filter settings coming soon.").size(13.0).color(style::TEXT2));
}

fn render_notifications(app: &mut Hoot, ui: &mut egui::Ui) {
    section_heading(ui, "Notifications");
    // Placeholder
    ui.label(RichText::new("Notification settings coming soon.").size(13.0).color(style::TEXT2));
}

fn render_about(ui: &mut egui::Ui) {
    section_heading(ui, "About");
    ui.label(RichText::new(format!("Hoot v{}", env!("CARGO_PKG_VERSION"))).size(13.0).color(style::TEXT));
    ui.add_space(8.0);
    ui.label(RichText::new("Built on the Nostr protocol.").size(13.0).color(style::TEXT2));
}
```

**Step 2: Add required state fields** to `HootState`:

```rust
pub settings_section: SettingsSection,
pub show_nsec: bool,
pub new_relay_url: String,
pub smtp_enabled: bool,
pub smtp_host: String,
pub smtp_port: String,
pub smtp_user: String,
pub smtp_password: String,
pub profile_display_name: String,
```

**Step 3: Build and fix**

```bash
cargo build 2>&1 | grep "^error" | head -30
```

**Step 4: Commit**

```bash
git add src/ui/settings.rs
git commit -m "feat: redesign settings page with sections and descriptions"
```

---

## Task 9: Onboarding

**Files:**
- Modify: `src/ui/onboarding.rs`

5-screen flow:
1. Welcome: wordmark + "Email that belongs to you." + two options
2a. Key Generation: show npub/nsec with amber warning, checkbox to continue
2b. Import Key: paste nsec input
3. Add Relay: pre-populated relay list
4. SMTP Bridge: optional toggle
5. Ready: show npub + "Open Hoot"

**Step 1: Rewrite `src/ui/onboarding.rs`**

```rust
use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Vec2};
use crate::{style, Hoot, Page};

pub struct OnboardingScreen;

impl OnboardingScreen {
    pub fn ui(app: &mut Hoot, ui: &mut egui::Ui) {
        // Full centered layout
        let avail = ui.available_size();
        egui::Frame::none()
            .fill(style::BG)
            .inner_margin(egui::Margin::same(0.0))
            .show(ui, |ui| {
                ui.set_min_size(avail);
                ui.vertical_centered(|ui| {
                    ui.add_space(avail.y * 0.15);

                    match app.page {
                        Page::Onboarding => render_welcome(app, ui),
                        Page::OnboardingNewUser => render_key_gen(app, ui),
                        Page::OnboardingNewShowKey => render_show_key(app, ui),
                        Page::OnboardingReturning => render_import_key(app, ui),
                        Page::OnboardingRelay => render_relays(app, ui),
                        Page::OnboardingSmtp => render_smtp(app, ui),
                        Page::OnboardingReady => render_ready(app, ui),
                        _ => {}
                    }

                    // Progress dots
                    ui.add_space(32.0);
                    render_progress_dots(ui, app);
                });
            });
    }
}

fn render_welcome(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(RichText::new("Hoot").size(34.0).strong().color(style::TEXT));
    ui.add_space(12.0);
    ui.label(RichText::new("Email that belongs to you.").size(16.0).color(style::TEXT2));
    ui.add_space(40.0);

    let btn_size = Vec2::new(240.0, 44.0);

    if ui.add(
        egui::Button::new(RichText::new("Create new identity").size(14.0).color(Color32::WHITE))
            .fill(style::ACCENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(btn_size),
    ).clicked() {
        app.page = Page::OnboardingNewUser;
        // generate keys
    }

    ui.add_space(12.0);

    if ui.add(
        egui::Button::new(RichText::new("I have a key").size(14.0).color(style::TEXT))
            .fill(style::SURFACE)
            .stroke(egui::Stroke::new(1.0, style::BORDER_STRONG))
            .corner_radius(CornerRadius::same(10))
            .min_size(btn_size),
    ).clicked() {
        app.page = Page::OnboardingReturning;
    }
}

fn render_key_gen(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(RichText::new("Generating your keys…").size(20.0).strong().color(style::TEXT));
    ui.add_space(12.0);
    ui.label(RichText::new("This only takes a moment.").size(14.0).color(style::TEXT2));
    ui.add_space(32.0);
    ui.spinner();
    // After generation completes, auto-advance to OnboardingNewShowKey
}

fn render_show_key(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(RichText::new("Your identity is ready.").size(20.0).strong().color(style::TEXT));
    ui.add_space(24.0);

    // npub — shareable
    ui.label(RichText::new("Your address (share this)").size(12.0).strong().color(style::TEXT3));
    ui.add_space(4.0);
    if let Some(npub) = &app.state.onboarding_npub {
        ui.add(egui::TextEdit::singleline(&mut npub.clone())
            .interactive(false)
            .desired_width(360.0)
            .font(egui::FontId::monospace(11.0)));
    }
    ui.add_space(24.0);

    // nsec — amber warning
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(254, 243, 199))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_max_width(360.0);
            ui.label(RichText::new("Private key (nsec) — never share").size(12.0).strong().color(egui::Color32::from_rgb(146, 64, 14)));
            ui.add_space(8.0);
            if let Some(nsec) = &app.state.onboarding_nsec {
                ui.add(egui::TextEdit::singleline(&mut nsec.clone())
                    .interactive(false)
                    .desired_width(328.0)
                    .font(egui::FontId::monospace(11.0)));
            }
            ui.add_space(8.0);
            ui.label(RichText::new("This is your private key. If you lose it, your account cannot be recovered. Write it down somewhere safe.").size(12.0).color(egui::Color32::from_rgb(146, 64, 14)));
        });

    ui.add_space(24.0);

    ui.checkbox(&mut app.state.onboarding_key_saved, "I've saved my private key somewhere safe");
    ui.add_space(16.0);

    if ui.add_enabled(
        app.state.onboarding_key_saved,
        egui::Button::new(RichText::new("Continue").size(14.0).color(Color32::WHITE))
            .fill(style::ACCENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(240.0, 44.0)),
    ).clicked() {
        app.page = Page::OnboardingRelay;
    }
}

fn render_import_key(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(RichText::new("Enter your private key").size(20.0).strong().color(style::TEXT));
    ui.add_space(8.0);
    ui.label(RichText::new("Paste your nsec or upload a keyfile.").size(14.0).color(style::TEXT2));
    ui.add_space(32.0);

    let input = ui.add(
        egui::TextEdit::singleline(&mut app.state.onboarding_nsec_input)
            .hint_text("nsec1…")
            .desired_width(360.0)
            .password(true),
    );

    // Validate and show npub preview
    if !app.state.onboarding_nsec_input.is_empty() {
        if let Some(npub) = &app.state.onboarding_derived_npub {
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Address: {}", npub)).size(12.0).color(style::GREEN));
        }
    }

    ui.add_space(24.0);

    let valid = app.state.onboarding_derived_npub.is_some();
    if ui.add_enabled(
        valid,
        egui::Button::new(RichText::new("Continue").size(14.0).color(Color32::WHITE))
            .fill(style::ACCENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(240.0, 44.0)),
    ).clicked() {
        app.page = Page::OnboardingRelay;
    }
}

fn render_relays(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(RichText::new("Add a relay").size(20.0).strong().color(style::TEXT));
    ui.add_space(8.0);
    ui.label(RichText::new("Relays are servers that deliver your messages. Add at least one.").size(14.0).color(style::TEXT2));
    ui.add_space(32.0);

    for (i, relay) in app.state.onboarding_relays.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(relay).size(13.0).color(style::TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(
                    egui::Button::new(RichText::new("Remove").size(12.0).color(style::TEXT3))
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                ).clicked() {
                    // remove
                }
            });
        });
    }

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.state.onboarding_relay_input)
                .hint_text("wss://relay.damus.io")
                .desired_width(280.0),
        );
        if ui.add(
            egui::Button::new(RichText::new("Add").color(style::ACCENT))
                .fill(style::ACCENT_SOFT)
                .corner_radius(CornerRadius::same(6)),
        ).clicked() {
            // add relay
        }
    });

    ui.add_space(8.0);
    if app.state.onboarding_relays.is_empty() {
        ui.label(RichText::new("Without a relay you won't receive messages.").size(12.0).color(style::AMBER));
    }

    ui.add_space(24.0);
    if ui.add(
        egui::Button::new(RichText::new("Continue").size(14.0).color(Color32::WHITE))
            .fill(style::ACCENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(240.0, 44.0)),
    ).clicked() {
        app.page = Page::OnboardingSmtp;
    }

    if ui.add(
        egui::Button::new(RichText::new("Skip for now").size(13.0).color(style::TEXT3))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE),
    ).clicked() {
        app.page = Page::OnboardingSmtp;
    }
}

fn render_smtp(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(RichText::new("Connect to regular email?").size(20.0).strong().color(style::TEXT));
    ui.add_space(8.0);
    ui.label(RichText::new("Send and receive from standard email addresses.\nMessages via this bridge are not end-to-end encrypted.").size(14.0).color(style::TEXT2));
    ui.add_space(32.0);

    ui.checkbox(&mut app.state.smtp_enabled, "Enable SMTP bridge");

    ui.add_space(24.0);

    if ui.add(
        egui::Button::new(RichText::new("Continue").size(14.0).color(Color32::WHITE))
            .fill(style::ACCENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(240.0, 44.0)),
    ).clicked() {
        app.page = Page::OnboardingReady;
    }
}

fn render_ready(app: &mut Hoot, ui: &mut egui::Ui) {
    ui.label(RichText::new("You're set up.").size(24.0).strong().color(style::TEXT));
    ui.add_space(8.0);
    ui.label(RichText::new("Your inbox is waiting.").size(16.0).color(style::TEXT2));

    ui.add_space(16.0);
    if let Some(npub) = &app.state.onboarding_npub {
        ui.label(RichText::new(npub).size(11.0).color(style::TEXT3));
    }

    ui.add_space(32.0);

    if ui.add(
        egui::Button::new(RichText::new("Open Hoot").size(15.0).color(Color32::WHITE))
            .fill(style::ACCENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(240.0, 48.0)),
    ).clicked() {
        app.page = Page::Inbox;
    }
}

fn render_progress_dots(ui: &mut egui::Ui, app: &Hoot) {
    let screens = [
        Page::Onboarding,
        Page::OnboardingNewUser,
        Page::OnboardingRelay,
        Page::OnboardingSmtp,
        Page::OnboardingReady,
    ];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for screen in &screens {
            let active = *screen == app.page;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(6.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 3.0, if active { style::ACCENT } else { style::BORDER_STRONG });
        }
    });
}
```

**Step 2: Add new Page variants** to `types.rs`:

```rust
OnboardingRelay,   // Screen 3: add relay
OnboardingSmtp,    // Screen 4: SMTP bridge
OnboardingReady,   // Screen 5: done
```

**Step 3: Add required state fields** to `HootState`:

```rust
pub onboarding_npub: Option<String>,
pub onboarding_nsec: Option<String>,
pub onboarding_key_saved: bool,
pub onboarding_nsec_input: String,
pub onboarding_derived_npub: Option<String>,
pub onboarding_relays: Vec<String>,
pub onboarding_relay_input: String,
pub smtp_enabled: bool,
```

**Step 4: Update `render_app` match** to route new `Page::` variants to `OnboardingScreen::ui`.

**Step 5: Build**

```bash
cargo build 2>&1 | grep "^error" | head -30
```

**Step 6: Commit**

```bash
git add src/ui/onboarding.rs src/types.rs src/main.rs
git commit -m "feat: redesign onboarding flow with all 5 screens"
```

---

## Task 10: Secondary Pages — Contacts, Drafts, Trash, Requests, Junk

**Files:**
- Modify: `src/ui/contacts.rs`, `src/ui/drafts_page.rs`, `src/ui/trash.rs`, `src/ui/requests.rs`, `src/ui/junk.rs`

These pages share a pattern: page heading + list with custom rows. Use the same visual language as inbox.

**Step 1: Shared pattern for list pages**

Each page should:
1. Show a page title (16px strong, TEXT)
2. Show a list of items using custom rows (not TableBuilder)
3. Use SURFACE for row hover, shadow_sm
4. Show empty state in TEXT2 when no items

**For `drafts_page.rs`** — show draft items, click to reopen compose:

```rust
pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    page_header(ui, "Drafts");

    if app.state.drafts.is_empty() {
        empty_state(ui, "No drafts.");
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);
        for draft in &app.state.drafts.clone() {
            let row = list_row(ui, &draft.subject, &draft.to, style::format_timestamp(draft.modified_at).as_str());
            if row.clicked() {
                // reopen draft in compose
            }
        }
    });
}
```

**For `requests.rs`** — show with Allow/Block buttons:

```rust
pub fn render(app: &mut Hoot, ui: &mut egui::Ui) {
    page_header(ui, "Message Requests");
    // same list pattern but with Allow (ACCENT) and Block (TEXT2) buttons per row
}
```

**Step 2: Add shared helpers** at top of `src/ui/mod.rs`:

```rust
pub fn page_header(ui: &mut egui::Ui, title: &str) {
    egui::Frame::none()
        .fill(crate::style::SURFACE)
        .inner_margin(egui::Margin { left: 20.0, right: 20.0, top: 16.0, bottom: 16.0 })
        .show(ui, |ui| {
            ui.label(egui::RichText::new(title).size(16.0).strong().color(crate::style::TEXT));
        });
    let y = ui.cursor().top();
    ui.painter().line_segment(
        [egui::Pos2::new(0.0, y), egui::Pos2::new(ui.max_rect().right(), y)],
        egui::Stroke::new(1.0, crate::style::BORDER),
    );
}

pub fn empty_state(ui: &mut egui::Ui, message: &str) {
    ui.add_space(80.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(message).size(14.0).color(crate::style::TEXT2));
    });
}

pub fn list_row(ui: &mut egui::Ui, primary: &str, secondary: &str, meta: &str) -> egui::Response {
    use eframe::egui::*;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(8), crate::style::SURFACE);
        }
        let x = rect.left() + 16.0;
        let text_w = rect.width() - 100.0;
        ui.painter().text(Pos2::new(x, rect.top() + 14.0), Align2::LEFT_TOP, primary,
            FontId::proportional(13.5), crate::style::TEXT);
        ui.painter().text(Pos2::new(x, rect.top() + 32.0), Align2::LEFT_TOP, secondary,
            FontId::proportional(12.0), crate::style::TEXT2);
        ui.painter().text(Pos2::new(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER,
            meta, FontId::proportional(11.5), crate::style::TEXT3);
    }
    response
}
```

**Step 3: Build all secondary pages**

```bash
cargo build 2>&1 | grep "^error" | head -30
```

**Step 4: Commit**

```bash
git add src/ui/contacts.rs src/ui/drafts_page.rs src/ui/trash.rs src/ui/requests.rs src/ui/junk.rs src/ui/mod.rs
git commit -m "feat: redesign secondary pages (contacts, drafts, trash, requests, junk)"
```

---

## Task 11: Unlock Screen

**Files:**
- Modify: `src/ui/unlock_database.rs`

Design: centered layout, simple heading, password input, unlock button.

**Step 1: Rewrite `src/ui/unlock_database.rs`**

```rust
use eframe::egui::{self, Color32, CornerRadius, RichText, Vec2};
use crate::style;

pub struct UnlockDatabase;

impl UnlockDatabase {
    pub fn ui(app: &mut crate::Hoot, ui: &mut egui::Ui) {
        let avail = ui.available_size();
        ui.set_min_size(avail);
        ui.vertical_centered(|ui| {
            ui.add_space(avail.y * 0.25);

            ui.label(RichText::new("Unlock Hoot").size(24.0).strong().color(style::TEXT));
            ui.add_space(8.0);
            ui.label(RichText::new("Enter your database password to continue.").size(14.0).color(style::TEXT2));
            ui.add_space(32.0);

            ui.add(
                egui::TextEdit::singleline(&mut app.state.unlock_password)
                    .password(true)
                    .hint_text("Password")
                    .desired_width(280.0)
                    .font(egui::FontId::proportional(14.0)),
            );

            ui.add_space(16.0);

            if !app.state.unlock_error.is_empty() {
                ui.label(RichText::new(&app.state.unlock_error).size(13.0).color(style::AMBER));
                ui.add_space(12.0);
            }

            if ui.add(
                egui::Button::new(RichText::new("Unlock").size(14.0).color(Color32::WHITE))
                    .fill(style::ACCENT)
                    .corner_radius(CornerRadius::same(10))
                    .min_size(Vec2::new(280.0, 44.0)),
            ).clicked() {
                // attempt unlock
            }
        });
    }
}
```

**Step 2: Build**

```bash
cargo build 2>&1 | grep "^error" | head -20
```

**Step 3: Commit**

```bash
git add src/ui/unlock_database.rs
git commit -m "feat: redesign unlock screen"
```

---

## Task 12: Final Polish — Visual Audit

**Goal:** Run the app and do a visual comparison against `example-doc.html`. Fix any remaining discrepancies.

**Step 1: Run the app**

```bash
cargo run
```

**Step 2: Open `example-doc.html` in a browser**

```bash
xdg-open example-doc.html   # Linux
# or: open example-doc.html  # macOS
```

**Step 3: Compare each view:**

- [ ] Sidebar width, app name, compose button, nav items, account footer
- [ ] Search/toolbar bar
- [ ] Inbox: row spacing, unread dot, avatar colors, text hierarchy, timestamps, badges
- [ ] Thread view: back button, header, body, reply area
- [ ] Compose: full-screen, underline inputs, send button state
- [ ] Settings: two-column, descriptions, relay list, nsec reveal
- [ ] Onboarding: each of 5 screens, progress dots
- [ ] Unlock screen

**Step 4: Fix pixel-level discrepancies** — adjust padding, font sizes, and colors in `style.rs` or the relevant UI file to match.

**Step 5: Final commit**

```bash
git add -p   # stage only relevant changes
git commit -m "feat: visual polish pass against design spec"
```

---

## Notes for Implementer

### egui Limitations vs CSS

| CSS Feature | egui Approach |
|-------------|--------------|
| `box-shadow` | `epaint::Shadow` on `egui::Frame` |
| `border-radius` | `CornerRadius::same(n)` |
| Left-border highlight | Custom painter: `rect_filled` on a 3px-wide sub-rect |
| Gradient avatar bg | Solid first-stop color (acceptable approximation) |
| `translateY(-0.5px)` hover | Not implementable; omit |
| `-webkit-font-smoothing` | egui uses freetype/wgpu; inherently smooth |
| Underline inputs | `TextEdit::frame(false)` + manual `painter.line_segment()` |
| Focus ring glow | `painter.rect_stroke()` with expanded rect + translucent stroke |
| `overflow: hidden` text | `painter.layout()` with max_width, or `RichText` truncation |

### Key Anti-Patterns to Avoid

- Don't use `egui::Button` for custom-painted items — allocate rect and paint manually
- Don't use `TableBuilder` for inbox rows — it can't do custom heights or left borders
- Don't call `ui.separator()` for borders — use `painter.line_segment()` for pixel control
- Don't use `ui.spacing_mut().item_spacing` globally — set it per-scope with `spacing_mut()`

### Build Incrementally

After each task: `cargo build && cargo run` — fix any errors before moving to next task.
