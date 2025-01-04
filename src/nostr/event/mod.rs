use bitcoin::secp256k1::{Message, PublicKey, SecretKey, Keypair, schnorr::Signature, Secp256k1, hashes::{Hash, sha256}};
use serde_json::json;

pub mod tag;
pub use tag::{Tag, list::Tags};
pub mod id;
pub use id::EventId;
pub mod kind;
pub use kind::EventKind; 

#[derive(Debug, PartialEq, Eq)]
pub enum EventBuilderError {
    MissingFields
}

#[derive(Debug, Default)]
pub struct EventBuilder {
    pub created_at: Option<i64>,
    pub kind: Option<EventKind>,
    pub tags: Tags,
    pub content: String,
}

impl EventBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn gift_wrap(sender_keypair: &Keypair, recipient_pubkey: &PublicKey, event: Event) -> Result<Event, EventBuilderError> {
        event.pubkey = Some(sender_keypair.clone().public_key());
        event.id = Some(event.compute_id());
    }

    pub fn kind(mut self, kind: EventKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn created_at(mut self, created_at: i64) -> Self {
        self.created_at = Some(created_at);
        self
    }

    pub fn tag(mut self, tag: Tag) -> Self {
        self.tags.push(tag);

        self
    }

    /// Extends the current tags.
    pub fn tags<I>(mut self, tags: I) -> Self
        where
            I: IntoIterator<Item = Tag>,
    {
        tags.into_iter().map(|t| self.tags.push(t));

        self
    }

    pub fn content(mut self, content: &str) -> Self {
        self.content = content.to_owned();
        self
    }

    pub fn build(&self) -> Result<Event, EventBuilderError> {
        if self.created_at.is_none() || self.kind.is_none() {
            return Err(EventBuilderError::MissingFields);
        }

        Ok(Event {
            created_at: self.created_at.unwrap(),
            kind: self.kind.unwrap().into(),
            tags: self.tags.clone(),
            content: self.content.clone(),
            id: None,
            pubkey: None,
            sig: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub id: Option<EventId>,
    pub pubkey: Option<PublicKey>,
    pub created_at: i64,
    pub kind: u32,
    pub tags: Tags,
    pub content: String,
    pub sig: Option<Signature>,
}

impl Event {
    /// Verifies the signature of the event
    pub fn verify(&self) -> bool {
        let secp = Secp256k1::verification_only();

        let message = Message::from_digest(*self.id.clone().expect("id should be present").as_bytes());
        
        secp.verify_schnorr(&self.sig.expect("signature should be present"), &message, &self.pubkey.expect("public key should be present").into()).is_ok()
    }

    pub fn sign_with_seckey(&mut self, seckey: &SecretKey) -> Result<(), String> {
        let secp = Secp256k1::new();

        let keypair = Keypair::from_secret_key(&secp, seckey);

        self.sign(&keypair)
    }

    /// Signs the event with the given private key
    pub fn sign(&mut self, keypair: &Keypair) -> Result<(), String> {
        let secp = Secp256k1::new();

        if self.pubkey.is_none() {
            self.pubkey = Some(keypair.public_key());
        }

        let id = self.compute_id();
        self.id = Some(id.clone());
        let message = Message::from_digest(*id.as_bytes());
        self.sig = Some(secp.sign_schnorr(&message, keypair));
        Ok(())
    }

    /// Computes the event ID
    fn compute_id(&self) -> EventId {
        let serialized = json!([
            0,
            self.pubkey,
            self.created_at,
            self.kind,
            self.tags,
            self.content
        ]);

        sha256::Hash::hash(serialized.as_str().unwrap().as_bytes()).to_string().into()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::{Secp256k1, SecretKey, PublicKey};

    fn create_test_event() -> Event {
        Event {
            id: Some(EventId::default()),
            pubkey: Some(PublicKey::from_slice(&[0; 33]).unwrap()),
            created_at: 1234567890,
            kind: 1,
            tags: vec![vec!["tag1".to_string(), "value1".to_string()]].into(),
            content: "Test content".to_string(),
            sig: Some(Signature::from_slice(&[0; 64]).unwrap()),
        }
    }

    #[test]
    fn test_compute_id() {
        let event = create_test_event();
        let id = event.compute_id();
        assert_ne!(id, EventId::default());
    }

    #[test]
    fn test_sign_and_verify() {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::new(&mut rand::thread_rng());
        let keypair = Keypair::from_secret_key(&secp, &secret_key);

        let mut event = create_test_event();
        assert!(event.sign(&keypair).is_ok());
        assert!(event.verify());
    }

    #[test]
    fn test_sign_with_seckey() {
        let secret_key = SecretKey::new(&mut rand::thread_rng());
        let mut event = create_test_event();
        assert!(event.sign_with_seckey(&secret_key).is_ok());
        assert!(event.verify());
    }

    #[test]
    fn test_verify_invalid_signature() {
        let mut event = create_test_event();
        event.content = "Modified content".to_string();
        assert!(!event.verify());
    }
}
