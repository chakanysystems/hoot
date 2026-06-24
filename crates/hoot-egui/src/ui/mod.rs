pub mod account_setup;
pub mod add_account_window;
pub mod compose_window;
pub mod contacts;
pub mod drafts_page;
pub mod inbox;
pub mod junk;
pub mod nip05_status;
pub mod onboarding;
pub mod requests;
pub mod search;
pub mod settings;
pub mod thread_view;
pub mod trash;
pub mod unlock_database;

/// Resolve the best display name for a pubkey and the corresponding initials,
/// triggering a metadata fetch if needed.
pub fn resolve_avatar_info(app: &mut crate::Hoot, pubkey: &str) -> (String, String) {
    let _ = crate::profile_metadata::get_profile_metadata(app, pubkey.to_string());
    let name = app
        .resolve_name(pubkey)
        .unwrap_or_else(|| pubkey.to_string());
    let initials = crate::style::initials_for_name(&name);
    (name, initials)
}
