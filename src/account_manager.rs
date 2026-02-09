use crate::db::Db;
use crate::STORAGE_NAME;
use anyhow::{Context, Result};
use keyring::Entry;
use nostr::nips::nip59::UnwrappedGift;
use nostr::{Event, Keys, SecretKey};
use pollster::FutureExt as _;
use tracing::{debug, error};

/// Parse and validate an nsec (bech32 private key) string, returning Keys on success.
pub fn validate_nsec(input: &str) -> Result<Keys, String> {
    if input.is_empty() {
        return Err("Please enter a private key".to_string());
    }
    use nostr::FromBech32;
    match nostr::SecretKey::from_bech32(input) {
        Ok(secret_key) => Ok(Keys::new(secret_key)),
        Err(_) => Err("Invalid nsec format".to_string()),
    }
}

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
            .with_context(|| {
                format!(
                    "Could not find pubkey inside wrapped event `{}`",
                    gift_wrap.id
                )
            })?;

        let target_key = self
            .loaded_keys
            .iter()
            .find(|key| key.public_key().to_string() == *target_pubkey)
            .with_context(|| {
                format!(
                    "Could not find pubkey `{}` inside wrapped event `{}`",
                    target_pubkey, gift_wrap.id
                )
            })?;

        let unwrapped = UnwrappedGift::from_gift_wrap(target_key, gift_wrap)
            .block_on()
            .context("Couldn't unwrap gift")?;

        Ok(unwrapped)
    }

    pub fn generate_new_keys_and_save(&mut self, db: &Db) -> Result<Keys> {
        let new_keypair = Keys::generate();

        let entry = Entry::new(STORAGE_NAME, new_keypair.public_key().to_hex().as_ref())?;
        entry.set_secret(new_keypair.secret_key().as_secret_bytes())?;

        db.add_pubkey(new_keypair.public_key().to_hex())?;

        self.loaded_keys.push(new_keypair.clone());

        Ok(new_keypair)
    }

    pub fn save_keys(&mut self, db: &Db, keys: &Keys) -> Result<()> {
        let entry = Entry::new(STORAGE_NAME, keys.public_key().to_hex().as_ref())?;
        entry.set_secret(keys.secret_key().as_secret_bytes())?;

        db.add_pubkey(keys.public_key().to_hex())?;

        self.loaded_keys.push(keys.clone());

        Ok(())
    }

    pub fn load_keys(&mut self, db: &Db) -> Result<Vec<Keys>> {
        let db_saved_pubkeys = db.get_pubkeys()?;
        let mut keypairs: Vec<Keys> = Vec::new();
        for pubkey in db_saved_pubkeys {
            let entry = match Entry::new(STORAGE_NAME, pubkey.as_ref()) {
                Ok(v) => v,
                Err(e) => {
                    error!("Couldn't create keying entry struct, skipping: {}", e);
                    continue;
                }
            };
            let privkey = match entry.get_secret() {
                Ok(v) => v,
                Err(e) => {
                    error!("Couldn't get private key from keystore, skipping: {}", e);
                    continue;
                }
            };

            debug!("key: {:?}", privkey.to_ascii_lowercase());
            let parsed_sk = match SecretKey::from_slice(&privkey) {
                Ok(key) => key,
                Err(e) => {
                    error!("Couldn't parse private key from keystore, skipping: {}", e);
                    continue;
                }
            };
            keypairs.push(Keys::new(parsed_sk));
        }
        self.loaded_keys = keypairs.clone();

        Ok(keypairs)
    }

    pub fn delete_key(&mut self, db: &Db, key: &Keys) -> Result<()> {
        let pubkey = key.public_key().to_hex();
        db.delete_pubkey(pubkey.clone()).with_context(|| {
            format!("Tried to delete public key `{}` from pubkeys table", pubkey)
        })?;
        let entry = Entry::new(STORAGE_NAME, pubkey.as_ref()).with_context(|| {
            format!(
                "Couldn't to create keyring entry struct for pubkey `{}`",
                pubkey
            )
        })?;
        entry.delete_credential().with_context(|| {
            format!("Tried to delete keyring entry for public key `{}`", pubkey)
        })?;

        if let Some(index) = self
            .loaded_keys
            .iter()
            .position(|saved_keys| saved_keys.public_key() == key.public_key())
        {
            self.loaded_keys.remove(index);
        } else {
            debug!(
                "Couldn't remove pubkey `{}` from self.loaded_keys because it wasn't found",
                key.public_key()
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyring::credential::{
        Credential, CredentialApi, CredentialBuilderApi, CredentialPersistence,
    };
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    /// Global shared store so credentials persist across Entry instances (like a real keystore).
    static MOCK_STORE: LazyLock<Mutex<HashMap<String, Vec<u8>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    #[derive(Debug)]
    struct SharedMockCredential {
        key: String,
    }

    impl CredentialApi for SharedMockCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            MOCK_STORE
                .lock()
                .unwrap()
                .insert(self.key.clone(), secret.to_vec());
            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            MOCK_STORE
                .lock()
                .unwrap()
                .get(&self.key)
                .cloned()
                .ok_or(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            MOCK_STORE
                .lock()
                .unwrap()
                .remove(&self.key)
                .map(|_| ())
                .ok_or(keyring::Error::NoEntry)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Debug)]
    struct SharedMockCredentialBuilder;

    impl CredentialBuilderApi for SharedMockCredentialBuilder {
        fn build(
            &self,
            _target: Option<&str>,
            service: &str,
            user: &str,
        ) -> keyring::Result<Box<Credential>> {
            Ok(Box::new(SharedMockCredential {
                key: format!("{}:{}", service, user),
            }))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn persistence(&self) -> CredentialPersistence {
            CredentialPersistence::UntilDelete
        }
    }

    fn setup() {
        MOCK_STORE.lock().unwrap().clear();
        keyring::set_default_credential_builder(Box::new(SharedMockCredentialBuilder));
    }

    #[test]
    fn test_generate_key_and_save_in_memory() -> Result<()> {
        setup();
        let mut account_manager = AccountManager::new();
        let db = Db::new_in_memory()?;

        let generated_keys = account_manager.generate_new_keys_and_save(&db)?;
        assert_eq!(
            account_manager.loaded_keys.first().unwrap(),
            &generated_keys
        );

        Ok(())
    }

    #[test]
    fn test_load_keys() -> Result<()> {
        setup();
        let db = Db::new_in_memory()?;

        let generated_keys;

        {
            let mut account_manager = AccountManager::new();
            generated_keys = account_manager.generate_new_keys_and_save(&db)?;
            assert!(account_manager.loaded_keys.first().is_some());
        }

        let mut account_manager = AccountManager::new();
        let loaded_keys = account_manager.load_keys(&db)?;

        assert_ne!(loaded_keys.len(), 0);
        assert_eq!(loaded_keys.first().unwrap(), &generated_keys);
        assert_eq!(loaded_keys, account_manager.loaded_keys);

        Ok(())
    }

    #[test]
    fn test_delete_keys() -> Result<()> {
        setup();
        let db = Db::new_in_memory()?;

        let mut account_manager = AccountManager::new();
        let generated_keys = account_manager.generate_new_keys_and_save(&db)?;
        assert!(account_manager.loaded_keys.first().is_some());
        account_manager.delete_key(&db, &generated_keys)?;
        assert_eq!(account_manager.loaded_keys.len(), 0); // test the remove key in-memory

        let entry = Entry::new(STORAGE_NAME, &generated_keys.public_key().to_hex())?;
        assert!(matches!(entry.get_secret(), Err(keyring::Error::NoEntry)));

        let db_keys = db.get_pubkeys()?;
        assert!(db_keys.is_empty());

        Ok(())
    }
}
