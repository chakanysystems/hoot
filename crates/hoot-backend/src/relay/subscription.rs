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

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Kind;

    #[test]
    fn default_subscription_id_is_seven_alphanumeric_characters() {
        let subscription = Subscription::default();

        assert_eq!(subscription.id.len(), 7);
        assert!(subscription.id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(subscription.filters.is_empty());
    }

    #[test]
    fn filter_appends_filters_in_order_and_supports_chaining() {
        let first = Filter::new().kind(Kind::TextNote);
        let second = Filter::new().kind(Kind::Metadata);
        let mut subscription = Subscription::new("mailbox".to_string(), vec![]);

        subscription.filter(first.clone()).filter(second.clone());

        assert_eq!(subscription.filters, vec![first, second]);
    }
}
