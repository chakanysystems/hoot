#![cfg(target_os = "windows")]
use super::{Error, KeyStorage, basic_file_storage::BasicFileStorage};
use nostr::Keys;

pub struct WindowsKeyStorage {}

impl WindowsKeyStorage {
    pub fn new() -> Self {
        Self {}
    }
}

impl KeyStorage for WindowsKeyStorage {
    fn get_keys(&self) -> Result<Vec<Keys>, Error> {
        let bfs = BasicFileStorage::new().get_keys()?;
        Ok(bfs)
    }
    fn add_key(&self, key: &Keys) -> Result<(), Error> {
        BasicFileStorage::new().add_key(key)?;
        Ok(())
    }
    fn remove_key(&self, key: &Keys) -> Result<(), Error> {
        BasicFileStorage::new().remove_key(key)?;
        Ok(())
    }
}