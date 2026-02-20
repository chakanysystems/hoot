use anyhow::Result;
use rusqlite::OptionalExtension;

use crate::profile_metadata::ProfileMetadata;

use super::Db;

impl Db {
    // there is a high chance i am a retard.
    // there is a very high chance that there is a better way to do this
    // but it's not coming to mind! guess we'll find out.
    // context: see profile_metadata sql definition and compare to events definition

    pub fn get_profile_metadata(&self, pubkey: &str) -> Result<Option<ProfileMetadata>> {
        let mut stmt = self.connection.prepare(
            "SELECT name, display_name, picture, nip05 FROM profile_metadata WHERE pubkey = ?",
        )?;

        Ok(stmt
            .query_one([pubkey], |row| {
                Ok(ProfileMetadata {
                    name: row.get(0)?,
                    display_name: row.get(1)?,
                    picture: row.get(2)?,
                    nip05: row.get(3)?,
                })
            })
            .optional()?)
    }

    pub fn get_contacts(&self) -> Result<Vec<(String, ProfileMetadata)>> {
        let mut stmt = self.connection.prepare(
            "SELECT pubkey, name, display_name, picture, nip05
             FROM profile_metadata
             ORDER BY LOWER(COALESCE(display_name, name, pubkey))",
        )?;

        let contacts_iter = stmt.query_map([], |row| {
            let pubkey: String = row.get(0)?;
            let metadata = ProfileMetadata {
                name: row.get(1)?,
                display_name: row.get(2)?,
                picture: row.get(3)?,
                nip05: row.get(4)?,
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
            .execute("REPLACE INTO profile_metadata (pubkey, id, name, display_name, picture, nip05, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (event.pubkey.to_string(), event.id.to_string(), meta.name, meta.display_name, meta.picture, meta.nip05, event.created_at.as_u64())
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
            "SELECT c.pubkey, c.petname, pm.name, pm.display_name, pm.picture, pm.nip05
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
                nip05: row.get(5)?,
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
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM profile_metadata WHERE pubkey = ?1 AND created_at > ?2)",
            (pubkey.to_string(), created_at),
            |row| row.get(0),
        )?;
        // If no record exists or the existing one is older, this is newer
        Ok(!exists)
    }
}
