//! PKCE (Proof Key for Code Exchange) implementation
//!
//! This module implements RFC 7636 PKCE for OAuth 2.0 authorization code flow.
//! PKCE provides additional security for public clients by using a code verifier
//! and code challenge.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};

/// PKCE challenge pair containing the verifier and challenge
#[derive(Debug, Clone)]
pub struct PkceChallenge {
    /// The verifier string (stored client-side, sent during token exchange)
    pub verifier: String,
    /// The challenge string (sent during authorization request)
    pub challenge: String,
}

impl PkceChallenge {
    /// Generate a new PKCE challenge pair
    ///
    /// Creates a cryptographically random 32-byte verifier and computes
    /// the SHA256 hash as the challenge (S256 method).
    pub fn generate() -> Self {
        // Generate 32 random bytes for the verifier
        let random_bytes: [u8; 32] = rand::random();
        let verifier = URL_SAFE_NO_PAD.encode(random_bytes);

        // Compute SHA256 hash of verifier for the challenge
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        Self {
            verifier,
            challenge,
        }
    }

    /// Get the code challenge method (always S256)
    pub fn challenge_method() -> &'static str {
        "S256"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let pkce = PkceChallenge::generate();

        // Verifier should be base64url encoded 32 bytes = 43 characters
        assert_eq!(pkce.verifier.len(), 43);

        // Challenge should be base64url encoded SHA256 = 43 characters
        assert_eq!(pkce.challenge.len(), 43);

        // Verifier and challenge should be different
        assert_ne!(pkce.verifier, pkce.challenge);
    }

    #[test]
    fn test_pkce_uniqueness() {
        let pkce1 = PkceChallenge::generate();
        let pkce2 = PkceChallenge::generate();

        // Each generation should produce unique values
        assert_ne!(pkce1.verifier, pkce2.verifier);
        assert_ne!(pkce1.challenge, pkce2.challenge);
    }

    #[test]
    fn test_pkce_verifier_challenge_relationship() {
        let pkce = PkceChallenge::generate();

        // Verify that the challenge is the SHA256 hash of the verifier
        let mut hasher = Sha256::new();
        hasher.update(pkce.verifier.as_bytes());
        let expected_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        assert_eq!(pkce.challenge, expected_challenge);
    }

    #[test]
    fn test_challenge_method() {
        assert_eq!(PkceChallenge::challenge_method(), "S256");
    }

    #[test]
    fn test_pkce_base64url_format() {
        let pkce = PkceChallenge::generate();

        // Verify that the verifier and challenge use URL-safe base64 characters
        let valid_chars = |s: &str| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };

        assert!(valid_chars(&pkce.verifier));
        assert!(valid_chars(&pkce.challenge));
    }
}
