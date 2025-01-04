use std::collections::BTreeMap;
use chrono::{DateTime, Utc, serde::ts_seconds_option};
use super::EventId;
use serde::ser::{SerializeMap, Serializer};
use serde::{Serialize, Deserialize};

type GenericTags = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<EventId>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<u32>>,

    #[serde(with = "ts_seconds_option", skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,

    #[serde(with = "ts_seconds_option", skip_serializing_if = "Option::is_none")]
    pub until: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,

    #[serde(flatten, serialize_with = "serialize_generic_tags")]
    pub generic_tags: GenericTags,
}

impl Filter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_ids(mut self, ids: Vec<EventId>) -> Self {
        self.ids = Some(ids);
        self
    }

    pub fn unset_ids(mut self) -> Self {
        self.ids = None;
        self
    }

    pub fn add_id(mut self, id: EventId) -> Self {
        self.ids.get_or_insert_with(Vec::new).push(id);
        self
    }

    pub fn remove_id(mut self, id: &EventId) -> Self {
        if let Some(ids) = &mut self.ids {
            ids.retain(|x| x != id);
        }
        self
    }

    pub fn set_authors(mut self, authors: Vec<String>) -> Self {
        self.authors = Some(authors);
        self
    }

    pub fn unset_authors(mut self) -> Self {
        self.authors = None;
        self
    }

    pub fn add_author(mut self, author: String) -> Self {
        self.authors.get_or_insert_with(Vec::new).push(author);
        self
    }

    pub fn remove_author(mut self, author: &str) -> Self {
        if let Some(authors) = &mut self.authors {
            authors.retain(|x| x != author);
        }
        self
    }

    pub fn set_kinds(mut self, kinds: Vec<u32>) -> Self {
        self.kinds = Some(kinds);
        self
    }

    pub fn unset_kinds(mut self) -> Self {
        self.kinds = None;
        self
    }

    pub fn add_kind(mut self, kind: u32) -> Self {
        self.kinds.get_or_insert_with(Vec::new).push(kind);
        self
    }

    pub fn remove_kind(mut self, kind: u32) -> Self {
        if let Some(kinds) = &mut self.kinds {
            kinds.retain(|&x| x != kind);
        }
        self
    }

    pub fn set_since(mut self, since: DateTime<Utc>) -> Self {
        self.since = Some(since);
        self
    }

    pub fn unset_since(mut self) -> Self {
        self.since = None;
        self
    }

    pub fn set_until(mut self, until: DateTime<Utc>) -> Self {
        self.until = Some(until);
        self
    }

    pub fn unset_until(mut self) -> Self {
        self.until = None;
        self
    }

    pub fn set_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn unset_limit(mut self) -> Self {
        self.limit = None;
        self
    }

    pub fn add_tag(mut self, key: &str, value: Vec<String>) -> Self {
        self.generic_tags.insert(key.to_owned(), value);
        self
    }

    pub fn remove_tag(mut self, key: &str, value: &str) -> Self {
        if let Some(values) = self.generic_tags.get_mut(key) {
            values.retain(|x| x != value);
            if values.is_empty() {
                self.generic_tags.remove(key);
            }
        }
        self
    }

    pub fn clear_tag(mut self, key: &str) -> Self {
        self.generic_tags.remove(key);
        self
    }

    pub fn clear_all_tags(mut self) -> Self {
        self.generic_tags.clear();
        self
    }
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            ids: None,
            authors: None,
            kinds: None,
            since: None,
            until: None,
            limit: None,
            generic_tags: GenericTags::new(),
        }
    }
}

fn serialize_generic_tags<S>(tags: &GenericTags, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(tags.len()))?;
    for (key, value) in tags {
        map.serialize_entry(key, value)?;
    }
    map.end()
}
