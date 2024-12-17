use serde::{Serialize, Deserialize};

pub mod list;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag(Vec<String>);

impl Tag {
    #[inline]
    pub fn new() -> Self {
        Tag::new_with_values(Vec::new())
    }

    #[inline]
    pub fn new_with_values(values: Vec<String>) -> Self {
        Self(values)
    }

    #[inline]
    pub fn kind(&self) -> &str {
        &self.0[0]
    }

    #[inline]
    pub fn content(&self) -> Option<&str> {
        self.0.get(1).map(|s| s.as_str())
    }

    #[inline]
    pub fn len(self) -> usize {
        self.0.len()
    }
}

impl From<Vec<String>> for Tag {
    fn from(value: Vec<String>) -> Self {
        Tag::new_with_values(value)
    }
}
