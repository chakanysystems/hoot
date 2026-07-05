pub use crate::dto::ProfileMetadata;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileOption {
    Waiting,
    Some(ProfileMetadata),
}

impl Default for ProfileOption {
    fn default() -> Self {
        Self::Waiting
    }
}
