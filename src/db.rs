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
use tracing::{debug, info};

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
        debug!("Loading database at location {:?}", path.to_str());
        let conn = Connection::open(path)?;

        Ok(Self { connection: conn })
    }

    pub fn new_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;

        MIGRATIONS.to_latest(&mut conn);

        Ok(Self { connection: conn })
    }

    pub fn unlock_with_password(&mut self, password: String) -> Result<()> {
        self.connection.pragma_update(None, "key", password)?;

        // Apply migrations
        info!("Running Migrations");
        MIGRATIONS.to_latest(&mut self.connection)?;

        Ok(())
    }

    pub fn is_unlocked(&self) -> bool {
        // Try a simple query to check if the database is unlocked
        // If the database is locked, this will fail
        self.connection
            .query_row("SELECT 1", [], |_| Ok(()))
            .is_ok()
    }

    pub fn is_initialized(&self) -> bool {
        // Check if migrations have been run by checking if any tables exist
        // An uninitialized database won't have the schema set up yet
        self.connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' LIMIT 1",
                [],
                |_| Ok(()),
            )
            .is_ok()
    }

    pub fn get_pubkeys(&self) -> Result<Vec<String>> {
        let mut stmt = self.connection.prepare("SELECT pubkey FROM pubkeys;")?;

        let pubkeys_iter = stmt.query_map([], |row| Ok(row.get(0)?))?;
        let pubkeys = pubkeys_iter.collect::<Result<Vec<String>, rusqlite::Error>>()?;
        Ok(pubkeys)
    }

    pub fn add_pubkey(&self, pubkey: String) -> Result<()> {
        self.connection
            .execute("INSERT INTO pubkeys (pubkey) VALUES (?1)", ((pubkey),))?;

        Ok(())
    }

    pub fn delete_pubkey(&self, pubkey: String) -> Result<()> {
        self.connection
            .execute("DELETE FROM pubkeys WHERE pubkey = ?1", ((pubkey),))?;

        Ok(())
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

    /// Add a contact to the contacts table. If the contact already exists, update the petname.
    pub fn save_contact(&self, pubkey: &str, petname: Option<&str>) -> Result<()> {
        self.connection.execute(
            "INSERT INTO contacts (pubkey, petname) VALUES (?1, ?2)
             ON CONFLICT(pubkey) DO UPDATE SET petname = ?2",
            (pubkey, petname),
        )?;
        Ok(())
    }

    /// Update just the petname for an existing contact.
    pub fn update_contact_petname(&self, pubkey: &str, petname: Option<&str>) -> Result<()> {
        self.connection.execute(
            "UPDATE contacts SET petname = ?1 WHERE pubkey = ?2",
            (petname, pubkey),
        )?;
        Ok(())
    }

    /// Delete a contact from the contacts table.
    pub fn delete_contact(&self, pubkey: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM contacts WHERE pubkey = ?1", (pubkey,))?;
        Ok(())
    }

    /// Check if a pubkey is in the contacts table.
    pub fn is_contact(&self, pubkey: &str) -> Result<bool> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM contacts WHERE pubkey = ?1",
            [pubkey],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get all user contacts joined with their profile metadata.
    /// Returns (pubkey, petname, ProfileMetadata).
    pub fn get_user_contacts(&self) -> Result<Vec<(String, Option<String>, ProfileMetadata)>> {
        let mut stmt = self.connection.prepare(
            "SELECT c.pubkey, c.petname, pm.name, pm.display_name, pm.picture
             FROM contacts c
             LEFT JOIN profile_metadata pm ON c.pubkey = pm.pubkey
             ORDER BY LOWER(COALESCE(c.petname, pm.display_name, pm.name, c.pubkey))",
        )?;

        let contacts_iter = stmt.query_map([], |row| {
            let pubkey: String = row.get(0)?;
            let petname: Option<String> = row.get(1)?;
            let metadata = ProfileMetadata {
                name: row.get(2)?,
                display_name: row.get(3)?,
                picture: row.get(4)?,
            };
            Ok((pubkey, petname, metadata))
        })?;

        let mut contacts = Vec::new();
        for contact in contacts_iter {
            contacts.push(contact?);
        }
        Ok(contacts)
    }

    /// Get the petname for a given pubkey, if they are a contact.
    pub fn get_contact_petname(&self, pubkey: &str) -> Result<Option<String>> {
        let result: Option<Option<String>> = self
            .connection
            .query_row(
                "SELECT petname FROM contacts WHERE pubkey = ?1",
                [pubkey],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result.flatten())
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
                "WITH RECURSIVE
roots AS (
    SELECT DISTINCT e.id
    FROM events e, json_each(e.tags) AS tag
    WHERE jsonb_extract(tag.value, '$[0]') = 'subject'
    AND NOT EXISTS (
        SELECT 1
        FROM json_each(e.tags) AS etag
        WHERE jsonb_extract(etag.value, '$[0]') = 'e'
        AND EXISTS (SELECT 1 FROM events WHERE id = jsonb_extract(etag.value, '$[1]'))
    )
),
thread AS (
    SELECT id as root_id, id as msg_id FROM roots
    UNION
    SELECT t.root_id, e.id
    FROM thread t, events e, json_each(e.tags) AS etag
    WHERE jsonb_extract(etag.value, '$[0]') = 'e'
    AND jsonb_extract(etag.value, '$[1]') = t.msg_id
)
SELECT
    r.id,
    le.content,
    le.created_at,
    re.pubkey,
    (SELECT jsonb_extract(stag.value, '$[1]')
     FROM json_each(le.tags) AS stag
     WHERE jsonb_extract(stag.value, '$[0]') = 'subject'
     LIMIT 1) as subject,
    (SELECT COUNT(*) FROM thread t WHERE t.root_id = r.id) as thread_count
FROM roots r
JOIN events re ON re.id = r.id
JOIN events le ON le.id = (
    SELECT t2.msg_id FROM thread t2
    JOIN events e2 ON e2.id = t2.msg_id
    WHERE t2.root_id = r.id
    ORDER BY e2.created_at DESC
    LIMIT 1)
ORDER BY le.created_at DESC
            ")?;
        let msgs_iter = stmt.query_map([], |row| {
            Ok(TableEntry {
                id: row.get(0)?,
                content: row.get(1)?,
                created_at: row.get(2)?,
                pubkey: row.get(3)?,
                subject: row.get(4)?,
                thread_count: row.get(5)?,
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

    // --- Draft methods ---

    pub fn save_draft(
        &self,
        subject: &str,
        to_field: &str,
        content: &str,
        parent_events: &[String],
        selected_account: Option<&str>,
    ) -> Result<i64> {
        let parent_events_json = serde_json::to_string(parent_events)?;
        self.connection.execute(
            "INSERT INTO drafts (subject, to_field, content, parent_events, selected_account)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                subject,
                to_field,
                content,
                &parent_events_json,
                selected_account,
            ),
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn update_draft(
        &self,
        id: i64,
        subject: &str,
        to_field: &str,
        content: &str,
        parent_events: &[String],
        selected_account: Option<&str>,
    ) -> Result<()> {
        let parent_events_json = serde_json::to_string(parent_events)?;
        self.connection.execute(
            "UPDATE drafts SET subject = ?1, to_field = ?2, content = ?3,
             parent_events = ?4, selected_account = ?5, updated_at = unixepoch()
             WHERE id = ?6",
            (
                subject,
                to_field,
                content,
                &parent_events_json,
                selected_account,
                id,
            ),
        )?;
        Ok(())
    }

    pub fn get_drafts(&self) -> Result<Vec<Draft>> {
        let mut stmt = self.connection.prepare(
            "SELECT id, subject, to_field, content, parent_events, selected_account, created_at, updated_at
             FROM drafts ORDER BY updated_at DESC",
        )?;

        let drafts_iter = stmt.query_map([], |row| {
            let parent_events_json: String = row.get(4)?;
            let parent_events: Vec<String> =
                serde_json::from_str(&parent_events_json).unwrap_or_default();

            Ok(Draft {
                id: row.get(0)?,
                subject: row.get(1)?,
                to_field: row.get(2)?,
                content: row.get(3)?,
                parent_events,
                selected_account: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;

        let drafts = drafts_iter.collect::<Result<Vec<Draft>, rusqlite::Error>>()?;
        Ok(drafts)
    }

    pub fn delete_draft(&self, id: i64) -> Result<()> {
        self.connection
            .execute("DELETE FROM drafts WHERE id = ?1", (id,))?;
        Ok(())
    }

    pub fn get_draft_count(&self) -> Result<i64> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM drafts", [], |row| row.get(0))?;
        Ok(count)
    }
}

#[derive(Clone, Debug)]
pub struct Draft {
    pub id: i64,
    pub subject: String,
    pub to_field: String,
    pub content: String,
    pub parent_events: Vec<String>,
    pub selected_account: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
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

fn is_gift_wrap(event: &Event) -> bool {
    event.kind == Kind::GiftWrap
}

/// Format a database unlock error into a user-friendly message.
/// Detects the "wrong password" case from SQLCipher's NotADatabase error code.
pub fn format_unlock_error(e: &anyhow::Error) -> String {
    match e.downcast_ref::<rusqlite_migration::Error>() {
        Some(rusqlite_migration::Error::RusqliteError { err, .. }) => {
            match err.sqlite_error_code() {
                Some(rusqlite::ErrorCode::NotADatabase) => "Wrong password".to_string(),
                _ => format!("Database error: {}", e),
            }
        }
        _ => format!("Database error: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    #[test]
    fn test_load_pubkey() -> Result<()> {
        let db = Db::new_in_memory()?;
        let pk = Keys::generate().public_key();
        db.add_pubkey(pk.to_hex())?;
        let saved_list = db.get_pubkeys()?;
        assert!(saved_list.first().is_some());
        assert_eq!(saved_list.first().unwrap(), &pk.to_hex());

        Ok(())
    }

    #[test]
    fn test_delete_pubkey() -> Result<()> {
        let db = Db::new_in_memory()?;
        let pk = Keys::generate().public_key();
        db.add_pubkey(pk.to_hex())?;
        let saved_list = db.get_pubkeys()?;
        assert!(saved_list.first().is_some());
        assert_eq!(saved_list.first().unwrap(), &pk.to_hex());

        db.delete_pubkey(pk.to_hex())?;
        let saved_list = db.get_pubkeys()?;
        assert!(saved_list.first().is_none());
        assert!(saved_list.is_empty());

        Ok(())
    }
}
