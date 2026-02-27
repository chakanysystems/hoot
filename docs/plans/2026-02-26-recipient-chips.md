# Recipient Chips Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the plain-text TO field in the compose window with inline chips that commit on Enter/Space/Comma/Tab and resolve NIP-05 addresses automatically on chip creation.

**Architecture:** Add a `Recipient` / `RecipientKind` enum to `compose_window.rs`, replace `to_field: String` with `to_input + recipients`, render chips inline with the text input using `horizontal_wrapped`, and poll NIP-05 resolution state each frame.

**Tech Stack:** Rust, egui (`horizontal_wrapped`, `TextEdit`, `Frame`, painter calls), existing `Nip05Resolver` and `parse_nip05` from `src/nip05.rs`.

**Design doc:** `docs/plans/2026-02-26-recipient-chips-design.md`

---

### Task 1: Add `Recipient` types and `parse_recipient_token`

**Files:**
- Modify: `src/ui/compose_window.rs` (top of file, before `ComposeWindowState`)

**Step 1: Write failing tests**

Add at the bottom of `src/ui/compose_window.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_recipient_npub() {
        // A known valid npub1 bech32 key
        let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
        let result = parse_recipient_token(npub);
        assert!(matches!(result, Some(RecipientKind::Pubkey(_))));
    }

    #[test]
    fn test_parse_recipient_nip05() {
        let result = parse_recipient_token("bob@example.com");
        assert!(matches!(
            result,
            Some(RecipientKind::Nip05 { .. })
        ));
    }

    #[test]
    fn test_parse_recipient_invalid() {
        assert!(parse_recipient_token("notakey").is_none());
        assert!(parse_recipient_token("").is_none());
        assert!(parse_recipient_token("   ").is_none());
    }

    #[test]
    fn test_serialize_recipients_roundtrip() {
        // A known valid npub
        let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
        let recipients = vec![
            Recipient {
                raw: npub.to_string(),
                kind: RecipientKind::Nip05 {
                    identifier: "alice@example.com".to_string(),
                    resolution: crate::nip05::Nip05Resolution::Failed,
                },
            },
            Recipient {
                raw: "alice@example.com".to_string(),
                kind: RecipientKind::Nip05 {
                    identifier: "alice@example.com".to_string(),
                    resolution: crate::nip05::Nip05Resolution::Failed,
                },
            },
        ];
        let serialized = serialize_recipients(&recipients);
        let tokens: Vec<&str> = serialized.split_whitespace().collect();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], npub);
        assert_eq!(tokens[1], "alice@example.com");
    }
}
```

**Step 2: Run to verify it fails**

```
cargo test -p hoot 2>&1 | grep -A3 "test_parse_recipient"
```

Expected: compile error — `Recipient`, `RecipientKind`, `parse_recipient_token`, `serialize_recipients` don't exist yet.

**Step 3: Add the types and functions**

In `src/ui/compose_window.rs`, add after the imports and before `ComposeWindowState`:

```rust
use crate::nip05::{parse_nip05, Nip05Resolution};

#[derive(Debug, Clone)]
pub struct Recipient {
    pub raw: String,
    pub kind: RecipientKind,
}

#[derive(Debug, Clone)]
pub enum RecipientKind {
    Pubkey(nostr::PublicKey),
    Nip05 {
        identifier: String,
        resolution: Nip05Resolution,
    },
}

/// Parse a single whitespace-trimmed token into a RecipientKind.
/// Returns None for empty or unrecognizable input.
pub fn parse_recipient_token(token: &str) -> Option<RecipientKind> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    // NIP-05 identifier
    if token.contains('@') {
        if parse_nip05(token).is_some() {
            return Some(RecipientKind::Nip05 {
                identifier: token.to_string(),
                resolution: Nip05Resolution::Pending,
            });
        }
        return None;
    }

    // bech32 npub
    use nostr::FromBech32;
    if let Ok(pk) = nostr::PublicKey::from_bech32(token) {
        return Some(RecipientKind::Pubkey(pk));
    }

    // hex pubkey
    if let Ok(pk) = nostr::PublicKey::from_hex(token) {
        return Some(RecipientKind::Pubkey(pk));
    }

    None
}

/// Serialize recipients back to a space-separated string for draft storage.
pub fn serialize_recipients(recipients: &[Recipient]) -> String {
    recipients
        .iter()
        .map(|r| r.raw.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}
```

**Step 4: Run tests**

```
cargo test -p hoot 2>&1 | grep -A3 "test_parse_recipient\|test_serialize"
```

Expected: all 4 tests pass.

**Step 5: Commit**

```
jj describe -m "feat: add Recipient types and parse_recipient_token" && jj new
```

---

### Task 2: Update `ComposeWindowState`

**Files:**
- Modify: `src/ui/compose_window.rs` (`ComposeWindowState` struct)

**Step 1: Replace `to_field` with `to_input` + `recipients`**

Find `ComposeWindowState` and change:

```rust
// BEFORE
pub to_field: String,

// AFTER
pub to_input: String,
pub recipients: Vec<Recipient>,
```

