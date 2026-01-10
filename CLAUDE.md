# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Hoot is a desktop email/messaging client built on the Nostr protocol using Rust and egui. It provides email-like functionality over Nostr, featuring a native GUI for sending/receiving messages, managing contacts, and viewing profiles.

## Build & Development Commands

### Standard Development
```bash
# Development build and run
cargo build
cargo run

# Release build and run
cargo build --release
cargo run --release

# Run tests
cargo test

# Run specific test
cargo test <test_name>
```

### Profiling
Build with profiling enabled to use puffin profiler:
```bash
cargo build --features profiling
cargo run --features profiling
```
When profiling is enabled, the app starts a puffin server on `127.0.0.1:8585` and attempts to launch `puffin_viewer` automatically.

### Nix Development
If using Nix with flakes:
```bash
nix develop
```

## Architecture Overview

### Core Components

**Main Application (`main.rs`)**
- `Hoot` struct is the main application state containing:
  - `relay::RelayPool`: Manages WebSocket connections to Nostr relays
  - `account_manager::AccountManager`: Handles Nostr keypairs and gift-wrap decryption
  - `db::Db`: SQLite database for events, messages, and profile metadata
  - `HootState`: UI component states (compose windows, onboarding, settings)
- Application follows an immediate-mode GUI pattern with `update_app()` and `render_app()` functions
- Event loop handles relay messages, database updates, and UI rendering

**Relay System (`relay/`)**
- `RelayPool`: Manages multiple relay connections with automatic reconnection (5s intervals) and keepalive pings (30s)
- `Relay`: Individual WebSocket connection using `ewebsock` library
- `Subscription`: Nostr subscription filters sent to relays
- Message types: `ClientMessage` (outbound) and `RelayMessage` (inbound)
- All relay operations are async with wake-up callbacks to trigger UI repaints

**Database (`db.rs`)**
- Uses SQLite with `rusqlite` and bundled SQLCipher for encryption
- Migrations managed by `rusqlite_migration` from `migrations/` directory
- Stores raw Nostr events as JSON blobs with generated columns for querying
- Key tables:
  - `events`: Stores raw Nostr events with virtual columns extracted from JSON
  - `profile_metadata`: Caches Nostr metadata (kind 0) events
- Gift-wrapped events are automatically unwrapped and stored as their inner rumor
- Thread reconstruction via recursive CTEs in `get_email_thread()`

**Key Storage (`keystorage/`)**
- Platform-specific secure storage via trait abstraction
- Linux: Secret Service API / file-based fallback
- macOS: Keychain API via `security-framework` crate
- Windows: Credential Manager
- `AccountManager` coordinates key loading, generation, and gift-wrap decryption

**Mail Events (`mail_event.rs`)**
- Custom kind 2024 events for mail messages
- `MailMessage` struct with to/cc/bcc, subject, threading via parent event IDs
- Converts to Nostr gift-wrap events (NIP-59) for privacy - one wrapped event per recipient
- Uses `pollster` for blocking async operations

**UI System (`ui/`)**
- Modular UI with separate modules: `compose_window`, `onboarding`, `settings`
- Page-based navigation via `Page` enum (Inbox, Drafts, Settings, Contacts, etc.)
- Compose windows are floating and can have multiple instances tracked by unique IDs
- Contact images fetched asynchronously via background threads with caching

### Event Flow

1. **Receiving Messages**:
   - RelayPool receives WebSocket events → `process_message()` → `process_event()`
   - Event verification → duplicate check → store in DB
   - Gift wraps (kind 1059) are unwrapped if recipient key is available
   - Profile metadata (kind 0) updates contact cache and database

2. **Sending Messages**:
   - User composes message in `ComposeWindow`
   - `MailMessage::to_events()` creates gift-wrapped events for each recipient
   - Events sent to all connected relays via `RelayPool::send()`

3. **Profile Metadata**:
   - Lazy-loaded via `get_profile_metadata()` helper
   - Returns `ProfileOption::Waiting` if not cached (triggers relay request)
   - Cached in `profile_metadata` HashMap and persisted to database
   - Contact images fetched on-demand in background threads

### Database Schema Notes

- Events table uses JSON storage with generated virtual columns for efficient querying
- The `pmeta_is_newer()` function checks timestamps before updating profile metadata
- Thread reconstruction walks both parent and child references recursively
- All event IDs and public keys stored as hex strings

### Threading Model

- Main UI thread runs immediate-mode egui
- WebSocket connections use `ewebsock` with wake-up callbacks
- Profile image fetching uses `std::thread::spawn` with `reqwest::blocking`
- Gift wrap operations use `pollster` to block on async Nostr operations
- Database operations are synchronous (rusqlite)

## Development Notes

- Nostr protocol implementation via `nostr` crate (v0.37.0)
- NIP-59 (gift wrap) used for all private messages
- Application state stored in platform-specific directory via `eframe::storage_dir("hoot")`
- Database file: `{storage_dir}/hoot.db`
- Custom Inter font embedded and loaded at startup
