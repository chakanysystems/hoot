use anyhow::Result;
use rusqlite::OptionalExtension;

use crate::dto::Nip05Entry;

use super::Db;

fn nip05_entry_from_row(row: &rusqlite::Row) -> rusqlite::Result<Nip05Entry> {
    Ok(Nip05Entry {
        id: row.get(0)?,
        pubkey: row.get(1)?,
        nip05: row.get(2)?,
        is_own: row.get(3)?,
        first_seen: row.get(4)?,
        last_verified: row.get(5)?,
        last_checked: row.get(6)?,
    })
}

impl Db {
    /// Add a NIP-05 entry for a pubkey
    /// If the entry already exists, this will update the is_own flag
    pub fn add_nip05(&self, pubkey: &str, nip05: &str, is_own: bool) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        self.connection.execute(
            "INSERT INTO nip05_cache (pubkey, nip05, is_own, first_seen) 
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(pubkey, nip05) DO UPDATE SET is_own = ?3",
            (pubkey, nip05, is_own, now),
        )?;

        Ok(())
    }

    /// Update the verification timestamps for a NIP-05 entry
    /// Set verified to true to update last_verified, false to only update last_checked
    pub fn update_nip05_verification_status(
        &self,
        pubkey: &str,
        nip05: &str,
        verified: bool,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        if verified {
            self.connection.execute(
                "UPDATE nip05_cache 
                 SET last_verified = ?1, last_checked = ?1 
                 WHERE pubkey = ?2 AND nip05 = ?3",
                (now, pubkey, nip05),
            )?;
        } else {
            self.connection.execute(
                "UPDATE nip05_cache 
                 SET last_checked = ?1 
                 WHERE pubkey = ?2 AND nip05 = ?3",
                (now, pubkey, nip05),
            )?;
        }

        Ok(())
    }

    /// Get all NIP-05 entries for a given pubkey
    pub fn get_nip05s_for_pubkey(&self, pubkey: &str) -> Result<Vec<Nip05Entry>> {
        let mut stmt = self.connection.prepare(
            "SELECT id, pubkey, nip05, is_own, first_seen, last_verified, last_checked 
                      FROM nip05_cache 
                      WHERE pubkey = ? 
                      ORDER BY first_seen DESC",
        )?;

        let entries = stmt
            .query_map([pubkey], |row| nip05_entry_from_row(row))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(entries)
    }

    /// Delete a NIP-05 entry
    pub fn delete_nip05(&self, pubkey: &str, nip05: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM nip05_cache WHERE pubkey = ?1 AND nip05 = ?2",
            (pubkey, nip05),
        )?;

        Ok(())
    }

    /// Get the most recent cached NIP-05 entry for a pubkey
    /// Returns None if no entries exist
    pub fn get_cached_nip05(&self, pubkey: &str) -> Result<Option<Nip05Entry>> {
        let mut stmt = self.connection.prepare(
            "SELECT id, pubkey, nip05, is_own, first_seen, last_verified, last_checked 
                      FROM nip05_cache 
                      WHERE pubkey = ? 
                      ORDER BY last_verified DESC NULLS LAST, first_seen DESC 
                      LIMIT 1",
        )?;

        let entry = stmt
            .query_one([pubkey], |row| nip05_entry_from_row(row))
            .optional()?;

        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get_nip05() -> Result<()> {
        let db = Db::new_in_memory()?;

        // Add a NIP-05
        db.add_nip05("test_pubkey", "user@example.com", true)?;

        // Get it back
        let entries = db.get_nip05s_for_pubkey("test_pubkey")?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].nip05, "user@example.com");
        assert!(entries[0].is_own);

        Ok(())
    }

    #[test]
    fn test_update_verification_status() -> Result<()> {
        let db = Db::new_in_memory()?;

        db.add_nip05("test_pubkey", "user@example.com", true)?;
        db.update_nip05_verification_status("test_pubkey", "user@example.com", true)?;

        let entries = db.get_nip05s_for_pubkey("test_pubkey")?;
        assert!(entries[0].last_verified.is_some());
        assert!(entries[0].last_checked.is_some());

        Ok(())
    }

    #[test]
    fn test_delete_nip05() -> Result<()> {
        let db = Db::new_in_memory()?;

        db.add_nip05("test_pubkey", "user@example.com", true)?;
        db.delete_nip05("test_pubkey", "user@example.com")?;

        let entries = db.get_nip05s_for_pubkey("test_pubkey")?;
        assert!(entries.is_empty());

        Ok(())
    }
}
