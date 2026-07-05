# AGENTS.md — Hoot

Compact context for OpenCode sessions. When this conflicts with repo files, trust the repo.

## Project

Hoot — desktop email/messaging client on Nostr (Rust + egui). Single crate, not a workspace.

## Build & Run

```bash
cargo build
cargo run

# With profiling (puffin server on 127.0.0.1:8585)
cargo build --features profiling
cargo run --features profiling

# Makefile shortcut for dev + profiling + backtrace
make dev   # RUST_BACKTRACE=1 cargo run --features profiling

# Tests
cargo test
cargo test <test_name>
```

## Verification & CI

- GitHub Actions: `.github/workflows/build.yml` — builds and tests on ubuntu/windows/macOS, plus cross-compile targets.
- Nix flake (`flake.nix`) defines stricter checks: clippy (`--all-targets -- --deny warnings`), docs (`--deny warnings`), fmt, `cargo audit`, `cargo deny`.
- **No `rustfmt.toml` or `clippy.toml`** — default rules apply.

## Nix Development

```bash
nix develop
```

- Linux binary needs `nixGL` wrapper for GPU drivers (see flake `nixgl-wrapper`).
- Dev shell includes `rust-bin.stable.latest.default` with `rust-src` and `rust-analyzer`.

## Architecture Quick Reference

- **Entrypoint**: `src/main.rs` → `eframe::run_native("Hoot", ...)`
- **App state**: `Hoot` struct (in `main.rs`) holds `RelayPool`, `AccountManager`, `Db`, `HootState`.
- **GUI pattern**: Immediate-mode egui. `update_app()` and `render_app()` drive the loop.
- **UI modules**: `src/ui/` — `inbox`, `compose_window`, `thread_view`, `contacts`, `settings`, `onboarding`, etc.
- **Relay layer**: `src/relay/` — `RelayPool`, `Relay`, `Subscription`, `ClientMessage`/`RelayMessage`.
- **Database**: `src/db/mod.rs` and submodules (`contacts`, `drafts`, `events`, `nip05`, `queries`, `sender_status`).
- **Key storage**: `src/account_manager.rs` + platform-specific secure storage via `keyring` crate.
- **Mail events**: `src/mail_event.rs` — custom Nostr kind 2024, converted to NIP-59 gift-wraps per recipient.
- **Threading**: Main UI thread. WebSockets async via `ewebsock` with wake-up callbacks. HTTP blocking in `std::thread::spawn` (`reqwest::blocking`). Gift wrap uses `pollster` to block on async Nostr ops. DB ops are synchronous (`rusqlite`).

## Database

- SQLite with bundled SQLCipher (encryption). `rusqlite_migration` manages schema.
- **Migrations**: `migrations/NNN-name/` directories with `up.sql` (and optionally `down.sql`). Numbered sequentially.
- Migrations are **embedded at compile time** via `include_dir!("$CARGO_MANIFEST_DIR/migrations")` (`src/db/mod.rs`).
- DB path: `{eframe::storage_dir("hoot")}/hoot.db`.
- Must call `unlock_with_password(password)` before use; this applies pending migrations.
- Events stored as JSON blobs with `GENERATED ALWAYS` virtual columns (`pubkey`, `kind`, `created_at`, `tags`, `content`, `sig`) for querying.

## Nostr Dependencies & Protocol

- `nostr` crate v0.37.0 with `nip59` feature.
- All private messages use NIP-59 gift wrap (kind 1059 → unwrapped to inner rumor).
- Custom kind 2024 for mail message events.
- Event IDs and pubkeys stored as hex strings.

## Styling & UI Design

- **Read `DESIGN.md`** for full UI/UX spec (colors, typography, spacing, interaction rules, voice/tone).
- Light theme only. Accent: `#7c3aed` (purple). Fonts: Instrument Sans primary, Inter fallback.
- Custom theme applied via `style::apply_theme(&cc.egui_ctx)` in `main.rs`.
- Fonts embedded from `assets/` at compile time (`include_bytes!`).
- Avoid default egui/system widget styling — every interactive element is custom styled per `DESIGN.md`.

## Existing Instruction Files

- `CLAUDE.md` — detailed architecture overview and development notes (primary for Claude Code).
- `DESIGN.md` — UI/UX design specification.
- `README.md` — basic build instructions.

## Asset & Font Changes

- Fonts live in `assets/` and are baked in with `include_bytes!`. Changing a font file requires recompilation.
- `assets/` and `migrations/` are included in the Nix source set explicitly (`flake.nix`).
