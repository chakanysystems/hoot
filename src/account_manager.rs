use crate::keystorage::{Error, KeyStorage, KeyStorageType, Result};
use nostr::nips::nip59::UnwrappedGift;
use nostr::{Event, Keys};
use pollster::FutureExt as _;

pub struct AccountManager {
    pub loaded_keys: Vec<Keys>,
}

impl AccountManager {
    pub fn new() -> Self {
        Self {
            loaded_keys: Vec::new(),
        }
    }

    pub fn unwrap_gift_wrap(&mut self, gift_wrap: &Event) -> Result<UnwrappedGift> {
        let target_pubkey = gift_wrap
            .tags
            .iter()
            .find(|tag| tag.kind() == "p".into())
            .and_then(|tag| tag.content())
            .ok_or(Error::KeyNotFound)?;

        let target_key = self
            .loaded_keys
            .iter()
            .find(|key| key.public_key().to_string() == *target_pubkey)
            .ok_or(Error::KeyNotFound)?;

        UnwrappedGift::from_gift_wrap(target_key, gift_wrap)
            .block_on()
            .map_err(|e| Error::UnwrappingFailed(e.to_string()))
    }

    pub fn generate_keys(&mut self) -> Result<Keys> {
        let new_keypair = Keys::generate();
        self.loaded_keys.push(new_keypair.clone());

        Ok(new_keypair)
    }

    pub fn load_keys(&mut self) -> Result<Vec<Keys>> {
        let mut keys = self.get_keys()?;
        keys.append(&mut self.loaded_keys);
        keys.dedup();
        self.loaded_keys = keys.clone();

        Ok(keys)
    }

    pub fn delete_key(&mut self, key: &Keys) -> Result<()> {
        self.remove_key(key)?;
        if let Some(index) = self.loaded_keys.iter().position(|k| k == key) {
            self.loaded_keys.remove(index);
        }

        Ok(())
    }

    #[allow(unreachable_code)]
    fn get_platform_keystorage() -> KeyStorageType {
        #[cfg(target_os = "linux")]
        {
            return KeyStorageType::Linux;
        }

        #[cfg(target_os = "macos")]
        {
            return KeyStorageType::MacOS;
        }

        #[cfg(target_os = "windows")]
        {
            return KeyStorageType::Windows;
        }

        #[cfg(not(target_os = "linux"))]
        KeyStorageType::None
    }
}
impl KeyStorage for AccountManager {
    fn get_keys(&self) -> Result<Vec<Keys>> {
        Self::get_platform_keystorage().get_keys()
    }

    fn add_key(&self, key: &Keys) -> Result<()> {
        Self::get_platform_keystorage().add_key(key)
    }

    fn remove_key(&self, key: &Keys) -> Result<()> {
        Self::get_platform_keystorage().remove_key(key)
    }
}
