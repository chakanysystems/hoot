use nostr::types::Filter;
use rand::{distributions::Alphanumeric, Rng};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Subscription {
    pub id: String,
    pub filters: Vec<Filter>,
}

impl Default for Subscription {
    fn default() -> Self {
        let s: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
        Self::new(s, vec![])
    }
}

impl Subscription {
    pub fn new(id: String, filters: Vec<Filter>) -> Self {
        Self { id, filters }
    }

    pub fn filter(&mut self, filter: Filter) -> &mut Self {
        self.filters.push(filter);

        self
    }
}
