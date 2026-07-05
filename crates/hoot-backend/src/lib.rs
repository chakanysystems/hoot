pub mod account_manager;
pub mod backend;
pub mod db;
pub mod dto;
pub mod error;
pub mod mail_event;
pub mod nip05;
pub mod profile_metadata;
pub mod relay;
pub mod threaded_event;

pub use backend::HootBackend;
pub use dto::*;
pub use error::{HootError, HootResult};

#[cfg(debug_assertions)]
pub const STORAGE_NAME: &str = "systems.chakany.hoot-dev";
#[cfg(not(debug_assertions))]
pub const STORAGE_NAME: &str = "systems.chakany.hoot";

uniffi::setup_scaffolding!();
