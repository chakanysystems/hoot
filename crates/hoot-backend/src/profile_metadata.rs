pub use crate::dto::ProfileMetadata;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProfileOption {
    #[default]
    Waiting,
    Some(ProfileMetadata),
}
