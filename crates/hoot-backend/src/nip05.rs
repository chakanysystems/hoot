use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use tracing::{debug, error, warn};

/// Verification status for a NIP-05 identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nip05VerificationStatus {
    /// Successfully verified - the pubkey matches
    Verified,
    /// Failed verification - pubkey mismatch or other error
    Failed,
}

impl Nip05VerificationStatus {
    /// Returns true if this status represents a successful verification
    pub fn is_verified(&self) -> bool {
        matches!(self, Self::Verified)
    }
}

/// Message sent back from a background verification thread
struct VerificationResult {
    nip05: String,
    pubkey_hex: String,
    status: Nip05VerificationStatus,
}

/// Handles NIP-05 verification on background threads to avoid blocking the UI.
pub struct Nip05Verifier {
    sender: Sender<VerificationResult>,
    receiver: Receiver<VerificationResult>,
    pending: HashSet<String>,
}

impl Nip05Verifier {
    pub fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            sender,
            receiver,
            pending: HashSet::new(),
        }
    }

    /// Enqueue a NIP-05 identifier for background verification.
    /// Deduplicates requests — if this (pubkey, nip05) pair is already in flight, does nothing.
    pub fn request(&mut self, nip05: String, pubkey_hex: String) {
        let key = format!("{}:{}", pubkey_hex, nip05);
        if self.pending.contains(&key) {
            return;
        }

        let sender = self.sender.clone();
        let nip05_clone = nip05.clone();
        let pubkey_clone = pubkey_hex.clone();

        self.pending.insert(key);

        thread::spawn(move || {
            let status = verify_nip05_for_pubkey(&nip05_clone, &pubkey_clone);
            if sender
                .send(VerificationResult {
                    nip05: nip05_clone,
                    pubkey_hex: pubkey_clone,
                    status,
                })
                .is_err()
            {
                debug!("NIP-05 verification receiver dropped");
            }
        });
    }

    /// Drain completed verifications and write results to the database.
    /// Call this every frame from the main update loop.
    pub fn process_queue(&mut self, db: &crate::db::Db) -> bool {
        let mut updated = false;
        while let Ok(result) = self.receiver.try_recv() {
            let key = format!("{}:{}", result.pubkey_hex, result.nip05);
            self.pending.remove(&key);

            let verified = result.status.is_verified();
            if let Err(e) =
                db.update_nip05_verification_status(&result.pubkey_hex, &result.nip05, verified)
            {
                warn!("Failed to update NIP-05 verification status: {}", e);
            } else {
                updated = true;
                debug!(
                    "Verified NIP-05 {} for {}: {:?}",
                    result.nip05, result.pubkey_hex, result.status
                );
            }
        }
        updated
    }
}

/// Result of a NIP-05 resolution (nip05 → pubkey lookup)
struct ResolutionResult {
    nip05: String,
    pubkey_hex: Option<String>,
}

/// The state of a NIP-05 resolution request
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nip05Resolution {
    /// Resolution is in progress
    Pending,
    /// Resolved successfully to this pubkey hex
    Resolved(String),
    /// Resolution failed (network error, not found, etc.)
    Failed,
}

/// Resolves NIP-05 identifiers to pubkeys on background threads.
/// Used by the compose window to avoid blocking the UI when sending to NIP-05 addresses.
pub struct Nip05Resolver {
    results: HashMap<String, Nip05Resolution>,
    pending: HashSet<String>,
    sender: Sender<ResolutionResult>,
    receiver: Receiver<ResolutionResult>,
}

