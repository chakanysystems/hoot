use anyhow::Result;
use rusqlite::OptionalExtension;

use super::Db;

#[derive(Debug, Clone, PartialEq)]
pub enum SenderStatus {
    Allowed,
    Junked,
}

impl SenderStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SenderStatus::Allowed => "allowed",
            SenderStatus::Junked => "junked",
        }
    }
}

impl Db {
    pub fn set_sender_status(&self, pubkey: &str, status: &SenderStatus) -> Result<()> {
        self.connection.execute(
            "INSERT INTO sender_status (pubkey, status)
             VALUES (?1, ?2)
             ON CONFLICT(pubkey) DO UPDATE SET status = ?2, updated_at = unixepoch()",
            (pubkey, status.as_str()),
        )?;
        Ok(())
    }

    pub fn get_sender_status(&self, pubkey: &str) -> Result<Option<SenderStatus>> {
        let result: Option<String> = self
            .connection
            .query_row(
                "SELECT status FROM sender_status WHERE pubkey = ?1",
                [pubkey],
                |row| row.get(0),
            )
            .optional()?;

        result
            .map(|s| match s.as_str() {
                "allowed" => Ok(SenderStatus::Allowed),
                "junked" => Ok(SenderStatus::Junked),
                other => anyhow::bail!("Unknown sender status: {}", other),
            })
            .transpose()
    }

    pub fn remove_sender_status(&self, pubkey: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM sender_status WHERE pubkey = ?1", (pubkey,))?;
        Ok(())
    }

