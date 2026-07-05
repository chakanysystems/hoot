use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use include_dir::{include_dir, Dir};
use nostr::PublicKey;
use rusqlite::Connection;
use rusqlite_migration::Migrations;
use serde::Deserialize;
use tracing::{debug, info};

mod contacts;
mod drafts;
mod events;
mod nip05;
mod queries;
pub mod sender_status;

pub use drafts::Draft;

static MIGRATIONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

static MIGRATIONS: LazyLock<Migrations<'static>> =
    LazyLock::new(|| Migrations::from_directory(&MIGRATIONS_DIR).unwrap());

pub struct Db {
    pub(crate) connection: Connection,
}

impl Db {
    pub fn new(path: PathBuf) -> Result<Self> {
        debug!("Loading database at location {:?}", path.to_str());
        let conn = Connection::open(path)?;

        Ok(Self { connection: conn })
    }

    #[cfg(test)]
    pub fn new_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;

        MIGRATIONS.to_latest(&mut conn)?;

        Ok(Self { connection: conn })
    }

    pub fn unlock_with_password(&mut self, password: String) -> Result<()> {
        self.connection.pragma_update(None, "key", password)?;

        // Apply migrations
        info!("Running Migrations");
        MIGRATIONS.to_latest(&mut self.connection)?;

        Ok(())
    }

    #[cfg(test)]
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
}

/// A temporary struct to deserialize the raw JSON event from the database.
/// This makes parsing safe and reliable.
#[derive(Deserialize)]
pub(super) struct RawEventData {
    pub id: String,
    pub content: String,
    pub created_at: i64,
    pub tags: Vec<Vec<String>>,
    pub pubkey: PublicKey,
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

    #[test]
    fn initializes_new_file_database() -> Result<()> {
        let path = std::env::temp_dir().join(format!(
            "hoot-test-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));

        let mut db = Db::new(path.clone())?;
        db.unlock_with_password("test-password".to_string())?;
        let saved_list = db.get_pubkeys()?;
        assert!(saved_list.is_empty());

        std::fs::remove_file(path)?;
        Ok(())
    }

    fn temp_db_path(name: &str) -> Result<PathBuf> {
        Ok(std::env::temp_dir().join(format!(
            "hoot-{name}-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        )))
    }

    #[test]
    fn in_memory_database_is_unlocked_and_initialized_after_migrations() -> Result<()> {
        let db = Db::new_in_memory()?;

        assert!(db.is_unlocked());
        assert!(db.is_initialized());
        Ok(())
    }

    #[test]
    fn file_database_reports_uninitialized_until_unlocked_and_migrated() -> Result<()> {
        let path = temp_db_path("initialization-state")?;
        let mut db = Db::new(path.clone())?;

        assert!(!db.is_initialized());
        db.unlock_with_password("test-password".to_string())?;
        assert!(db.is_initialized());

        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn encrypted_database_requires_the_original_password_to_unlock() -> Result<()> {
        let path = temp_db_path("wrong-password")?;
        {
            let mut db = Db::new(path.clone())?;
            db.unlock_with_password("correct-password".to_string())?;
            assert!(db.is_unlocked());
        }

        let mut wrong_password = Db::new(path.clone())?;
        let err = wrong_password
            .unlock_with_password("wrong-password".to_string())
            .unwrap_err();
        assert_eq!(format_unlock_error(&err), "Wrong password");

        let mut correct_password = Db::new(path.clone())?;
        correct_password.unlock_with_password("correct-password".to_string())?;
        assert!(correct_password.is_unlocked());

        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn format_unlock_error_preserves_non_sqlcipher_errors() {
        let err = anyhow::anyhow!("ordinary failure");

        assert_eq!(
            format_unlock_error(&err),
            "Database error: ordinary failure"
        );
    }
}
