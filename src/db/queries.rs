use anyhow::Result;
use nostr::{EventId, PublicKey};

use crate::mail_event::{MailMessage, MAIL_EVENT_KIND};
use crate::types::TableEntry;

use super::{Db, RawEventData};

impl Db {
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
    AND (
        e.pubkey IN (SELECT pubkey FROM pubkeys)
        OR e.pubkey IN (SELECT pubkey FROM contacts)
        OR e.pubkey IN (SELECT pubkey FROM sender_status WHERE status = 'allowed')
        OR (
            e.pubkey NOT IN (SELECT pubkey FROM sender_status)
            AND e.pubkey IN (
                SELECT DISTINCT json_extract(t2.value, '$[1]')
                FROM events e2, json_each(e2.tags) AS t2
                WHERE e2.pubkey IN (SELECT pubkey FROM pubkeys)
                    AND e2.kind = 2024
                    AND json_extract(t2.value, '$[0]') = 'p'
            )
        )
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

    pub fn get_request_messages(&self) -> Result<Vec<TableEntry>> {
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
             FROM events e, json_each(e.tags) AS tag
             WHERE jsonb_extract(tag.value, '$[0]') = 'subject'
             AND e.pubkey NOT IN (SELECT pubkey FROM contacts)
             AND e.pubkey NOT IN (SELECT pubkey FROM sender_status)
             AND e.pubkey NOT IN (SELECT pubkey FROM pubkeys)
             AND e.pubkey NOT IN (
                 SELECT DISTINCT json_extract(t2.value, '$[1]')
                 FROM events e2, json_each(e2.tags) AS t2
                 WHERE e2.pubkey IN (SELECT pubkey FROM pubkeys)
                     AND e2.kind = 2024
                     AND json_extract(t2.value, '$[0]') = 'p'
             )
             AND NOT EXISTS (
                 SELECT 1 FROM deleted_events d
                 WHERE d.event_id = e.id
                 AND (d.author_pubkey IS NULL OR d.author_pubkey = e.pubkey)
             )
             AND NOT EXISTS (
                 SELECT 1 FROM trash_events t
                 WHERE t.event_id = e.id
             )
             ORDER BY e.created_at DESC",
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

    pub fn get_junk_messages(&self) -> Result<Vec<TableEntry>> {
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
             FROM events e, json_each(e.tags) AS tag
             WHERE jsonb_extract(tag.value, '$[0]') = 'subject'
             AND e.pubkey IN (SELECT pubkey FROM sender_status WHERE status = 'junked')
             AND NOT EXISTS (
                 SELECT 1 FROM deleted_events d
                 WHERE d.event_id = e.id
                 AND (d.author_pubkey IS NULL OR d.author_pubkey = e.pubkey)
             )
             AND NOT EXISTS (
                 SELECT 1 FROM trash_events t
                 WHERE t.event_id = e.id
             )
             ORDER BY e.created_at DESC",
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
        let mut sender_nip05 = None;

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
                    "nip05" => {
                        sender_nip05 = Some(tag[1].clone());
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
            sender_nip05,
        })
    }

    pub fn search_messages(&self, query: &str) -> Result<Vec<TableEntry>> {
        let search_pattern = format!("%{}%", query.to_lowercase());
        tracing::debug!("search_messages called with pattern: '{}'", search_pattern);

        // Search only mail events (kind 2024), excluding trash and junk
        let mail_kind = u32::from(MAIL_EVENT_KIND);
        let mut stmt = self.connection.prepare(
            "SELECT DISTINCT
                e.id,
                e.content,
                e.created_at,
                e.pubkey,
                COALESCE((SELECT jsonb_extract(stag.value, '$[1]')
                 FROM json_each(e.tags) AS stag
                 WHERE jsonb_extract(stag.value, '$[0]') = 'subject' LIMIT 1), '') as subject,
                1 as thread_count
            FROM events e
            LEFT JOIN profile_metadata pm ON e.pubkey = pm.pubkey
            WHERE e.kind = ?2
            AND NOT EXISTS (SELECT 1 FROM deleted_events d WHERE d.event_id = e.id)
            AND NOT EXISTS (SELECT 1 FROM trash_events t WHERE t.event_id = e.id)
            AND NOT EXISTS (
                SELECT 1 FROM sender_status ss
                WHERE ss.pubkey = e.pubkey
                AND ss.status = 'junked'
            )
            -- Only show messages from contacts, allowed senders, or your accounts
            AND (
                e.pubkey IN (SELECT pubkey FROM contacts)
                OR e.pubkey IN (SELECT pubkey FROM sender_status WHERE status = 'allowed')
                OR e.pubkey IN (SELECT pubkey FROM pubkeys)
            )
            AND (
                LOWER(e.content) LIKE LOWER(?1)
                OR EXISTS (
                    SELECT 1 FROM json_each(e.tags) AS stag
                    WHERE jsonb_extract(stag.value, '$[0]') = 'subject'
                    AND LOWER(jsonb_extract(stag.value, '$[1]')) LIKE LOWER(?1)
                )
                OR LOWER(pm.name) LIKE LOWER(?1)
                OR LOWER(pm.display_name) LIKE LOWER(?1)
            )
            ORDER BY e.created_at DESC
            LIMIT 100",
        )?;

        let params = rusqlite::params![&search_pattern, mail_kind];

        let entries: Vec<TableEntry> = stmt
            .query_map(params, |row| {
                Ok(TableEntry {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    created_at: row.get(2)?,
                    pubkey: row.get(3)?,
                    subject: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    thread_count: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }
}
