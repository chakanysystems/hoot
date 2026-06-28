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
