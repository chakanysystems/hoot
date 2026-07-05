use crate::profile_metadata::{
    get_profile_metadata, update_logged_in_profile_metadata, ProfileMetadata, ProfileOption,
};
use crate::Hoot;
use hoot_backend::AccountSummary;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, PartialEq)]
pub enum AccountCreationMode {
    Generate,
    Import,
}

pub fn validate_nsec(app: &Hoot, input: &str) -> Result<AccountSummary, String> {
    app.backend
        .validate_nsec(input.to_string())
        .map_err(|e| e.to_string())
}

pub fn account_already_exists(app: &Hoot, pubkey_hex: &str) -> bool {
    app.accounts
        .iter()
        .any(|account| account.pubkey_hex == pubkey_hex)
}

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
            debug!("Metadata requested from backend, will populate when received");
            (String::new(), String::new(), String::new(), false)
        }
    }
}

pub fn save_imported_account(
    app: &mut Hoot,
    nsec: &str,
    display_name: &str,
    name: &str,
    picture_url: &str,
    publish_metadata: bool,
) -> Result<AccountSummary, String> {
    let account = app
        .backend
        .import_account(nsec.to_string())
        .map_err(|e| format!("Failed to save key: {}", e))?;
    finish_saved_account(
        app,
        account,
        display_name,
        name,
        picture_url,
        publish_metadata,
    )
}

pub fn generate_account(
    app: &mut Hoot,
    display_name: &str,
    name: &str,
    picture_url: &str,
    publish_metadata: bool,
) -> Result<AccountSummary, String> {
    let account = app
        .backend
        .generate_account()
        .map_err(|e| format!("Failed to generate key: {}", e))?;
    finish_saved_account(
        app,
        account,
        display_name,
        name,
        picture_url,
        publish_metadata,
    )
}

fn finish_saved_account(
    app: &mut Hoot,
    account: AccountSummary,
    display_name: &str,
    name: &str,
    picture_url: &str,
    publish_metadata: bool,
) -> Result<AccountSummary, String> {
    app.active_account_pubkey = Some(account.pubkey_hex.clone());
    app.refresh_accounts();

    if publish_metadata {
        let metadata = ProfileMetadata {
            display_name: non_empty(display_name),
            name: non_empty(name),
            picture: non_empty(picture_url),
            nip05: None,
        };
        if metadata.display_name.is_some() || metadata.name.is_some() || metadata.picture.is_some()
        {
            match update_logged_in_profile_metadata(app, account.pubkey_hex.clone(), metadata) {
                Ok(_) => info!("Metadata published successfully"),
                Err(e) => warn!("Failed to publish metadata (non-critical): {}", e),
            }
        }
    }

    info!("Account saved successfully");
    Ok(account)
}

pub fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