impl Nip05Resolver {
    pub fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            results: HashMap::new(),
            pending: HashSet::new(),
            sender,
            receiver,
        }
    }

    /// Look up a NIP-05 identifier. Returns the cached resolution state, or None if not yet
    /// requested. Call `request()` to enqueue a resolution.
    pub fn get(&self, nip05: &str) -> Option<&Nip05Resolution> {
        self.results.get(nip05)
    }

    /// Enqueue a NIP-05 identifier for background resolution.
    /// Does nothing if this identifier is already pending or resolved.
    pub fn request(&mut self, nip05: String) {
        if self.results.contains_key(&nip05) || self.pending.contains(&nip05) {
            return;
        }

        let sender = self.sender.clone();
        let nip05_clone = nip05.clone();

        self.pending.insert(nip05.clone());
        self.results.insert(nip05, Nip05Resolution::Pending);

        thread::spawn(move || {
            let pubkey_hex = if let Some((local, domain)) = parse_nip05(&nip05_clone) {
                match fetch_nip05_verification(&local, &domain) {
                    Ok(hex) => hex,
                    Err(e) => {
                        debug!("NIP-05 resolution failed for {}: {}", nip05_clone, e);
                        None
                    }
                }
            } else {
                None
            };
            let _ = sender.send(ResolutionResult {
                nip05: nip05_clone,
                pubkey_hex,
            });
        });
    }

    /// Drain completed resolutions. Call this every frame from the main update loop.
    pub fn process_queue(&mut self) -> bool {
        let mut updated = false;
        while let Ok(result) = self.receiver.try_recv() {
            self.pending.remove(&result.nip05);
            let resolution = match result.pubkey_hex {
                Some(hex) => Nip05Resolution::Resolved(hex),
                None => Nip05Resolution::Failed,
            };
            self.results.insert(result.nip05, resolution);
            updated = true;
        }
        updated
    }
}

/// Response structure from the NIP-05 verification endpoint
#[derive(Debug, Deserialize)]
struct Nip05Response {
    names: std::collections::HashMap<String, String>,
}

/// Parse a NIP-05 identifier into its components
/// Returns Some((local_part, domain)) if valid, None otherwise
/// Valid format: local@domain where local uses only a-z, 0-9, -, _, .
pub fn parse_nip05(identifier: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = identifier.split('@').collect();
    if parts.len() != 2 {
        return None;
    }

    let local = parts[0];
    let domain = parts[1];

    // Local part cannot be empty
    if local.is_empty() {
        return None;
    }

    // Validate local part - must only contain a-z, 0-9, -, _, .
    if !local
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.')
    {
        return None;
    }

    // Domain cannot be empty
    if domain.is_empty() {
        return None;
    }

    Some((local.to_string(), domain.to_string()))
}

/// Fetch the NIP-05 verification data from the domain's well-known endpoint
/// Returns the hex pubkey associated with the local name, or None if not found
pub fn fetch_nip05_verification(local: &str, domain: &str) -> Result<Option<String>> {
    let url = format!("https://{}/.well-known/nostr.json?name={}", domain, local);
    debug!("Fetching NIP-05 verification from: {}", url);

    let client = reqwest::blocking::Client::builder()
        // Do not follow redirects per NIP-05 spec
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to build HTTP client")?;

    let response = client
        .get(&url)
        .send()
        .context("Failed to fetch NIP-05 verification")?;

    // Check for redirect (we must not follow redirects per NIP-05 spec)
    if response.status().is_redirection() {
        warn!("NIP-05 endpoint returned redirect - ignoring per spec");
        return Ok(None);
    }

    if !response.status().is_success() {
        warn!("NIP-05 endpoint returned status: {}", response.status());
        return Ok(None);
    }

    let nip05_response: Nip05Response = response
        .json()
        .context("Failed to parse NIP-05 response JSON")?;

    // Look up the local name in the names map
    // Per NIP-05, keys must be in lowercase hex format
    let pubkey_hex = nip05_response.names.get(local).cloned();

    if let Some(hex_str) = &pubkey_hex {
        // Validate that it's a valid hex string (64 characters for a 32-byte pubkey)
        // Per NIP-05 spec, keys must be in lowercase hex format
        if hex_str.len() != 64 || !hex_str.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
            warn!("NIP-05 response contained invalid hex pubkey: {}", hex_str);
            return Ok(None);
        }
    }

    Ok(pubkey_hex)
}