**Step 2: Fix all compile errors**

Run `cargo build 2>&1 | grep "error\[" | head -40` and fix each:

- Any `state.to_field` reference → decide whether it should read from `serialize_recipients(&state.recipients)` (draft save) or `state.to_input` (the live input). See Task 4 and Task 5 for the full send/draft logic — for now just make it compile by temporarily replacing `state.to_field` with `state.to_input`.
- Constructor sites (e.g. where `ComposeWindowState { to_field: ..` is built) → replace with `to_input: String::new(), recipients: Vec::new()`.

**Step 3: Build succeeds**

```
cargo build 2>&1 | grep "^error"
```

Expected: no errors.

**Step 4: Commit**

```
jj describe -m "refactor: replace to_field with to_input + recipients in ComposeWindowState" && jj new
```

---

### Task 3: Chip commit logic

**Files:**
- Modify: `src/ui/compose_window.rs` (TO row rendering section, inside `show_window`)

**Step 1: Replace the TO row with chip flow + input**

Find the TO row block (around line 270):

```rust
// To row
ui.horizontal(|ui| {
    ui.label(...);
    ui.add_space(4.0);
    style::underline_text_edit(ui, &mut state.to_field, ...);
});
```

Replace it with:

```rust
// To row
ui.horizontal_wrapped(|ui| {
    ui.label(
        RichText::new("TO")
            .size(11.0)
            .color(style::TEXT3)
            .strong(),
    );
    ui.add_space(4.0);

    // Render committed chips
    let mut remove_idx: Option<usize> = None;
    for (i, recipient) in state.recipients.iter().enumerate() {
        let (label_text, status_color) = match &recipient.kind {
            RecipientKind::Pubkey(_) => {
                let truncated = if recipient.raw.len() > 16 {
                    format!("{}…{}", &recipient.raw[..8], &recipient.raw[recipient.raw.len()-4..])
                } else {
                    recipient.raw.clone()
                };
                (truncated, None)
            }
            RecipientKind::Nip05 { identifier, resolution } => {
                let (icon, color, _) = match resolution {
                    Nip05Resolution::Pending  => ("?", style::TEXT3, ""),
                    Nip05Resolution::Resolved(_) => ("✓", style::GREEN, ""),
                    Nip05Resolution::Failed   => ("✗", egui::Color32::RED, ""),
                };
                (format!("{} {}", icon, identifier), Some(color))
            }
        };

        egui::Frame::new()
            .fill(style::SURFACE2)
            .stroke(egui::Stroke::new(1.0, style::border()))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin { left: 6, right: 4, top: 2, bottom: 2 })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let text = RichText::new(&label_text)
                        .size(12.5)
                        .color(status_color.unwrap_or(style::TEXT));
                    ui.label(text);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("×").size(12.0).color(style::TEXT3),
                            )
                            .frame(false)
                            .min_size(egui::vec2(14.0, 14.0)),
                        )
                        .clicked()
                    {
                        remove_idx = Some(i);
                    }
                });
            });
    }
    if let Some(idx) = remove_idx {
        state.recipients.remove(idx);
    }

    // Text input
    let input_resp = ui.add(
        egui::TextEdit::singleline(&mut state.to_input)
            .hint_text(if state.recipients.is_empty() {
                "npub, hex, or user@domain.com"
            } else {
                ""
            })
            .frame(false)
            .desired_width(120.0)
            .font(egui::FontId::proportional(13.5))
            .text_color(style::TEXT)
            .lock_focus(true),
    );

    // Commit on Enter or Tab
    let commit_keyed = input_resp.has_focus() && ui.input_mut(|i| {
        let enter = i.key_pressed(egui::Key::Enter);
        let tab   = i.key_pressed(egui::Key::Tab);
        if enter { i.consume_key(egui::Modifiers::NONE, egui::Key::Enter); }
        if tab   { i.consume_key(egui::Modifiers::NONE, egui::Key::Tab); }
        enter || tab
    });

    // Commit on trailing space or comma
    let commit_delim = state.to_input.ends_with(' ') || state.to_input.ends_with(',');

    if commit_keyed || commit_delim {
        let token = state.to_input.trim_end_matches([' ', ',']).trim().to_string();
        state.to_input.clear();
        if !token.is_empty() {
            if let Some(kind) = parse_recipient_token(&token) {
                // Fire NIP-05 resolution immediately
                if let RecipientKind::Nip05 { ref identifier, .. } = kind {
                    app.nip05_resolver.request(identifier.clone());
                }
                state.recipients.push(Recipient { raw: token, kind });
            }
        }
    }
});
```

**Step 2: Build**

```
cargo build 2>&1 | grep "^error"
```

Fix any borrow/lifetime issues. If `state` and `app` can't both be borrowed at the same time (the existing pattern defers such work), use a local variable to stage the new recipient and push it after the UI block — same pattern as `DraftAction`.

**Step 3: Smoke test visually**

```
cargo run
```

Open a compose window, type `bob@example.com` and press Space. Verify a chip appears. Type an npub and press Enter. Verify a pubkey chip appears. Click × on a chip — it disappears.

