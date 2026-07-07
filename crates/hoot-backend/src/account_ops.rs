use crate::backend::BackendInner;
use crate::conversions::{database_error, keyring_error};
use crate::dto::{AccountSummary, ProfileMetadata};
use crate::error::HootResult;
use nostr::{Keys, ToBech32};

pub(crate) fn load_account_keys(inner: &mut BackendInner) -> HootResult<Vec<Keys>> {
    inner
        .account_manager
        .load_keys(&inner.db)
        .map_err(keyring_error)
}

pub(crate) fn generate_new_keys(inner: &mut BackendInner) -> HootResult<Keys> {
    inner
        .account_manager
        .generate_new_keys_and_save(&inner.db)
        .map_err(keyring_error)
}

pub(crate) fn save_account_keys(inner: &mut BackendInner, keys: &Keys) -> HootResult<()> {
    inner
        .account_manager
        .save_keys(&inner.db, keys)
        .map_err(keyring_error)
}

pub(crate) fn delete_account_key(inner: &mut BackendInner, keys: &Keys) -> HootResult<()> {
    inner
        .account_manager
        .delete_key(&inner.db, keys)
        .map_err(keyring_error)
}

pub(crate) fn account_summaries(inner: &BackendInner) -> HootResult<Vec<AccountSummary>> {
    inner
        .account_manager
        .loaded_keys
        .iter()
        .map(|keys| account_summary(inner, keys))
        .collect()
}

pub(crate) fn account_summary(inner: &BackendInner, keys: &Keys) -> HootResult<AccountSummary> {
    let pubkey_hex = keys.public_key().to_hex();
    let metadata = inner
        .db
        .get_profile_metadata(&pubkey_hex)
        .map_err(database_error)?;
    Ok(account_summary_for_keys(
        keys,
        metadata,
        inner.active_account_pubkey.as_deref() == Some(pubkey_hex.as_str()),
    ))
}

pub(crate) fn account_summary_for_keys(
    keys: &Keys,
    metadata: Option<ProfileMetadata>,
    is_active: bool,
) -> AccountSummary {
    let pubkey_hex = keys.public_key().to_hex();
    let npub = keys
        .public_key()
        .to_bech32()
        .unwrap_or_else(|_| pubkey_hex.clone());
    let display_name = metadata.and_then(|metadata| metadata.display_name.or(metadata.name));
    AccountSummary {
        pubkey_hex,
        npub,
        display_name,
        is_active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_summary_uses_profile_display_name_precedence_and_active_marker() {
        let keys = Keys::generate();
        let with_display_name = account_summary_for_keys(
            &keys,
            Some(ProfileMetadata {
                name: Some("alice".to_string()),
                display_name: Some("Alice Display".to_string()),
                picture: None,
                nip05: None,
            }),
            true,
        );
        assert_eq!(with_display_name.pubkey_hex, keys.public_key().to_hex());
        assert_eq!(
            with_display_name.display_name.as_deref(),
            Some("Alice Display")
        );
        assert!(with_display_name.is_active);

        let with_name_only = account_summary_for_keys(
            &keys,
            Some(ProfileMetadata {
                name: Some("alice".to_string()),
                display_name: None,
                picture: None,
                nip05: None,
            }),
            false,
        );
        assert_eq!(with_name_only.display_name.as_deref(), Some("alice"));
        assert!(!with_name_only.is_active);
    }
}
