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

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{FromBech32, PublicKey};

    #[test]
    fn parse_recipient_token_trims_and_accepts_public_key_encodings() {
        let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
        let expected_from_npub = PublicKey::from_bech32(npub).expect("valid npub").to_hex();
        assert_eq!(
            parse_recipient_token(&format!("  {npub}\n")),
            Some(ParsedRecipient::Pubkey(expected_from_npub))
        );

        let hex = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefaf0f1";
        assert_eq!(
            parse_recipient_token(&format!("\t{hex}  ")),
            Some(ParsedRecipient::Pubkey(hex.to_string()))
        );
    }

    #[test]
    fn parse_recipient_token_normalizes_nip05_identifiers_before_validation() {
        assert_eq!(
            parse_recipient_token("  Alice.Example@Example.COM  "),
            Some(ParsedRecipient::Nip05 {
                identifier: "alice.example@example.com".to_string(),
            })
        );
    }

    #[test]
    fn parse_recipient_token_rejects_malformed_tokens() {
        for token in [
            "",
            "   ",
            "notakey",
            "npub1notvalid",
            "abcdef",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            "bob@@example.com",
            "@example.com",
            "bob@",
            "white space@example.com",
            "name!@example.com",
            "alice@ example.com",
            "alice@\texample.com",
            "alice@example.com,",
        ] {
            assert!(
                parse_recipient_token(token).is_none(),
                "expected {token:?} to be rejected"
            );
        }
    }
}