**Step 4: Commit**

```
jj describe -m "feat: add recipient chip flow to compose To row" && jj new
```

---

### Task 4: Per-frame NIP-05 resolution polling

**Files:**
- Modify: `src/ui/compose_window.rs` (top of `show_window`, before the egui::Window call)

**Step 1: Poll resolver each frame**

After the `account_options` build block and before `let state = app.state...`, the function already borrows `app` immutably then `app.state` mutably. Resolution polling needs mutable access to both `app.nip05_resolver` and `app.state`. Use the same deferred-action pattern already in the file:

At the top of `show_window`, add a staging variable:

```rust
let mut nip05_requests: Vec<String> = Vec::new();
```

Inside the window, after rendering chips (where `state` is in scope), add:

```rust
// Poll resolution state for pending NIP-05 chips
for recipient in state.recipients.iter_mut() {
    if let RecipientKind::Nip05 { identifier, resolution } = &mut recipient.kind {
        if *resolution == Nip05Resolution::Pending {
            if let Some(r) = app.nip05_resolver.get(identifier) {
                *resolution = r.clone();
            }
        }
    }
}
```

Note: `app.nip05_resolver.get()` is `&self` so it can coexist with the `state` mutable borrow since they're separate fields of `app`. If the borrow checker complains, split the poll into a pre-pass before borrowing `state`.

**Step 2: Build**

```
cargo build 2>&1 | grep "^error"
```

**Step 3: Commit**

```
jj describe -m "feat: poll NIP-05 resolution state each frame in compose window" && jj new
```

---

### Task 5: Update send logic

**Files:**
- Modify: `src/ui/compose_window.rs` (Send button `clicked()` handler)

**Step 1: Replace `to_field` parsing with `recipients` iteration**

Find the `if ui.add_enabled(!to_empty, send_btn).clicked()` block. Replace the entire recipient-building loop (lines ~131–182) with:

```rust
let mut recipient_keys: Vec<PublicKey> = Vec::new();
let mut any_pending = false;
let mut failed: Vec<String> = Vec::new();

for r in &state.recipients {
    match &r.kind {
        RecipientKind::Pubkey(pk) => {
            recipient_keys.push(*pk);
        }
        RecipientKind::Nip05 { identifier, resolution } => {
            match resolution {
                Nip05Resolution::Resolved(hex) => {
                    match PublicKey::from_hex(hex) {
                        Ok(pk) => recipient_keys.push(pk),
                        Err(_) => failed.push(identifier.clone()),
                    }
                }
                Nip05Resolution::Pending => {
                    any_pending = true;
                }
                Nip05Resolution::Failed => {
                    failed.push(identifier.clone());
                }
            }
        }
    }
}
```

Also update the `to_empty` check to use `state.recipients.is_empty() && state.to_input.trim().is_empty()`.

**Step 2: Build**

```
cargo build 2>&1 | grep "^error"
```

**Step 3: Commit**

```
jj describe -m "feat: update send logic to read from recipients chips" && jj new
```

---

### Task 6: Update draft save/load

**Files:**
- Modify: `src/ui/compose_window.rs` (DraftAction::Save construction and draft loading path)

**Step 1: Serialize on save**

Find where `DraftAction::Save { to_field: state.to_field.clone(), .. }` is built. Change to:

```rust
to_field: serialize_recipients(&state.recipients),
```

**Step 2: Deserialize on load**

Find where a `ComposeWindowState` is constructed from a saved draft (search for `ComposeWindowState {` in the codebase — likely in `drafts_page.rs` or `main.rs`). Where `to_field` is set from the database string, replace with:

```rust
to_input: String::new(),
recipients: draft_to_field
    .split_whitespace()
    .filter_map(|token| {
        parse_recipient_token(token).map(|kind| {
            // Fire NIP-05 resolution for any NIP-05 tokens
            // (caller must call nip05_resolver.request() separately)
            Recipient { raw: token.to_string(), kind }
        })
    })
    .collect(),
```

After constructing the state, for each NIP-05 recipient in the loaded state, call `app.nip05_resolver.request(identifier.clone())`.

**Step 3: Build and smoke test**

```
cargo build 2>&1 | grep "^error"
cargo run
```

Save a draft with a NIP-05 address. Reopen — verify the chip appears with `?` status that transitions to `✓` or `✗`.

**Step 4: Commit**

```
jj describe -m "feat: serialize/deserialize recipients for draft save/load" && jj new
```

---

### Task 7: Clean up dead code

**Files:**
- Modify: `src/ui/compose_window.rs`

Remove any remaining references to the old `to_field` field, `any_pending` / `failed_nip05s` variables from the old send logic, and the old `send_status` NIP-05 resolution message path if it's now unreachable. Run:

```
cargo build 2>&1 | grep "warning.*unused\|warning.*dead_code"
```

Fix any unused variable warnings introduced by this change.

**Commit:**

```
jj describe -m "chore: remove dead to_field code after chip migration" && jj new
```
