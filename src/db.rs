use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use include_dir::{include_dir, Dir};
use nostr::{Event, Kind};
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};
use serde_json::json;

use crate::account_manager::AccountManager;
use crate::mail_event::MAIL_EVENT_KIND;

static MIGRATIONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

static MIGRATIONS: LazyLock<Migrations<'static>> =
    LazyLock::new(|| Migrations::from_directory(&MIGRATIONS_DIR).unwrap());

pub struct Db {
    connection: Connection,
}

impl Db {
    pub fn new(path: PathBuf) -> Result<Self> {
        let mut conn = Connection::open(path)?;

        // Apply migrations
        MIGRATIONS.to_latest(&mut conn)?;

        Ok(Self { connection: conn })
    }

    /// Store a mail event in the database
    ///
    /// This function first attempts to unwrap the gift wrap if necessary,
    /// and then stores the event in the database.
    pub fn store_mail_event(
        &self,
        event: &Event,
        account_manager: &mut AccountManager,
    ) -> Result<()> {
        // Try to unwrap the gift wrap if this event is a gift wrap
        let store_unwrapped =
            is_gift_wrap(event) && account_manager.unwrap_gift_wrap(event).is_ok();

        // Determine what event to store
        if store_unwrapped {
            // Unwrap succeeded, store the unwrapped event
            let unwrapped = account_manager.unwrap_gift_wrap(event).unwrap();

            // Get event details from the unwrapped gift
            let id = match unwrapped.rumor.id {
                Some(id) => id.to_string(),
                None => "unknown".to_string(),
            };
            let pubkey = unwrapped.rumor.pubkey.to_string();
            let created_at = unwrapped.rumor.created_at.as_u64();
            let kind = unwrapped.rumor.kind.as_u16() as u32;
            let tags_json = json!(unwrapped.rumor.tags).to_string();
            let content = unwrapped.rumor.content.clone();
            let sig = unwrapped.sender.to_string(); // Use sender pubkey as signature reference

            // Store the unwrapped event in the database
            self.connection.execute(
                "INSERT OR REPLACE INTO events (id, pubkey, created_at, kind, tags, content, sig)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (id, pubkey, created_at, kind, tags_json, content, sig),
            )?;
        } else {
            // Use original event
            // Convert tags to JSON string for storage
            let tags_json = json!(event.tags).to_string();

            // Get event details
            let id = event.id.to_string();
            let pubkey = event.pubkey.to_string();
            let created_at = event.created_at.as_u64();
            let kind = event.kind.as_u16() as u32;
            let sig = event.sig.to_string();

            // Store the event in the database
            self.connection.execute(
                "INSERT OR REPLACE INTO events (id, pubkey, created_at, kind, tags, content, sig)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (id, pubkey, created_at, kind, tags_json, &event.content, sig),
            )?;
        }

        Ok(())
    }

    /// Check if the database contains an event with the given ID
    pub fn has_event(&self, event_id: &str) -> Result<bool> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM events WHERE id = ?",
            [event_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Get all event IDs for mail events
    pub fn get_mail_event_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .connection
            .prepare("SELECT id FROM events WHERE kind = ?")?;

        let mail_kind = u32::from(MAIL_EVENT_KIND as u16);

        let id_iter = stmt.query_map([mail_kind], |row| {
            let id: String = row.get(0)?;
            Ok(id)
        })?;

        let mut ids = Vec::new();
        for id_result in id_iter {
            match id_result {
                Ok(id) => ids.push(id),
                Err(e) => {
                    tracing::error!("Error loading mail event ID: {}", e);
                }
            }
        }

        Ok(ids)
    }

    /// Get the JSON representation of an event by its ID
    pub fn get_event_json(&self, event_id: &str) -> Result<Option<String>> {
        let result = self.connection.query_row(
            "SELECT json_object('id', id, 'pubkey', pubkey, 'created_at', created_at,
                              'kind', kind, 'tags', json(tags), 'content', content,
                              'sig', sig)
             FROM events WHERE id = ?",
            [event_id],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(json) => Ok(Some(json)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// Check if an event is a gift wrap
fn is_gift_wrap(event: &Event) -> bool {
    event.kind == Kind::GiftWrap
}
