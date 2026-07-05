mod account_manager;
mod account_ops;
mod backend;
mod conversions;
mod db;
mod dto;
mod error;
mod event_processing;
mod mail_event;
mod nip05;
mod profile_metadata;
mod recipient;
mod relay;

pub use backend::HootBackend;
pub use dto::*;
pub use error::{HootError, HootResult};
pub use profile_metadata::ProfileOption;
pub use recipient::{normalize_nip05_identifier, parse_recipient_token, ParsedRecipient};
pub use conversions::npub_string;
pub use relay::default_relay_urls;

#[cfg(debug_assertions)]
pub const STORAGE_NAME: &str = "systems.chakany.hoot-dev";
#[cfg(not(debug_assertions))]
pub const STORAGE_NAME: &str = "systems.chakany.hoot";

uniffi::setup_scaffolding!();
