# Recipient Chips in the Compose To Field

**Date:** 2026-02-26

## Problem

The compose window's TO field is a plain text string. NIP-05 resolution only fires when the user
presses Send, requiring a second Send press after resolution completes. There is no visual
feedback about resolution state until that point.

## Solution

Replace the plain `to_field: String` with a chip-based input. Recipients become committed chips
when the user presses Enter, Space, Comma, or Tab. NIP-05 chips immediately trigger background
resolution, so by the time the user hits Send the address is already resolved.

## Data Model

Replace `to_field: String` in `ComposeWindowState` with:

```rust
pub to_input: String,           // token currently being typed
pub recipients: Vec<Recipient>, // committed chips
```

New types in `compose_window.rs`:

```rust
pub struct Recipient {
    pub raw: String,
    pub kind: RecipientKind,
}

pub enum RecipientKind {
    Pubkey(PublicKey),
    Nip05 {
        identifier: String,
        resolution: Nip05Resolution,  // Pending | Resolved(hex) | Failed
    },
}
```

Each frame, every `Nip05 { Pending }` chip polls `nip05_resolver.get()` and updates its
resolution in place.

## Chip Rendering

The TO row uses `ui.horizontal_wrapped` so chips and the text input flow inline:

```
[ ✓ user@domain.com  × ]  [ npub1abc…xyz  × ]  [____________]
```

- **Frame**: fill `SURFACE2`, corner radius 6, thin `border()` stroke
- **NIP-05 chips**: status icon left of label — `?` gray (Pending), `✓` green (Resolved),
  `✗` red (Failed). Same `status_display()` convention used in Settings.
- **Pubkey chips**: truncated label only (`npub1abc…xyz`), no status icon
- **× button**: small clickable label in `TEXT3`; clicking discards the chip (not returned to
  input)
- **Text input**: sits after last chip, `desired_width(120.0)`, `lock_focus(true)` to intercept
  Tab

## Commit Triggers

After each frame:
- If `to_input` ends with a space or comma → strip delimiter, commit preceding token
- Enter and Tab → captured via `input_mut` consume, commit current token

On commit:
- Contains `@` and passes `parse_nip05` → Nip05 chip, calls `nip05_resolver.request()`
- Valid npub (bech32) or hex → Pubkey chip
- Otherwise → silently discard

## Send Logic

Read from `recipients` instead of parsing `to_field`:

- Any `Nip05 { Pending }` → abort, show "Resolving NIP-05 addresses…"
- Any `Nip05 { Failed }` → abort, show "Could not resolve: …" in red
- All resolved → extract `PublicKey` from each chip, send

## Draft Compatibility

Serialize `recipients` back to a space-separated string (same format as the old `to_field`)
for draft save/load. On load, each token is re-parsed into chips; NIP-05 ones start as Pending
and re-trigger resolution automatically.
