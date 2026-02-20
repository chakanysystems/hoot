use std::collections::HashSet;

use super::Db;
use anyhow::Result;
use nostr::nips::nip59::UnwrappedGift;
use nostr::Event;
use rusqlite::OptionalExtension;
use serde_json::json;

impl Db {
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
}
