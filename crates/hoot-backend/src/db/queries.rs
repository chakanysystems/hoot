use anyhow::Result;
use nostr::{EventId, PublicKey};

use crate::dto::TableEntry;
use crate::mail_event::{MailMessage, MAIL_EVENT_KIND};

use super::{Db, RawEventData};

#[derive(Debug)]
struct MessageListRow {
    id: String,
    content: String,
    created_at: i64,
    pubkey: String,
    subject: String,
    thread_count: i64,
}

impl MessageListRow {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            content: row.get(1)?,
            created_at: row.get(2)?,
            pubkey: row.get(3)?,
            subject: row.get(4)?,
            thread_count: row.get(5)?,
        })
    }
}

impl From<MessageListRow> for TableEntry {
    fn from(row: MessageListRow) -> Self {
        Self {
            id: row.id,
            content: row.content,
            created_at: row.created_at,
            pubkey: row.pubkey,
            subject: row.subject,
            thread_count: row.thread_count,
        }
    }
}

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
        let messages = stmt
            .query_map([], MessageListRow::from_row)?
            .map(|row| row.map(TableEntry::from))
            .collect::<Result<Vec<TableEntry>, rusqlite::Error>>()?;

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

        let messages = stmt
            .query_map([], MessageListRow::from_row)?
            .map(|row| row.map(TableEntry::from))
            .collect::<Result<Vec<TableEntry>, rusqlite::Error>>()?;
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

        let messages = stmt
            .query_map([], MessageListRow::from_row)?
            .map(|row| row.map(TableEntry::from))
            .collect::<Result<Vec<TableEntry>, rusqlite::Error>>()?;
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

        let messages = stmt
            .query_map([], MessageListRow::from_row)?
            .map(|row| row.map(TableEntry::from))
            .collect::<Result<Vec<TableEntry>, rusqlite::Error>>()?;
        Ok(messages)
    }

    /// Get all event IDs for mail events
    #[cfg(test)]
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

        let mail_kind = u32::from(MAIL_EVENT_KIND);

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

        let entries = stmt
            .query_map(params, MessageListRow::from_row)?
            .map(|row| row.map(TableEntry::from))
            .collect::<Result<Vec<TableEntry>, rusqlite::Error>>()?;

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{Event, EventBuilder, Keys, Kind, Tag, TagKind, TagStandard, Timestamp};

    #[test]
    fn table_entry_mapping_preserves_message_list_row_fields() -> Result<()> {
        let db = Db::new_in_memory()?;
        let row = db.connection.query_row(
            "SELECT 'event-id', 'body', 123, 'pubkey-hex', 'Subject line', 2",
            [],
            MessageListRow::from_row,
        )?;

        let entry = TableEntry::from(row);

        assert_eq!(entry.id, "event-id");
        assert_eq!(entry.content, "body");
        assert_eq!(entry.created_at, 123);
        assert_eq!(entry.pubkey, "pubkey-hex");
        assert_eq!(entry.subject, "Subject line");
        assert_eq!(entry.thread_count, 2);

        Ok(())
    }

    fn mail_event(
        keys: &Keys,
        subject: &str,
        content: &str,
        created_at: u64,
        parent: Option<&Event>,
    ) -> Event {
        let mut tags = vec![Tag::from_standardized(TagStandard::Subject(
            subject.to_string(),
        ))];
        if let Some(parent) = parent {
            tags.push(Tag::event(parent.id));
        }
        EventBuilder::new(Kind::Custom(MAIL_EVENT_KIND), content)
            .tags(tags)
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(keys)
            .expect("mail event should sign")
    }

    fn insert_profile_metadata(
        db: &Db,
        keys: &Keys,
        name: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<()> {
        let pubkey = keys.public_key().to_hex();
        db.connection.execute(
            "INSERT INTO profile_metadata (pubkey, id, name, display_name, picture, nip05, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, 42)",
            (pubkey.as_str(), format!("{pubkey}-profile"), name, display_name),
        )?;
        Ok(())
    }

    fn ids(entries: &[TableEntry]) -> Vec<String> {
        entries.iter().map(|entry| entry.id.clone()).collect()
    }

    #[test]
    fn mailbox_queries_classify_senders_and_exclude_trash_junk_and_requests() -> Result<()> {
        use crate::db::sender_status::SenderStatus;

        let mut db = Db::new_in_memory()?;
        let own = Keys::generate();
        let contact = Keys::generate();
        let allowed = Keys::generate();
        let request = Keys::generate();
        let junked = Keys::generate();

        db.add_pubkey(own.public_key().to_hex())?;
        db.save_contact(&contact.public_key().to_hex(), None)?;
        db.set_sender_status(&allowed.public_key().to_hex(), &SenderStatus::Allowed)?;
        db.set_sender_status(&junked.public_key().to_hex(), &SenderStatus::Junked)?;

        let own_event = mail_event(&own, "Needle own", "own body", 10, None);
        let contact_event = mail_event(&contact, "Needle contact", "contact body", 20, None);
        let allowed_event = mail_event(&allowed, "Allowed", "allowed needle body", 30, None);
        let request_event = mail_event(&request, "Needle request", "request body", 40, None);
        let junk_event = mail_event(&junked, "Needle junk", "junk body", 50, None);
        for event in [
            &own_event,
            &contact_event,
            &allowed_event,
            &request_event,
            &junk_event,
        ] {
            db.store_event(event, None, None)?;
        }

        let inbox_ids = ids(&db.get_top_level_messages()?);
        assert!(inbox_ids.contains(&own_event.id.to_string()));
        assert!(inbox_ids.contains(&contact_event.id.to_string()));
        assert!(inbox_ids.contains(&allowed_event.id.to_string()));
        assert!(!inbox_ids.contains(&request_event.id.to_string()));
        assert!(!inbox_ids.contains(&junk_event.id.to_string()));

        assert_eq!(
            ids(&db.get_request_messages()?),
            vec![request_event.id.to_string()]
        );
        assert_eq!(
            ids(&db.get_junk_messages()?),
            vec![junk_event.id.to_string()]
        );

        let search_ids = ids(&db.search_messages("needle")?);
        assert!(search_ids.contains(&own_event.id.to_string()));
        assert!(search_ids.contains(&contact_event.id.to_string()));
        assert!(search_ids.contains(&allowed_event.id.to_string()));
        assert!(!search_ids.contains(&request_event.id.to_string()));
        assert!(!search_ids.contains(&junk_event.id.to_string()));

        db.record_trash(&[contact_event.id.to_string()], 999)?;
        assert!(!ids(&db.get_top_level_messages()?).contains(&contact_event.id.to_string()));
        assert!(!ids(&db.search_messages("contact")?).contains(&contact_event.id.to_string()));
        assert_eq!(
            ids(&db.get_trash_messages()?),
            vec![contact_event.id.to_string()]
        );

        Ok(())
    }

    #[test]
    fn get_mail_event_ids_returns_only_live_mail_events() -> Result<()> {
        let mut db = Db::new_in_memory()?;
        let keys = Keys::generate();
        assert!(db.get_mail_event_ids()?.is_empty());

        let live = mail_event(&keys, "Live", "live mail", 10, None);
        let trashed = mail_event(&keys, "Trashed", "trashed mail", 20, None);
        let deleted = mail_event(&keys, "Deleted", "deleted mail", 30, None);
        let non_mail = EventBuilder::new(Kind::TextNote, "not mail")
            .custom_created_at(Timestamp::from(40))
            .sign_with_keys(&keys)
            .expect("text note should sign");
        for event in [&live, &trashed, &deleted, &non_mail] {
            db.store_event(event, None, None)?;
        }

        db.record_trash(&[trashed.id.to_string()], 999)?;
        db.record_deletions(
            &[deleted.id.to_string()],
            Some(&keys.public_key().to_hex()),
            None,
        )?;

        assert_eq!(db.get_mail_event_ids()?, vec![live.id.to_string()]);
        Ok(())
    }

    #[test]
    fn search_messages_matches_content_subject_and_profiles_for_eligible_senders() -> Result<()> {
        use crate::db::sender_status::SenderStatus;

        let db = Db::new_in_memory()?;
        let body_sender = Keys::generate();
        let subject_sender = Keys::generate();
        let name_sender = Keys::generate();
        let display_sender = Keys::generate();
        let request_sender = Keys::generate();
        let junked_sender = Keys::generate();
        let text_sender = Keys::generate();

        for sender in [
            &body_sender,
            &subject_sender,
            &name_sender,
            &display_sender,
            &text_sender,
        ] {
            db.set_sender_status(&sender.public_key().to_hex(), &SenderStatus::Allowed)?;
        }
        db.set_sender_status(&junked_sender.public_key().to_hex(), &SenderStatus::Junked)?;
        insert_profile_metadata(
            &db,
            &name_sender,
            Some("Needle Name"),
            Some("Plain Display"),
        )?;
        insert_profile_metadata(
            &db,
            &display_sender,
            Some("Plain Name"),
            Some("Needle Display"),
        )?;

        let subject_match = mail_event(&subject_sender, "NEEDLE subject", "plain body", 50, None);
        let body_match = mail_event(&body_sender, "Plain subject", "body needle", 40, None);
        let name_match = mail_event(&name_sender, "Plain subject", "plain body", 30, None);
        let display_match = mail_event(&display_sender, "Plain subject", "plain body", 20, None);
        let request_match = mail_event(&request_sender, "Needle request", "needle body", 60, None);
        let junked_match = mail_event(&junked_sender, "Needle junk", "needle body", 70, None);
        let text_note = EventBuilder::new(Kind::TextNote, "needle text note")
            .custom_created_at(Timestamp::from(80))
            .sign_with_keys(&text_sender)
            .expect("text note should sign");

        for event in [
            &subject_match,
            &body_match,
            &name_match,
            &display_match,
            &request_match,
            &junked_match,
            &text_note,
        ] {
            db.store_event(event, None, None)?;
        }

        assert_eq!(
            ids(&db.search_messages("needle")?),
            vec![
                subject_match.id.to_string(),
                body_match.id.to_string(),
                name_match.id.to_string(),
                display_match.id.to_string(),
            ]
        );
        assert!(db.search_messages("absent")?.is_empty());

        Ok(())
    }

    #[test]
    fn parse_mail_message_extracts_recipients_subject_parent_and_sender_nip05_tags() {
        let author = Keys::generate();
        let recipient = Keys::generate();
        let parent = mail_event(&author, "Parent", "parent", 1, None);
        let event = EventBuilder::new(Kind::Custom(MAIL_EVENT_KIND), "parsed body")
            .tags(vec![
                Tag::public_key(recipient.public_key()),
                Tag::event(parent.id),
                Tag::from_standardized(TagStandard::Subject("Parsed subject".to_string())),
                Tag::custom(TagKind::custom("nip05"), vec!["sender@example.com"]),
            ])
            .custom_created_at(Timestamp::from(99))
            .sign_with_keys(&author)
            .expect("mail event should sign");
        let raw = serde_json::to_string(&event).expect("event should serialize");

        let parsed = Db::parse_mail_message(&raw).expect("mail event should parse");

        assert_eq!(parsed.id, Some(event.id));
        assert_eq!(parsed.created_at, Some(99));
        assert_eq!(parsed.author, Some(author.public_key()));
        assert_eq!(parsed.content, "parsed body");
        assert_eq!(parsed.subject, "Parsed subject");
        assert_eq!(parsed.to, vec![recipient.public_key()]);
        assert_eq!(parsed.parent_events, Some(vec![parent.id]));
        assert_eq!(parsed.sender_nip05.as_deref(), Some("sender@example.com"));
    }
    #[test]
    fn thread_queries_walk_parents_and_replies_while_respecting_trash_filter() -> Result<()> {
        let mut db = Db::new_in_memory()?;
        let author = Keys::generate();
        db.add_pubkey(author.public_key().to_hex())?;
        let root = mail_event(&author, "Thread", "root body", 10, None);
        let reply = mail_event(&author, "Thread", "reply body", 20, Some(&root));
        db.store_event(&root, None, None)?;
        db.store_event(&reply, None, None)?;

        let top_level = db.get_top_level_messages()?;
        assert_eq!(top_level.len(), 1);
        assert_eq!(top_level[0].id, root.id.to_string());
        assert_eq!(top_level[0].content, "reply body");
        assert_eq!(top_level[0].thread_count, 2);

        let thread = db.get_email_thread(&reply.id.to_string())?;
        assert_eq!(thread.len(), 2);
        assert_eq!(thread[0].id, Some(root.id));
        assert_eq!(thread[0].parent_events, None);
        assert_eq!(thread[1].id, Some(reply.id));
        assert_eq!(thread[1].parent_events, Some(vec![root.id]));

        db.record_trash(&[reply.id.to_string()], 999)?;
        assert_eq!(db.get_email_thread(&root.id.to_string())?.len(), 1);
        assert_eq!(
            db.get_email_thread_including_trash(&root.id.to_string())?
                .len(),
            2
        );
        assert_eq!(db.get_mail_event_ids()?, vec![root.id.to_string()]);

        Ok(())
    }
}
