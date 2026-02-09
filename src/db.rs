use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use include_dir::{include_dir, Dir};
use nostr::nips::nip59::UnwrappedGift;
use nostr::{Event, EventId, PublicKey};
use rusqlite::{Connection, OptionalExtension};
use rusqlite_migration::Migrations;
use serde_json::json;
use tracing::{debug, info};

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

    pub fn store_event(
        &self,
        event: &Event,
        unwrapped: Option<&UnwrappedGift>,
        gift_wrap_recipient: Option<&str>,
    ) -> Result<()> {
        if let Some(unwrapped) = unwrapped {
            let mut rumor = unwrapped.rumor.clone();
            rumor.ensure_id();

            if unwrapped.sender != rumor.pubkey {
                anyhow::bail!("Seal signer does not match rumor pubkey");
            }

            let id = rumor
                .id
                .expect("Invalid Gift Wrapped Event: There is no ID!")
                .to_hex();
            let author_pubkey = rumor.pubkey.to_string();
            if self.is_deleted(&id, Some(author_pubkey.as_str()))? {
                return Ok(());
            }
            let raw = json!(rumor).to_string();

            self.connection.execute(
                "INSERT OR IGNORE INTO events (id, raw)
                 VALUES (?1, ?2)",
                (id.clone(), raw),
            )?;

            self.save_gift_wrap_map(
                &event.id.to_string(),
                &id,
                gift_wrap_recipient,
                event.created_at.as_u64() as i64,
            )?;
            return Ok(());
        }

        let id = event.id.to_string();
        let author_pubkey = event.pubkey.to_string();
        if self.is_deleted(&id, Some(author_pubkey.as_str()))? {
            return Ok(());
        }
        let raw = json!(event).to_string();

        self.connection.execute(
            "INSERT OR IGNORE INTO events (id, raw)
             VALUES (?1, ?2)",
            (id, raw),
        )?;

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

    pub fn is_deleted(&self, event_id: &str, author_pubkey: Option<&str>) -> Result<bool> {
        let count: i64 = if let Some(pubkey) = author_pubkey {
            self.connection.query_row(
                "SELECT COUNT(*) FROM deleted_events
                 WHERE event_id = ?1
                   AND (author_pubkey IS NULL OR author_pubkey = ?2)",
                (event_id, pubkey),
                |row| row.get(0),
            )?
        } else {
            self.connection.query_row(
                "SELECT COUNT(*) FROM deleted_events WHERE event_id = ?1",
                (event_id,),
                |row| row.get(0),
            )?
        };

        Ok(count > 0)
    }

    pub fn is_trashed(&self, event_id: &str) -> Result<bool> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM trash_events WHERE event_id = ?1",
            (event_id,),
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    pub fn record_deletions(
        &mut self,
        event_ids: &[String],
        author_pubkey: Option<&str>,
        source_event_id: Option<&str>,
    ) -> Result<()> {
        let tx = self.connection.transaction()?;
        let mut deletable_ids: Vec<String> = Vec::new();

        if !event_ids.is_empty() {
            let placeholders = vec!["?"; event_ids.len()].join(",");
            let mut select_sql = format!("SELECT id FROM events WHERE id IN ({})", placeholders);
            if let Some(pubkey) = author_pubkey {
                select_sql.push_str(" AND pubkey = ?");
                let params = rusqlite::params_from_iter(
                    event_ids
                        .iter()
                        .map(|id| id as &dyn rusqlite::ToSql)
                        .chain(std::iter::once(&pubkey as &dyn rusqlite::ToSql)),
                );
                let mut stmt = tx.prepare(&select_sql)?;
                let rows = stmt.query_map(params, |row| row.get(0))?;
                for row in rows {
                    deletable_ids.push(row?);
                }
            } else {
                let params = rusqlite::params_from_iter(
                    event_ids.iter().map(|id| id as &dyn rusqlite::ToSql),
                );
                let mut stmt = tx.prepare(&select_sql)?;
                let rows = stmt.query_map(params, |row| row.get(0))?;
                for row in rows {
                    deletable_ids.push(row?);
                }
            }

            if !deletable_ids.is_empty() {
                let placeholders = vec!["?"; deletable_ids.len()].join(",");
                let mut delete_sql = format!("DELETE FROM events WHERE id IN ({})", placeholders);
                let mut delete_pmeta_sql = format!(
                    "DELETE FROM profile_metadata WHERE id IN ({})",
                    placeholders
                );

                if let Some(pubkey) = author_pubkey {
                    delete_sql.push_str(" AND pubkey = ?");
                    delete_pmeta_sql.push_str(" AND pubkey = ?");

                    let params = rusqlite::params_from_iter(
                        deletable_ids
                            .iter()
                            .map(|id| id as &dyn rusqlite::ToSql)
                            .chain(std::iter::once(&pubkey as &dyn rusqlite::ToSql)),
                    );
                    tx.execute(&delete_sql, params)?;

                    let pmeta_params = rusqlite::params_from_iter(
                        deletable_ids
                            .iter()
                            .map(|id| id as &dyn rusqlite::ToSql)
                            .chain(std::iter::once(&pubkey as &dyn rusqlite::ToSql)),
                    );
                    tx.execute(&delete_pmeta_sql, pmeta_params)?;
                } else {
                    let params = rusqlite::params_from_iter(
                        deletable_ids.iter().map(|id| id as &dyn rusqlite::ToSql),
                    );
                    tx.execute(&delete_sql, params)?;

                    let pmeta_params = rusqlite::params_from_iter(
                        deletable_ids.iter().map(|id| id as &dyn rusqlite::ToSql),
                    );
                    tx.execute(&delete_pmeta_sql, pmeta_params)?;
                }
            }
        }

        if !deletable_ids.is_empty() {
            if let Some(author) = author_pubkey {
                let mut insert_stmt = tx.prepare(
                    "INSERT OR IGNORE INTO deleted_events (event_id, author_pubkey, source_event_id)
                     VALUES (?1, ?2, ?3)",
                )?;
                for event_id in &deletable_ids {
                    insert_stmt.execute((event_id, author, source_event_id))?;
                }
            } else {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO deleted_events (event_id, author_pubkey, source_event_id)
                     VALUES (?1, NULL, ?2)",
                )?;
                for event_id in &deletable_ids {
                    stmt.execute((event_id, source_event_id))?;
                }
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn record_trash(&mut self, event_ids: &[String], purge_after: i64) -> Result<()> {
        if event_ids.is_empty() {
            return Ok(());
        }

        let tx = self.connection.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO trash_events (event_id, purge_after)
                 VALUES (?1, ?2)",
            )?;
            for event_id in event_ids {
                stmt.execute((event_id, purge_after))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn purge_expired_trash(&mut self, now: i64) -> Result<Vec<String>> {
        let tx = self.connection.transaction()?;

        let mut event_ids: Vec<String> = Vec::new();
        {
            let mut stmt =
                tx.prepare("SELECT event_id FROM trash_events WHERE purge_after <= ?1")?;
            let rows = stmt.query_map((now,), |row| row.get(0))?;
            for row in rows {
                event_ids.push(row?);
            }
        }

        if !event_ids.is_empty() {
            let placeholders = vec!["?"; event_ids.len()].join(",");
            let delete_events_sql = format!("DELETE FROM events WHERE id IN ({})", placeholders);
            let delete_pmeta_sql = format!(
                "DELETE FROM profile_metadata WHERE id IN ({})",
                placeholders
            );
            let delete_trash_sql = format!(
                "DELETE FROM trash_events WHERE event_id IN ({})",
                placeholders
            );

            let mut insert_stmt = tx.prepare(
                "INSERT OR IGNORE INTO deleted_events (event_id, author_pubkey, source_event_id)
                 VALUES (?1, NULL, NULL)",
            )?;
            for event_id in &event_ids {
                insert_stmt.execute((event_id,))?;
            }

            let params =
                rusqlite::params_from_iter(event_ids.iter().map(|id| id as &dyn rusqlite::ToSql));
            tx.execute(&delete_events_sql, params)?;

            let params =
                rusqlite::params_from_iter(event_ids.iter().map(|id| id as &dyn rusqlite::ToSql));
            tx.execute(&delete_pmeta_sql, params)?;

            let params =
                rusqlite::params_from_iter(event_ids.iter().map(|id| id as &dyn rusqlite::ToSql));
            tx.execute(&delete_trash_sql, params)?;
        }

        tx.commit()?;
        Ok(event_ids)
    }

    pub fn restore_from_trash(&mut self, event_id: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM trash_events WHERE event_id = ?1", (event_id,))?;
        Ok(())
    }

    pub fn purge_deleted_events(&mut self) -> Result<()> {
        let tx = self.connection.transaction()?;
        tx.execute(
            "DELETE FROM events
             WHERE EXISTS (
                 SELECT 1 FROM deleted_events d
                 WHERE d.event_id = events.id
                   AND (d.author_pubkey IS NULL OR d.author_pubkey = events.pubkey)
             )",
            [],
        )?;
        tx.execute(
            "DELETE FROM profile_metadata
             WHERE EXISTS (
                 SELECT 1 FROM deleted_events d
                 WHERE d.event_id = profile_metadata.id
                   AND (d.author_pubkey IS NULL OR d.author_pubkey = profile_metadata.pubkey)
             )",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Record deletion markers without requiring the IDs to exist in the events table.
    /// Used for gift wrap IDs which are stored in gift_wrap_map, not in events.
    pub fn record_deletion_markers(
        &self,
        event_ids: &[String],
        source_event_id: Option<&str>,
    ) -> Result<()> {
        if event_ids.is_empty() {
            return Ok(());
        }
        let mut stmt = self.connection.prepare(
            "INSERT OR IGNORE INTO deleted_events (event_id, author_pubkey, source_event_id)
             VALUES (?1, NULL, ?2)",
        )?;
        for event_id in event_ids {
            stmt.execute((event_id, source_event_id))?;
        }
        Ok(())
    }

    pub fn save_gift_wrap_map(
        &self,
        wrap_id: &str,
        inner_id: &str,
        recipient_pubkey: Option<&str>,
        created_at: i64,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT OR IGNORE INTO gift_wrap_map (wrap_id, inner_id, recipient_pubkey, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            (wrap_id, inner_id, recipient_pubkey, created_at),
        )?;
        Ok(())
    }

    pub fn gift_wrap_exists(&self, wrap_id: &str) -> Result<bool> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM gift_wrap_map WHERE wrap_id = ?1",
            (wrap_id,),
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn delete_from_trash(&mut self, event_ids: &[String]) -> Result<()> {
        if event_ids.is_empty() {
            return Ok(());
        }

        let placeholders = vec!["?"; event_ids.len()].join(",");
        let sql = format!(
            "DELETE FROM trash_events WHERE event_id IN ({})",
            placeholders
        );
        let params =
            rusqlite::params_from_iter(event_ids.iter().map(|id| id as &dyn rusqlite::ToSql));
        self.connection.execute(&sql, params)?;
        Ok(())
    }

    pub fn get_wrap_ids_for_inner(&self, inner_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .connection
            .prepare("SELECT wrap_id FROM gift_wrap_map WHERE inner_id = ?1")?;
        let rows = stmt.query_map((inner_id,), |row| row.get(0))?;
        let mut wrap_ids = Vec::new();
        for row in rows {
            wrap_ids.push(row?);
        }
        Ok(wrap_ids)
    }

    pub fn get_trashed_event_ids(&self, event_ids: &[String]) -> Result<HashSet<String>> {
        let mut trashed = HashSet::new();
        if event_ids.is_empty() {
            return Ok(trashed);
        }

        let placeholders = vec!["?"; event_ids.len()].join(",");
        let sql = format!(
            "SELECT event_id FROM trash_events WHERE event_id IN ({})",
            placeholders
        );
        let mut stmt = self.connection.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(event_ids.iter().map(|id| id as &dyn rusqlite::ToSql)),
            |row| row.get(0),
        )?;
        for row in rows {
            trashed.insert(row?);
        }
        Ok(trashed)
    }

    pub fn get_event_kind_pubkey(&self, event_id: &str) -> Result<Option<(i64, String)>> {
        self.connection
            .query_row(
                "SELECT kind, pubkey FROM events WHERE id = ?1",
                (event_id,),
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    // there is a high chance i am a retard.
    // there is a very high chance that there is a better way to do this
    // but it's not coming to mind! guess we'll find out.
    // context: see profile_metadata sql definition and compare to events definition

    pub fn get_profile_metadata(&self, pubkey: &str) -> Result<Option<ProfileMetadata>> {
        let mut stmt = self
            .connection
            .prepare("SELECT * FROM profile_metadata WHERE pubkey = ?")?;

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
        let mut stmt = self.connection.prepare(
            "WITH RECURSIVE
roots AS (
    SELECT DISTINCT e.id
    FROM events e, json_each(e.tags) AS tag
    WHERE jsonb_extract(tag.value, '$[0]') = 'subject'
    AND NOT EXISTS (
        SELECT 1 FROM deleted_events d
        WHERE d.event_id = e.id
        AND (d.author_pubkey IS NULL OR d.author_pubkey = e.pubkey)
    )
    AND NOT EXISTS (
        SELECT 1 FROM trash_events t
        WHERE t.event_id = e.id
    )
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
    AND NOT EXISTS (
        SELECT 1 FROM deleted_events d
        WHERE d.event_id = e.id
        AND (d.author_pubkey IS NULL OR d.author_pubkey = e.pubkey)
    )
    AND NOT EXISTS (
        SELECT 1 FROM trash_events t
        WHERE t.event_id = e.id
    )
)
SELECT
    r.id,
    le.content,
    le.created_at,
    le.pubkey,
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
            ",
        )?;
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

    pub fn get_trash_messages(&self) -> Result<Vec<TableEntry>> {
        let mut stmt = self.connection.prepare(
            "SELECT
                 e.id,
                 e.content,
                 e.created_at,
                 e.pubkey,
                 COALESCE((SELECT jsonb_extract(stag.value, '$[1]')
                  FROM json_each(e.tags) AS stag
                  WHERE jsonb_extract(stag.value, '$[0]') = 'subject'
                  LIMIT 1), '') as subject,
                 1 as thread_count
             FROM events e
             JOIN trash_events t ON t.event_id = e.id
             ORDER BY t.trashed_at DESC",
        )?;

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
        let mut stmt = self.connection.prepare(
            "SELECT id FROM events
             WHERE kind = ?
               AND NOT EXISTS (
                   SELECT 1 FROM deleted_events d
                   WHERE d.event_id = events.id
                     AND (d.author_pubkey IS NULL OR d.author_pubkey = events.pubkey)
               )
               AND NOT EXISTS (
                   SELECT 1 FROM trash_events t
                   WHERE t.event_id = events.id
               )",
        )?;

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
        self.get_email_thread_inner(event_id, true)
    }

    pub fn get_email_thread_including_trash(&self, event_id: &str) -> Result<Vec<MailMessage>> {
        self.get_email_thread_inner(event_id, false)
    }

    fn get_email_thread_inner(
        &self,
        event_id: &str,
        exclude_trash: bool,
    ) -> Result<Vec<MailMessage>> {
        let trash_filter = if exclude_trash {
            "AND NOT EXISTS (
                SELECT 1 FROM trash_events t
                WHERE t.event_id = {alias}.id
            )"
        } else {
            ""
        };

        let query = format!(
            r#"
        WITH RECURSIVE thread AS (
            -- 1. Start with the initial event
            SELECT id, raw FROM events WHERE id = ?1
            AND NOT EXISTS (
                SELECT 1 FROM deleted_events d
                WHERE d.event_id = events.id
                AND (d.author_pubkey IS NULL OR d.author_pubkey = events.pubkey)
            )
            {trash_seed}
            UNION
            -- 2. Recursively find all replies to the events in the thread
            SELECT e.id, e.raw
            FROM events e, json_each(e.tags) AS t, thread
            WHERE json_extract(t.value, '$[0]') = 'e' AND json_extract(t.value, '$[1]') = thread.id
            AND NOT EXISTS (
                SELECT 1 FROM deleted_events d
                WHERE d.event_id = e.id
                AND (d.author_pubkey IS NULL OR d.author_pubkey = e.pubkey)
            )
            {trash_replies}
            UNION
            -- 3. Recursively find the parent of the events in the thread
            SELECT e.id, e.raw
            FROM events e, thread
            JOIN json_each(thread.raw, '$.tags') as t
            WHERE json_extract(t.value, '$[0]') = 'e' AND e.id = json_extract(t.value, '$[1]')
            AND NOT EXISTS (
                SELECT 1 FROM deleted_events d
                WHERE d.event_id = e.id
                AND (d.author_pubkey IS NULL OR d.author_pubkey = e.pubkey)
            )
            {trash_parents}
        )
        SELECT DISTINCT raw FROM thread
        ORDER BY json_extract(raw, '$.created_at') ASC;
    "#,
            trash_seed = trash_filter.replace("{alias}", "events"),
            trash_replies = trash_filter.replace("{alias}", "e"),
            trash_parents = trash_filter.replace("{alias}", "e"),
        );

        let mut stmt = self.connection.prepare(&query)?;
        let event_iter = stmt.query_map([event_id], |row| {
            let raw_json: String = row.get(0)?;
            Self::parse_mail_message(&raw_json)
        })?;

        let thread = event_iter.collect::<Result<Vec<MailMessage>, rusqlite::Error>>()?;
        Ok(thread)
    }

    fn parse_mail_message(raw_json: &str) -> Result<MailMessage, rusqlite::Error> {
        let parsed_event: RawEventData = serde_json::from_str(raw_json)
            .map_err(|e| rusqlite::Error::UserFunctionError(e.into()))?;

        let mut to = Vec::new();
        let mut parent_events = Vec::new();
        let mut subject = String::new();

        for tag in parsed_event.tags {
            if tag.len() >= 2 {
                match tag[0].as_str() {
                    "p" => {
                        if let Ok(pubkey) = PublicKey::parse(&tag[1]) {
                            to.push(pubkey);
                        }
                    }
                    "e" => {
                        if let Ok(event_id) = EventId::parse(&tag[1]) {
                            parent_events.push(event_id);
                        }
                    }
                    "subject" => {
                        subject = tag[1].clone();
                    }
                    _ => {}
                }
            }
        }

        Ok(MailMessage {
            id: EventId::parse(&parsed_event.id).ok(),
            created_at: Some(parsed_event.created_at),
            content: parsed_event.content,
            author: Some(parsed_event.pubkey),
            subject,
            to,
            cc: Vec::new(),
            bcc: Vec::new(),
            parent_events: if parent_events.is_empty() {
                None
            } else {
                Some(parent_events)
            },
        })
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