/// Verify that a NIP-05 identifier maps to the expected pubkey
/// Returns the verification status
pub fn verify_nip05_for_pubkey(nip05: &str, expected_pubkey_hex: &str) -> Nip05VerificationStatus {
    let Some((local, domain)) = parse_nip05(nip05) else {
        return Nip05VerificationStatus::Failed;
    };

    // Normalize expected pubkey to lowercase hex
    let expected_pubkey_lower = expected_pubkey_hex.to_lowercase();

    match fetch_nip05_verification(&local, &domain) {
        Ok(Some(fetched_pubkey)) => {
            let fetched_pubkey_lower = fetched_pubkey.to_lowercase();
            if fetched_pubkey_lower == expected_pubkey_lower {
                Nip05VerificationStatus::Verified
            } else {
                warn!(
                    "NIP-05 verification failed: {} claimed by {} but maps to {}",
                    nip05, expected_pubkey_lower, fetched_pubkey_lower
                );
                Nip05VerificationStatus::Failed
            }
        }
        Ok(None) => {
            warn!(
                "NIP-05 verification failed: {} not found on {}",
                local, domain
            );
            Nip05VerificationStatus::Failed
        }
        Err(e) => {
            error!("NIP-05 verification error for {}: {}", nip05, e);
            Nip05VerificationStatus::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nip05_valid() {
        assert_eq!(
            parse_nip05("bob@example.com"),
            Some(("bob".to_string(), "example.com".to_string()))
        );
        assert_eq!(
            parse_nip05("user_123@test.org"),
            Some(("user_123".to_string(), "test.org".to_string()))
        );
        assert_eq!(
            parse_nip05("a.b-c@domain.io"),
            Some(("a.b-c".to_string(), "domain.io".to_string()))
        );
    }

    #[test]
    fn test_parse_nip05_invalid() {
        assert_eq!(parse_nip05("bobaexample.com"), None); // No @
        assert_eq!(parse_nip05("@example.com"), None); // Empty local
        assert_eq!(parse_nip05("bob@"), None); // Empty domain
        assert_eq!(parse_nip05("bob@@example.com"), None); // Double @
        assert_eq!(parse_nip05("Bob@example.com"), None); // Uppercase in local
        assert_eq!(
            parse_nip05("user@domain"),
            Some(("user".to_string(), "domain".to_string()))
        ); // No TLD is valid
    }

    #[test]
    fn recipient_parser_accepts_pubkey_encodings_and_normalizes_nip05() {
        use nostr::{FromBech32, PublicKey};

        let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
        let expected_from_npub = PublicKey::from_bech32(npub).unwrap().to_hex();
        match crate::parse_recipient_token(npub) {
            Some(crate::ParsedRecipient::Pubkey(hex)) => assert_eq!(hex, expected_from_npub),
            other => panic!("expected npub to parse as pubkey, got {other:?}"),
        }

        let hex = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefaf0f1";
        match crate::parse_recipient_token(hex) {
            Some(crate::ParsedRecipient::Pubkey(parsed_hex)) => assert_eq!(parsed_hex, hex),
            other => panic!("expected hex pubkey to parse as pubkey, got {other:?}"),
        }

        match crate::parse_recipient_token("Bob@Example.COM") {
            Some(crate::ParsedRecipient::Nip05 { identifier }) => {
                assert_eq!(identifier, "bob@example.com");
            }
            other => panic!("expected NIP-05 to normalize to lowercase, got {other:?}"),
        }
    }

    #[test]
    fn recipient_parser_rejects_invalid_tokens() {
        for token in ["", "   ", "notakey", "bob@@example.com", "@example.com"] {
            assert!(
                crate::parse_recipient_token(token).is_none(),
                "expected {token:?} to be rejected"
            );
        }
    }
}
