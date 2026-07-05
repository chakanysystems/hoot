use anyhow::Result;
use rusqlite::OptionalExtension;

use crate::dto::ProfileMetadata;

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
    #[cfg(test)]
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
    #[cfg(test)]
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
            "SELECT EXISTS (SELECT 1 FROM profile_metadata WHERE pubkey = ?1 AND created_at >= ?2)",
            (pubkey.to_string(), created_at),
            |row| row.get(0),
        )?;
        // If no record exists or the existing one is older, this is newer
        Ok(!exists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{Event, EventBuilder, Keys, Kind, Timestamp};

    fn metadata_event(
        keys: &Keys,
        created_at: u64,
        name: &str,
        display_name: &str,
        picture: &str,
        nip05: &str,
    ) -> Event {
        let metadata = ProfileMetadata {
            name: Some(name.to_string()),
            display_name: Some(display_name.to_string()),
            picture: Some(picture.to_string()),
            nip05: Some(nip05.to_string()),
        };
        EventBuilder::new(
            Kind::Metadata,
            serde_json::to_string(&metadata).expect("metadata should serialize"),
        )
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("metadata event should sign")
    }

    fn insert_profile_metadata_row(
        db: &Db,
        pubkey: &str,
        name: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<()> {
        db.connection.execute(
            "INSERT INTO profile_metadata (pubkey, id, name, display_name, picture, nip05, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, 42)",
            (pubkey, format!("{pubkey}-metadata"), name, display_name),
        )?;
        Ok(())
    }

    #[test]
    fn contacts_lifecycle_preserves_petnames_and_defaults_missing_metadata() -> Result<()> {
        let db = Db::new_in_memory()?;
        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();

        assert!(!db.is_contact(&pubkey)?);
        assert_eq!(db.get_contact_petname(&pubkey)?, None);

        db.save_contact(&pubkey, Some("Alice"))?;
        assert!(db.is_contact(&pubkey)?);
        assert_eq!(db.get_contact_petname(&pubkey)?.as_deref(), Some("Alice"));
        let contacts = db.get_user_contacts()?;
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].0, pubkey);
        assert_eq!(contacts[0].1.as_deref(), Some("Alice"));
        assert_eq!(contacts[0].2, ProfileMetadata::default());

        db.update_contact_petname(&contacts[0].0, Some("Best Alice"))?;
        assert_eq!(
            db.get_contact_petname(&contacts[0].0)?.as_deref(),
            Some("Best Alice")
        );

        db.save_contact(&contacts[0].0, None)?;
        assert_eq!(db.get_contact_petname(&contacts[0].0)?, None);

        db.delete_contact(&contacts[0].0)?;
        assert!(!db.is_contact(&contacts[0].0)?);
        assert!(db.get_user_contacts()?.is_empty());

        Ok(())
    }

    #[test]
    fn get_user_contacts_orders_by_petname_then_profile_name_then_pubkey() -> Result<()> {
        let db = Db::new_in_memory()?;
        db.save_contact("pubkey-z", None)?;
        db.save_contact("pubkey-b", None)?;
        db.save_contact("pubkey-c", Some("Amber Alias"))?;
        insert_profile_metadata_row(&db, "pubkey-b", Some("Beta Name"), None)?;
        insert_profile_metadata_row(&db, "pubkey-c", Some("Zulu Name"), Some("Zulu Display"))?;

        let contacts = db.get_user_contacts()?;

        assert_eq!(
            contacts
                .iter()
                .map(|(pubkey, petname, _)| (pubkey.as_str(), petname.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("pubkey-c", Some("Amber Alias")),
                ("pubkey-b", None),
                ("pubkey-z", None),
            ]
        );
        Ok(())
    }

    #[test]
    fn profile_metadata_updates_only_when_event_is_newer() -> Result<()> {
        let db = Db::new_in_memory()?;
        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();

        db.update_profile_metadata(metadata_event(
            &keys,
            100,
            "alice",
            "Alice",
            "https://example.com/alice.png",
            "alice@example.com",
        ))?;
        assert_eq!(
            db.get_profile_metadata(&pubkey)?
                .unwrap()
                .display_name
                .as_deref(),
            Some("Alice")
        );

        db.update_profile_metadata(metadata_event(
            &keys,
            100,
            "same-timestamp",
            "Same Timestamp",
            "https://example.com/same.png",
            "same@example.com",
        ))?;
        assert_eq!(
            db.get_profile_metadata(&pubkey)?
                .unwrap()
                .display_name
                .as_deref(),
            Some("Alice"),
            "same-timestamp metadata must not replace the cached profile"
        );

        db.update_profile_metadata(metadata_event(
            &keys,
            50,
            "older",
            "Older",
            "https://example.com/old.png",
            "old@example.com",
        ))?;
        assert_eq!(
            db.get_profile_metadata(&pubkey)?
                .unwrap()
                .display_name
                .as_deref(),
            Some("Alice"),
            "older metadata must not replace the cached profile"
        );

        db.update_profile_metadata(metadata_event(
            &keys,
            200,
            "newer",
            "Newer",
            "https://example.com/new.png",
            "new@example.com",
        ))?;
        let metadata = db.get_profile_metadata(&pubkey)?.unwrap();
        assert_eq!(metadata.name.as_deref(), Some("newer"));
        assert_eq!(metadata.display_name.as_deref(), Some("Newer"));
        assert_eq!(
            metadata.picture.as_deref(),
            Some("https://example.com/new.png")
        );
        assert_eq!(metadata.nip05.as_deref(), Some("new@example.com"));

        Ok(())
    }

    #[test]
    fn get_contacts_returns_profile_metadata_sorted_by_display_name_name_then_pubkey() -> Result<()>
    {
        let db = Db::new_in_memory()?;
        let zed = Keys::generate();
        let amy = Keys::generate();

        db.write_profile_metadata(metadata_event(
            &zed,
            100,
            "zed",
            "Zed",
            "https://example.com/zed.png",
            "zed@example.com",
        ))?;
        db.write_profile_metadata(metadata_event(
            &amy,
            100,
            "amy",
            "Amy",
            "https://example.com/amy.png",
            "amy@example.com",
        ))?;

        let contacts = db.get_contacts()?;

        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].0, amy.public_key().to_hex());
        assert_eq!(contacts[0].1.display_name.as_deref(), Some("Amy"));
        assert_eq!(contacts[0].1.nip05.as_deref(), Some("amy@example.com"));
        assert_eq!(contacts[1].0, zed.public_key().to_hex());
        assert_eq!(contacts[1].1.display_name.as_deref(), Some("Zed"));
        assert_eq!(
            contacts[1].1.picture.as_deref(),
            Some("https://example.com/zed.png")
        );

        Ok(())
    }
    #[test]
    fn write_profile_metadata_rejects_non_metadata_events() -> Result<()> {
        let db = Db::new_in_memory()?;
        let event = EventBuilder::new(Kind::TextNote, "not metadata")
            .sign_with_keys(&Keys::generate())
            .expect("text note should sign");

        let err = db.write_profile_metadata(event).unwrap_err();

        assert!(
            err.to_string().contains("not a kind 0 event"),
            "unexpected error: {err}"
        );
        Ok(())
    }
}
