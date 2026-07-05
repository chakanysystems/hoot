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
            .query_map([pubkey], nip05_entry_from_row)?
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

        let entry = stmt.query_one([pubkey], nip05_entry_from_row).optional()?;

        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_cache_times(
        db: &Db,
        pubkey: &str,
        nip05: &str,
        first_seen: i64,
        last_verified: Option<i64>,
        last_checked: Option<i64>,
    ) -> Result<()> {
        db.connection.execute(
            "UPDATE nip05_cache
             SET first_seen = ?1, last_verified = ?2, last_checked = ?3
             WHERE pubkey = ?4 AND nip05 = ?5",
            (first_seen, last_verified, last_checked, pubkey, nip05),
        )?;
        Ok(())
    }

    #[test]
    fn add_nip05_upsert_changes_is_own_without_losing_verification_history() -> Result<()> {
        let db = Db::new_in_memory()?;

        db.add_nip05("test_pubkey", "user@example.com", false)?;
        set_cache_times(
            &db,
            "test_pubkey",
            "user@example.com",
            10,
            Some(20),
            Some(30),
        )?;

        db.add_nip05("test_pubkey", "user@example.com", true)?;

        let entries = db.get_nip05s_for_pubkey("test_pubkey")?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].nip05, "user@example.com");
        assert!(entries[0].is_own);
        assert_eq!(entries[0].first_seen, 10);
        assert_eq!(entries[0].last_verified, Some(20));
        assert_eq!(entries[0].last_checked, Some(30));

        Ok(())
    }

    #[test]
    fn failed_verification_updates_last_checked_without_clearing_last_verified() -> Result<()> {
        let db = Db::new_in_memory()?;

        db.add_nip05("test_pubkey", "user@example.com", true)?;
        set_cache_times(
            &db,
            "test_pubkey",
            "user@example.com",
            10,
            Some(20),
            Some(20),
        )?;

        db.update_nip05_verification_status("test_pubkey", "user@example.com", false)?;

        let entries = db.get_nip05s_for_pubkey("test_pubkey")?;
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].last_verified,
            Some(20),
            "a failed check must preserve the last successful verification time"
        );
        assert!(
            entries[0].last_checked.unwrap() > 20,
            "a failed check must still record that the identifier was checked"
        );

        Ok(())
    }

    #[test]
    fn verified_check_updates_last_verified_and_last_checked_together() -> Result<()> {
        let db = Db::new_in_memory()?;

        db.add_nip05("test_pubkey", "user@example.com", true)?;

        db.update_nip05_verification_status("test_pubkey", "user@example.com", true)?;

        let entries = db.get_nip05s_for_pubkey("test_pubkey")?;
        assert_eq!(entries.len(), 1);
        assert!(entries[0].last_verified.is_some());
        assert_eq!(
            entries[0].last_verified, entries[0].last_checked,
            "a successful check must make the cache both checked and verified"
        );

        Ok(())
    }

    #[test]
    fn get_cached_nip05_prefers_verified_entry_over_newer_unverified_entry() -> Result<()> {
        let db = Db::new_in_memory()?;

        db.add_nip05("test_pubkey", "old-verified@example.com", true)?;
        db.add_nip05("test_pubkey", "new-unverified@example.com", false)?;
        set_cache_times(
            &db,
            "test_pubkey",
            "old-verified@example.com",
            10,
            Some(20),
            Some(20),
        )?;
        set_cache_times(
            &db,
            "test_pubkey",
            "new-unverified@example.com",
            30,
            None,
            Some(40),
        )?;

        let cached = db
            .get_cached_nip05("test_pubkey")?
            .expect("cache should contain entries for the pubkey");
        assert_eq!(cached.nip05, "old-verified@example.com");
        assert_eq!(cached.last_verified, Some(20));

        Ok(())
    }

    #[test]
    fn get_cached_nip05_uses_most_recent_entry_when_none_are_verified() -> Result<()> {
        let db = Db::new_in_memory()?;

        db.add_nip05("test_pubkey", "old@example.com", false)?;
        db.add_nip05("test_pubkey", "new@example.com", false)?;
        set_cache_times(&db, "test_pubkey", "old@example.com", 10, None, Some(15))?;
        set_cache_times(&db, "test_pubkey", "new@example.com", 20, None, Some(25))?;

        let cached = db
            .get_cached_nip05("test_pubkey")?
            .expect("cache should contain entries for the pubkey");
        assert_eq!(cached.nip05, "new@example.com");
        assert_eq!(cached.first_seen, 20);

        Ok(())
    }

    #[test]
    fn delete_nip05_removes_only_matching_identifier_for_pubkey() -> Result<()> {
        let db = Db::new_in_memory()?;

        db.add_nip05("test_pubkey", "remove@example.com", true)?;
        db.add_nip05("test_pubkey", "keep@example.com", false)?;
        db.add_nip05("other_pubkey", "remove@example.com", true)?;

        db.delete_nip05("test_pubkey", "remove@example.com")?;

        let test_pubkey_entries = db.get_nip05s_for_pubkey("test_pubkey")?;
        assert_eq!(test_pubkey_entries.len(), 1);
        assert_eq!(test_pubkey_entries[0].nip05, "keep@example.com");

        let other_pubkey_entries = db.get_nip05s_for_pubkey("other_pubkey")?;
        assert_eq!(other_pubkey_entries.len(), 1);
        assert_eq!(other_pubkey_entries[0].nip05, "remove@example.com");

        Ok(())
    }
}