    pub fn get_senders_by_status(
        &self,
        status: &SenderStatus,
    ) -> Result<Vec<(String, Option<String>, Option<String>, Option<String>, i64)>> {
        let mut stmt = self.connection.prepare(
            "SELECT ss.pubkey, pm.name, pm.display_name, pm.picture, ss.created_at
             FROM sender_status ss
             LEFT JOIN profile_metadata pm ON ss.pubkey = pm.pubkey
             WHERE ss.status = ?1
             ORDER BY ss.created_at DESC",
        )?;

        let rows = stmt.query_map([status.as_str()], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?;

        let results = rows.collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(results)
    }

    pub fn is_known_sender(&self, pubkey: &str) -> Result<bool> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM (
                SELECT 1 FROM contacts WHERE pubkey = ?1
                UNION ALL
                SELECT 1 FROM sender_status WHERE pubkey = ?1 AND status = 'allowed'
            )",
            [pubkey],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn is_sender_junked(&self, pubkey: &str) -> Result<bool> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM sender_status WHERE pubkey = ?1 AND status = 'junked'",
            [pubkey],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_profile_metadata(
        db: &Db,
        pubkey: &str,
        name: &str,
        display_name: &str,
        picture: &str,
    ) -> Result<()> {
        db.connection.execute(
            "INSERT INTO profile_metadata (pubkey, id, name, display_name, picture, nip05, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, 42)",
            (pubkey, format!("{pubkey}-profile"), name, display_name, picture),
        )?;
        Ok(())
    }

    #[test]
    fn sender_status_lifecycle_controls_known_and_junked_sender_queries() -> Result<()> {
        let db = Db::new_in_memory()?;
        let allowed = "allowed-pubkey";
        let junked = "junked-pubkey";
        let contact = "contact-pubkey";

        assert_eq!(SenderStatus::Allowed.as_str(), "allowed");
        assert_eq!(SenderStatus::Junked.as_str(), "junked");
        assert_eq!(db.get_sender_status(allowed)?, None);
        assert!(!db.is_known_sender(allowed)?);
        assert!(!db.is_sender_junked(junked)?);

        db.set_sender_status(allowed, &SenderStatus::Allowed)?;
        db.set_sender_status(junked, &SenderStatus::Junked)?;
        db.save_contact(contact, Some("Known contact"))?;

        assert_eq!(db.get_sender_status(allowed)?, Some(SenderStatus::Allowed));
        assert_eq!(db.get_sender_status(junked)?, Some(SenderStatus::Junked));
        assert!(db.is_known_sender(allowed)?);
        assert!(db.is_known_sender(contact)?);
        assert!(!db.is_known_sender(junked)?);
        assert!(db.is_sender_junked(junked)?);

        db.set_sender_status(allowed, &SenderStatus::Junked)?;
        assert_eq!(db.get_sender_status(allowed)?, Some(SenderStatus::Junked));
        assert!(!db.is_known_sender(allowed)?);
        assert!(db.is_sender_junked(allowed)?);

        db.remove_sender_status(allowed)?;
        db.remove_sender_status(allowed)?;
        assert_eq!(db.get_sender_status(allowed)?, None);
        assert!(!db.is_sender_junked(allowed)?);

        Ok(())
    }

    #[test]
    fn get_senders_by_status_joins_profile_metadata_and_filters_status() -> Result<()> {
        let db = Db::new_in_memory()?;
        let allowed = "allowed-pubkey";
        let junked = "junked-pubkey";
        insert_profile_metadata(&db, allowed, "alice", "Alice", "https://example.com/a.png")?;
        insert_profile_metadata(
            &db,
            junked,
            "mallory",
            "Mallory",
            "https://example.com/m.png",
        )?;
        db.set_sender_status(allowed, &SenderStatus::Allowed)?;
        db.set_sender_status(junked, &SenderStatus::Junked)?;

        let allowed_rows = db.get_senders_by_status(&SenderStatus::Allowed)?;
        assert_eq!(allowed_rows.len(), 1);
        assert_eq!(allowed_rows[0].0, allowed);
        assert_eq!(allowed_rows[0].1.as_deref(), Some("alice"));
        assert_eq!(allowed_rows[0].2.as_deref(), Some("Alice"));
        assert_eq!(
            allowed_rows[0].3.as_deref(),
            Some("https://example.com/a.png")
        );
        assert!(allowed_rows[0].4 > 0);

        let junked_rows = db.get_senders_by_status(&SenderStatus::Junked)?;
        assert_eq!(junked_rows.len(), 1);
        assert_eq!(junked_rows[0].0, junked);

        Ok(())
    }

    #[test]
    fn remove_sender_status_removes_only_that_sender_from_status_filters() -> Result<()> {
        let db = Db::new_in_memory()?;
        let removed = "removed-pubkey";
        let still_allowed = "still-allowed-pubkey";
        let junked = "junked-pubkey";
        insert_profile_metadata(
            &db,
            removed,
            "removed",
            "Removed",
            "https://example.com/removed.png",
        )?;
        insert_profile_metadata(
            &db,
            still_allowed,
            "allowed",
            "Allowed",
            "https://example.com/allowed.png",
        )?;

        db.set_sender_status(removed, &SenderStatus::Allowed)?;
        db.set_sender_status(still_allowed, &SenderStatus::Allowed)?;
        db.set_sender_status(junked, &SenderStatus::Junked)?;
        db.remove_sender_status("missing-pubkey")?;
        db.remove_sender_status(removed)?;

        assert_eq!(db.get_sender_status(removed)?, None);
        assert!(!db.is_known_sender(removed)?);
        assert_eq!(
            db.get_senders_by_status(&SenderStatus::Allowed)?
                .into_iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            vec![still_allowed.to_string()]
        );
        assert_eq!(
            db.get_senders_by_status(&SenderStatus::Junked)?
                .into_iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            vec![junked.to_string()]
        );

        Ok(())
    }

    #[test]
    fn get_sender_status_reports_unknown_database_values() -> Result<()> {
        let db = Db::new_in_memory()?;
        db.connection
            .pragma_update(None, "ignore_check_constraints", true)?;
        db.connection.execute(
            "INSERT INTO sender_status (pubkey, status) VALUES ('bad-pubkey', 'muted')",
            [],
        )?;

        let err = db.get_sender_status("bad-pubkey").unwrap_err();

        assert!(
            err.to_string().contains("Unknown sender status: muted"),
            "unexpected error: {err}"
        );
        Ok(())
    }
}
