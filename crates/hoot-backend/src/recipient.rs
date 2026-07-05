use nostr::{FromBech32, PublicKey};

use crate::nip05::parse_nip05;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedRecipient {
    Pubkey(String),
    Nip05 { identifier: String },
}

pub fn parse_recipient_token(token: &str) -> Option<ParsedRecipient> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    if token.contains('@') {
        return normalize_nip05_identifier(token)
            .map(|identifier| ParsedRecipient::Nip05 { identifier });
    }

    if let Ok(public_key) = PublicKey::from_bech32(token) {
        return Some(ParsedRecipient::Pubkey(public_key.to_hex()));
    }

    if let Ok(public_key) = PublicKey::from_hex(token) {
        return Some(ParsedRecipient::Pubkey(public_key.to_hex()));
    }

    None
}

pub fn normalize_nip05_identifier(identifier: &str) -> Option<String> {
    let normalized = identifier.trim().to_lowercase();
    parse_nip05(&normalized)?;
    Some(normalized)
}
