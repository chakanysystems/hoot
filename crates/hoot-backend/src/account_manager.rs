use crate::db::Db;
use crate::STORAGE_NAME;
use anyhow::{Context, Result};
use keyring::Entry;
use nostr::nips::nip59::UnwrappedGift;
use nostr::{Event, EventBuilder, Keys, SecretKey};
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

    pub fn create_auth_event(keys: &Keys, relay_url: &str, challenge: &str) -> Result<Event> {
        use nostr::RelayUrl;

        let relay_url_parsed =
            RelayUrl::parse(relay_url).map_err(|e| anyhow::anyhow!("Invalid relay URL: {}", e))?;

        let event = EventBuilder::auth(challenge, relay_url_parsed)
            .sign_with_keys(keys)
            .map_err(|e| anyhow::anyhow!("Failed to sign auth event: {}", e))?;

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyring::credential::{
        Credential, CredentialApi, CredentialBuilderApi, CredentialPersistence,
    };
    use nostr::{EventBuilder, Keys, Kind, TagKind, ToBech32};
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    /// Global shared store so credentials persist across Entry instances (like a real keystore).
    static MOCK_STORE: LazyLock<Mutex<HashMap<String, Vec<u8>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    /// Serializes tests because keyring's default credential builder and the mock store are global.
    static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn lock_mock_store() -> MutexGuard<'static, HashMap<String, Vec<u8>>> {
        match MOCK_STORE.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn lock_tests() -> MutexGuard<'static, ()> {
        match TEST_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[derive(Debug)]
    struct SharedMockCredential {
        key: String,
    }

    impl CredentialApi for SharedMockCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            lock_mock_store().insert(self.key.clone(), secret.to_vec());
            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            lock_mock_store()
                .get(&self.key)
                .cloned()
                .ok_or(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            lock_mock_store()
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

    fn setup() -> MutexGuard<'static, ()> {
        let guard = lock_tests();
        lock_mock_store().clear();
        keyring::set_default_credential_builder(Box::new(SharedMockCredentialBuilder));
        guard
    }

    fn mock_store_key(pubkey_hex: &str) -> String {
        format!("{}:{}", STORAGE_NAME, pubkey_hex)
    }

    fn store_secret(keys: &Keys) {
        let pubkey_hex = keys.public_key().to_hex();
        lock_mock_store().insert(
            mock_store_key(&pubkey_hex),
            keys.secret_key().as_secret_bytes().to_vec(),
        );
    }

    fn store_malformed_secret(pubkey_hex: &str, secret: Vec<u8>) {
        lock_mock_store().insert(mock_store_key(pubkey_hex), secret);
    }

    fn keyring_secret_for(pubkey_hex: &str) -> keyring::Result<Vec<u8>> {
        Entry::new(STORAGE_NAME, pubkey_hex)?.get_secret()
    }

    #[test]
    fn test_validate_nsec_accepts_generated_nsec_and_rejects_invalid_input() -> Result<()> {
        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32()?;

        let validated_keys = validate_nsec(&nsec).map_err(anyhow::Error::msg)?;
        assert_eq!(validated_keys.public_key(), keys.public_key());

        assert_eq!(validate_nsec("").unwrap_err(), "Please enter a private key");
        assert_eq!(
            validate_nsec("not-an-nsec").unwrap_err(),
            "Invalid nsec format"
        );

        Ok(())
    }

    #[test]
    fn test_generate_key_and_save_in_memory() -> Result<()> {
        let _guard = setup();
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
    fn test_load_keys_skips_missing_keyring_entries_and_replaces_loaded_keys() -> Result<()> {
        let _guard = setup();
        let db = Db::new_in_memory()?;
        let valid_keys = Keys::generate();
        let missing_keyring_keys = Keys::generate();
        let stale_loaded_keys = Keys::generate();

        db.add_pubkey(valid_keys.public_key().to_hex())?;
        db.add_pubkey(missing_keyring_keys.public_key().to_hex())?;
        store_secret(&valid_keys);

        let mut account_manager = AccountManager {
            loaded_keys: vec![stale_loaded_keys],
        };
        let loaded_keys = account_manager.load_keys(&db)?;

        assert_eq!(loaded_keys, vec![valid_keys.clone()]);
        assert_eq!(account_manager.loaded_keys, vec![valid_keys]);

        Ok(())
    }

    #[test]
    fn test_load_keys_skips_malformed_keyring_secrets() -> Result<()> {
        let _guard = setup();
        let db = Db::new_in_memory()?;
        let valid_keys = Keys::generate();
        let malformed_secret_pubkey = Keys::generate().public_key().to_hex();

        db.add_pubkey(valid_keys.public_key().to_hex())?;
        db.add_pubkey(malformed_secret_pubkey.clone())?;
        store_secret(&valid_keys);
        store_malformed_secret(&malformed_secret_pubkey, vec![0; 31]);

        let mut account_manager = AccountManager::new();
        let loaded_keys = account_manager.load_keys(&db)?;

        assert_eq!(loaded_keys, vec![valid_keys.clone()]);
        assert_eq!(account_manager.loaded_keys, vec![valid_keys]);

        Ok(())
    }

    #[test]
    fn test_save_keys_persists_to_db_and_keyring_for_fresh_load() -> Result<()> {
        let _guard = setup();
        let db = Db::new_in_memory()?;
        let keys = Keys::generate();
        let pubkey_hex = keys.public_key().to_hex();

        let mut saving_manager = AccountManager::new();
        saving_manager.save_keys(&db, &keys)?;

        assert_eq!(db.get_pubkeys()?, vec![pubkey_hex.clone()]);
        assert_eq!(
            keyring_secret_for(&pubkey_hex)?,
            keys.secret_key().as_secret_bytes().to_vec()
        );

        let mut loading_manager = AccountManager::new();
        let loaded_keys = loading_manager.load_keys(&db)?;
        assert_eq!(loaded_keys, vec![keys.clone()]);
        assert_eq!(loading_manager.loaded_keys, vec![keys]);

        Ok(())
    }

    #[test]
    fn test_delete_keys_removes_loaded_db_and_keyring_entries() -> Result<()> {
        let _guard = setup();
        let db = Db::new_in_memory()?;

        let mut account_manager = AccountManager::new();
        let generated_keys = account_manager.generate_new_keys_and_save(&db)?;
        assert!(account_manager.loaded_keys.first().is_some());
        account_manager.delete_key(&db, &generated_keys)?;
        assert_eq!(account_manager.loaded_keys.len(), 0);

        let entry = Entry::new(STORAGE_NAME, &generated_keys.public_key().to_hex())?;
        assert!(matches!(entry.get_secret(), Err(keyring::Error::NoEntry)));

        let db_keys = db.get_pubkeys()?;
        assert!(db_keys.is_empty());

        Ok(())
    }

    #[test]
    fn test_delete_key_reports_missing_keyring_entry_after_db_delete() -> Result<()> {
        let _guard = setup();
        let db = Db::new_in_memory()?;
        let keys = Keys::generate();
        let pubkey_hex = keys.public_key().to_hex();
        db.add_pubkey(pubkey_hex.clone())?;

        let mut account_manager = AccountManager {
            loaded_keys: vec![keys.clone()],
        };
        let error = account_manager.delete_key(&db, &keys).unwrap_err();

        assert!(error
            .to_string()
            .contains("Tried to delete keyring entry for public key"));
        assert!(db.get_pubkeys()?.is_empty());
        assert_eq!(account_manager.loaded_keys, vec![keys]);

        Ok(())
    }

    #[test]
    fn test_delete_key_succeeds_when_db_row_is_missing_but_keyring_entry_exists() -> Result<()> {
        let _guard = setup();
        let db = Db::new_in_memory()?;
        let keys = Keys::generate();
        let pubkey_hex = keys.public_key().to_hex();
        store_secret(&keys);

        let mut account_manager = AccountManager {
            loaded_keys: vec![keys],
        };
        let key_to_delete = account_manager.loaded_keys[0].clone();
        account_manager.delete_key(&db, &key_to_delete)?;

        assert!(matches!(
            keyring_secret_for(&pubkey_hex),
            Err(keyring::Error::NoEntry)
        ));
        assert!(account_manager.loaded_keys.is_empty());
        assert!(db.get_pubkeys()?.is_empty());

        Ok(())
    }

    #[test]
    fn unwrap_gift_wrap_selects_the_recipient_key_from_the_p_tag() -> Result<()> {
        let sender = Keys::generate();
        let recipient = Keys::generate();
        let wrong_recipient = Keys::generate();
        let rumor = EventBuilder::new(Kind::TextNote, "wrapped secret");
        let gift_wrap = EventBuilder::gift_wrap(&sender, &recipient.public_key(), rumor, None)
            .block_on()
            .map_err(anyhow::Error::msg)?;
        let mut account_manager = AccountManager {
            loaded_keys: vec![wrong_recipient, recipient.clone()],
        };

        let unwrapped = account_manager.unwrap_gift_wrap(&gift_wrap)?;

        assert_eq!(unwrapped.sender, sender.public_key());
        assert_eq!(unwrapped.rumor.pubkey, sender.public_key());
        assert_eq!(unwrapped.rumor.content, "wrapped secret");
        Ok(())
    }
    #[test]
    fn unwrap_gift_wrap_reports_target_pubkey_when_loaded_key_is_missing() -> Result<()> {
        let sender = Keys::generate();
        let recipient = Keys::generate();
        let wrong_recipient = Keys::generate();
        let rumor = EventBuilder::new(Kind::TextNote, "wrapped for someone else");
        let gift_wrap = EventBuilder::gift_wrap(&sender, &recipient.public_key(), rumor, None)
            .block_on()
            .map_err(anyhow::Error::msg)?;
        let mut account_manager = AccountManager {
            loaded_keys: vec![wrong_recipient],
        };

        let error = account_manager.unwrap_gift_wrap(&gift_wrap).unwrap_err();
        let message = error.to_string();

        assert!(message.contains(&recipient.public_key().to_string()));
        assert!(message.contains(&gift_wrap.id.to_string()));
        Ok(())
    }
    #[test]
    fn test_create_auth_event() -> Result<()> {
        let keys = Keys::generate();
        let relay_url = "wss://relay.example.com";
        let challenge = "test-challenge-123";

        let event = AccountManager::create_auth_event(&keys, relay_url, challenge)?;

        // Verify event properties
        assert_eq!(event.kind, Kind::Authentication);
        assert_eq!(event.content, "");

        // Verify the event has relay and challenge tags
        let has_relay_tag = event.tags.iter().any(|tag| {
            tag.kind() == TagKind::Relay && tag.content().map(|c| c == relay_url).unwrap_or(false)
        });
        let has_challenge_tag = event.tags.iter().any(|tag| {
            tag.kind() == TagKind::Challenge
                && tag.content().map(|c| c == challenge).unwrap_or(false)
        });

        assert!(has_relay_tag, "Auth event should have relay tag");
        assert!(has_challenge_tag, "Auth event should have challenge tag");

        // Verify signature is valid
        assert!(
            event.verify().is_ok(),
            "Auth event signature should be valid"
        );

        Ok(())
    }
}
