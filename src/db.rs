use std::any::Any;
use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use egui_extras::Table;
use include_dir::{include_dir, Dir};
use nostr::{Event, EventId, Kind, PublicKey};
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ValueRef};
use rusqlite::{Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde_json::json;

use crate::account_manager::AccountManager;
use crate::mail_event::{MailMessage, MAIL_EVENT_KIND};
use crate::ProfileMetadata;
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

    // there is a high chance i am a retard.
    // there is a very high chance that there is a better way to do this
    // but it's not coming to mind! guess we'll find out.
    // context: see profile_metadata sql definition and compare to events definition

    pub fn get_profile_metadata(&self, pubkey: &str) -> Result<Option<ProfileMetadata>> {
        let mut stmt = self
            .connection
            .prepare("SELECT * FROM profile_metadata WHERE pubkey = ?")?;

        use anyhow::Context;
        Ok(stmt
            .query_one([pubkey], |row| {
                Ok(ProfileMetadata {
                    name: row.get(2)?,
                    display_name: row.get(3)?,
                    picture: row.get(4)?,
                })
            })
            .optional()?)
    }

    pub fn get_contacts(&self) -> Result<Vec<(String, ProfileMetadata)>> {
        let mut stmt = self.connection.prepare(
            "SELECT pubkey, name, display_name, picture
             FROM profile_metadata
             ORDER BY LOWER(COALESCE(display_name, name, pubkey))",
        )?;

        let contacts_iter = stmt.query_map([], |row| {
            let pubkey: String = row.get(0)?;
            let metadata = ProfileMetadata {
                name: row.get(1)?,
                display_name: row.get(2)?,
                picture: row.get(3)?,
            };
            Ok((pubkey, metadata))
        })?;

        let mut contacts = Vec::new();
        for contact in contacts_iter {
            contacts.push(contact?);
        }

        Ok(contacts)
    }

    /// This function combines `write_profile_metadata` and `pmeta_is_newer` into
    /// one nice package.
    pub fn update_profile_metadata(&self, event: nostr::Event) -> Result<()> {
        if self.pmeta_is_newer(event.pubkey, event.created_at.as_u64())? {
            // we have new information
            self.write_profile_metadata(event)?;
        }

        Ok(())
    }

    /// This writes a raw profile metadata event to the DB.
    pub fn write_profile_metadata(&self, event: nostr::Event) -> Result<()> {
        if event.kind != nostr::Kind::Metadata {
            anyhow::bail!("Event provided is not a kind 0 event.");
        }

        use nostr::JsonUtil;
        let meta: nostr::Metadata = nostr::Metadata::from_json(event.content)?;

        self.connection
            .execute("REPLACE INTO profile_metadata (pubkey, id, name, display_name, picture, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (event.pubkey.to_string(), event.id.to_string(), meta.name, meta.display_name, meta.picture, event.created_at.as_u64())
            )?;
        Ok(())
    }

    /// Check to see if the created_at for the profile metadata event is newer than
    /// what we have saved for this pubkey.
    /// Returns true if `created_at` is newer than what is saved, and false if they are the same or older
    /// Note to self/TODO: Look into forking the nostr crate to convert time stamps to i64.
    fn pmeta_is_newer(&self, pubkey: nostr::PublicKey, created_at: u64) -> Result<bool> {
        self.connection
            .execute(
                "SELECT EXISTS (SELECT 1 FROM profile_metadata WHERE pubkey = $1 AND created_at <= $2) AS wow;",
                (pubkey.to_string(), created_at)
            )?;
        Ok(true)
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

    /// Fetches an entire email thread starting from a given event ID.
    /// It traverses up to the root and down to the latest reply.
    pub fn get_email_thread(&self, event_id: &str) -> Result<Vec<MailMessage>> {
        // This CTE recursively finds all parent and child event IDs related to the initial event.
        // The final SELECT now fetches the entire 'raw' JSON object for each event in the thread.
        let query = r#"
        WITH RECURSIVE thread AS (
            -- 1. Start with the initial event
            SELECT id, raw FROM events WHERE id = ?1
            UNION
            -- 2. Recursively find all replies to the events in the thread
            SELECT e.id, e.raw
            FROM events e, json_each(e.tags) AS t, thread
            WHERE json_extract(t.value, '$[0]') = 'e' AND json_extract(t.value, '$[1]') = thread.id
            UNION
            -- 3. Recursively find the parent of the events in the thread
            SELECT e.id, e.raw
            FROM events e, thread
            JOIN json_each(thread.raw, '$.tags') as t
            WHERE json_extract(t.value, '$[0]') = 'e' AND e.id = json_extract(t.value, '$[1]')
        )
        SELECT DISTINCT raw FROM thread
        ORDER BY json_extract(raw, '$.created_at') ASC;
    "#;

        let mut stmt = self.connection.prepare(query)?;
        let event_iter = stmt.query_map([event_id], |row| {
            let raw_json: String = row.get(0)?;

            // Deserialize the JSON into our temporary struct.
            let parsed_event: RawEventData = serde_json::from_str(&raw_json)
                .map_err(|e| rusqlite::Error::UserFunctionError(e.into()))?;

            // Process tags to extract recipients, parents, and subject.
            let mut to = Vec::new();
            let mut parent_events = Vec::new();
            let mut subject = String::new();

            for tag in parsed_event.tags {
                if tag.len() >= 2 {
                    match tag[0].as_str() {
                        "p" => {
                            // 'p' tags are for public keys (recipients)
                            if let Ok(pubkey) = PublicKey::parse(&tag[1]) {
                                to.push(pubkey);
                            }
                        }
                        "e" => {
                            // 'e' tags are for event IDs (threading)
                            if let Ok(event_id) = EventId::parse(&tag[1]) {
                                parent_events.push(event_id);
                            }
                        }
                        "subject" => {
                            subject = tag[1].clone();
                        }
                        _ => {} // Ignore other tags
                    }
                }
            }

            // Construct the final MailMessage struct.
            Ok(MailMessage {
                id: EventId::parse(&parsed_event.id).ok(),
                created_at: Some(parsed_event.created_at),
                content: parsed_event.content,
                author: Some(parsed_event.pubkey),
                subject,
                to,
                cc: Vec::new(),  // Assuming no 'cc' info in the event tags.
                bcc: Vec::new(), // Assuming no 'bcc' info in the event tags.
                parent_events: if parent_events.is_empty() {
                    None
                } else {
                    Some(parent_events)
                },
            })
        })?;

        let thread = event_iter.collect::<Result<Vec<MailMessage>, rusqlite::Error>>()?;
        Ok(thread)
    }
}

use serde::Deserialize;
/// A temporary struct to deserialize the raw JSON event from the database.
/// This makes parsing safe and reliable.
#[derive(Deserialize)]
struct RawEventData {
    id: String,
    content: String,
    created_at: i64,
    tags: Vec<Vec<String>>,
    pubkey: PublicKey,
}

/// Check if an event is a gift wrap
fn is_gift_wrap(event: &Event) -> bool {
    event.kind == Kind::GiftWrap
}
