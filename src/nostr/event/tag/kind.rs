use serde::{Serialize, Deserialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagKind {
    #[serde(rename = "t")]
    Tag,
    #[serde(rename = "p")]
    Pubkey,
    #[serde(rename = "subject")]
    Subject,
}
