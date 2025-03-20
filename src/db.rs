use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use egui_extras::Table;
use include_dir::{include_dir, Dir};
use nostr::{Event, Kind};
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};
use serde_json::json;

use crate::account_manager::AccountManager;
use crate::mail_event::{MailMessage, MAIL_EVENT_KIND};
use crate::TableEntry;

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

    pub fn store_event(&self, event: &Event, account_manager: &mut AccountManager) -> Result<()> {
        // Try to unwrap the gift wrap if this event is a gift wrap
        let store_unwrapped =
            is_gift_wrap(event) && account_manager.unwrap_gift_wrap(event).is_ok();

        if store_unwrapped {
            let unwrapped = account_manager.unwrap_gift_wrap(event).unwrap();
            let mut rumor = unwrapped.rumor.clone();
            rumor.ensure_id();

            let id = rumor
                .id
                .expect("Invalid Gift Wrapped Event: There is no ID!")
                .to_hex();
            let raw = json!(rumor).to_string();

            self.connection.execute(
                "INSERT INTO events (id, raw)
                 VALUES (?1, ?2)",
                (id, raw),
            )?;
        } else {
            let id = event.id.to_string();
            let raw = json!(event).to_string();

            self.connection.execute(
                "INSERT INTO events (id, raw)
                 VALUES (?1, ?2)",
                (id, raw),
            )?;
        }

        Ok(())
    }

    pub fn has_event(&self, event_id: &str) -> Result<bool> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM events WHERE id = ?",
            [event_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// These messages will be displayed inside the top-level table.
    pub fn get_top_level_messages(&self) -> Result<Vec<TableEntry>> {
        let mut stmt = self.connection
            .prepare(
                "SELECT DISTINCT e.id, e.content, e.created_at, e.pubkey, jsonb_extract(value, '$[1]') AS subject
FROM events e, json_each(e.tags) AS tag
WHERE jsonb_extract(tag.value, '$[0]') = 'subject'
AND (EXISTS (
    SELECT 1
    FROM json_each(e.tags) AS tag
    WHERE jsonb_extract(tag.value, '$[0]') = 'e'
)
   OR NOT EXISTS (
    SELECT 1
    FROM events f, json_each(f.tags) AS tag
    WHERE jsonb_extract(tag.value, '$[0]') = 'e'
      AND jsonb_extract(tag.value, '$[1]') = e.id
))
ORDER BY created_at DESC
            ")?;
        let msgs_iter = stmt.query_map([], |row| {
            Ok(TableEntry {
                id: row.get(0)?,
                content: row.get(1)?,
                created_at: row.get(2)?,
                pubkey: row.get(3)?,
                subject: row.get(4)?,
            })
        })?;

        let messages = msgs_iter.collect::<Result<Vec<TableEntry>, rusqlite::Error>>()?;

        Ok(messages)
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
}

/// Check if an event is a gift wrap
fn is_gift_wrap(event: &Event) -> bool {
    event.kind == Kind::GiftWrap
}
