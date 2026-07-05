mod account_manager;
mod backend;
mod db;
mod dto;
mod error;
mod mail_event;
mod nip05;
mod profile_metadata;
mod recipient;
mod relay;
mod threaded_event;

pub use backend::HootBackend;
pub use dto::*;
pub use error::{HootError, HootResult};
pub use profile_metadata::ProfileOption;
pub use recipient::{normalize_nip05_identifier, parse_recipient_token, ParsedRecipient};
pub use relay::default_relay_urls;

#[cfg(debug_assertions)]
pub const STORAGE_NAME: &str = "systems.chakany.hoot-dev";
#[cfg(not(debug_assertions))]
pub const STORAGE_NAME: &str = "systems.chakany.hoot";

uniffi::setup_scaffolding!();
