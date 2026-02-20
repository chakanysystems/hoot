use crate::profile_metadata::{
    get_profile_metadata, update_logged_in_profile_metadata, ProfileMetadata, ProfileOption,
};
use crate::Hoot;
use nostr::Keys;
use tracing::{debug, info, warn};

/// Shared between onboarding and add-account flows.
#[derive(Debug, Clone, PartialEq)]
pub enum AccountCreationMode {
    Generate,
    Import,
}

/// Validate an nsec string and return the parsed Keys.
pub fn validate_nsec(input: &str) -> Result<Keys, String> {
    crate::account_manager::validate_nsec(input)
}

/// Check if a key is already loaded in the account manager.
pub fn key_already_exists(app: &Hoot, keys: &Keys) -> bool {
    app.account_manager
        .loaded_keys
        .iter()
        .any(|k| k.public_key() == keys.public_key())
}

/// Fetch existing metadata from relays/db and return the pre-filled values.
/// Returns (display_name, name, picture_url, was_fetched).
pub fn fetch_and_prefill_metadata(app: &mut Hoot, pubkey: &str) -> (String, String, String, bool) {
    match get_profile_metadata(app, pubkey.to_string()).clone() {
        ProfileOption::Some(meta) => {
            debug!("Pre-filled metadata for imported key");
            (
                meta.display_name.clone().unwrap_or_default(),
                meta.name.clone().unwrap_or_default(),
                meta.picture.clone().unwrap_or_default(),
                true,
            )
        }
        ProfileOption::Waiting => {
            debug!("Metadata requested from relays, will populate when received");
            (String::new(), String::new(), String::new(), false)
        }
    }
}

/// Save an account key, optionally publish metadata, and update subscriptions.
/// Returns Ok(()) on success, Err(message) on failure.
pub fn save_account(
    app: &mut Hoot,
    key: &Keys,
    display_name: &str,
    name: &str,
    picture_url: &str,
    publish_metadata: bool,
) -> Result<(), String> {
    app.account_manager
        .save_keys(&app.db, key)
        .map_err(|e| format!("Failed to save key: {}", e))?;

    app.active_account = Some(key.clone());

    if publish_metadata {
        let metadata = ProfileMetadata {
            display_name: non_empty(display_name),
            name: non_empty(name),
            picture: non_empty(picture_url),
            nip05: None,
        };
        if metadata.display_name.is_some() || metadata.name.is_some() || metadata.picture.is_some()
        {
            match update_logged_in_profile_metadata(app, key.public_key(), metadata) {
                Ok(_) => info!("Metadata published successfully"),
                Err(e) => warn!("Failed to publish metadata (non-critical): {}", e),
            }
        }
    }

    app.update_gift_wrap_subscription();
    info!("Account saved successfully");
    Ok(())
}

pub fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
