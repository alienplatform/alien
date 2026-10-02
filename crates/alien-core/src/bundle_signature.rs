//! Signatures on air-gapped bundles.
//!
//! A manager signs each bundle's manifest, which lists the SHA-256 of every
//! other file in the bundle, with an Ed25519 key it keeps in its state
//! directory. The environment verifies the signature against the manager's
//! public key before applying anything, so a bundle altered on its way in
//! is rejected.
//!
//! Keys and signatures travel as text: `ed25519:<base64>`.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_compact::{KeyPair, PublicKey, Seed, Signature};

use crate::error::{ErrorData, Result};
use alien_error::AlienError;

const PREFIX: &str = "ed25519:";

/// A manager's bundle signing key.
pub struct BundleSigningKey(KeyPair);

impl std::fmt::Debug for BundleSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BundleSigningKey")
            .field(&self.public_key())
            .finish()
    }
}

impl BundleSigningKey {
    /// The key for a 32-byte seed.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(KeyPair::from_seed(Seed::new(seed)))
    }

    /// The public key, as the environment is given it.
    pub fn public_key(&self) -> String {
        format!("{PREFIX}{}", STANDARD.encode(self.0.pk.as_ref()))
    }

    /// Sign `message` (a bundle manifest).
    pub fn sign(&self, message: &[u8]) -> String {
        format!("{PREFIX}{}", STANDARD.encode(self.0.sk.sign(message, None)))
    }
}

/// Check that `signature` over `message` was made with `public_key`.
pub fn verify(public_key: &str, message: &[u8], signature: &str) -> Result<()> {
    let key = decode(public_key, "public key")?;
    let key = PublicKey::from_slice(&key).map_err(|_| invalid("the public key is malformed"))?;
    let signature = decode(signature, "signature")?;
    let signature =
        Signature::from_slice(&signature).map_err(|_| invalid("the signature is malformed"))?;
    key.verify(message, &signature).map_err(|_| {
        invalid("the bundle was not signed by the trusted key, or was changed after signing")
    })
}

fn decode(value: &str, what: &str) -> Result<Vec<u8>> {
    let encoded = value
        .trim()
        .strip_prefix(PREFIX)
        .ok_or_else(|| invalid(&format!("the {what} must start with '{PREFIX}'")))?;
    STANDARD
        .decode(encoded)
        .map_err(|_| invalid(&format!("the {what} is not valid base64")))
}

fn invalid(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::BundleSignatureInvalid {
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_manifest_verifies() {
        let key = BundleSigningKey::from_seed([3; 32]);
        let signature = key.sign(b"manifest");
        verify(&key.public_key(), b"manifest", &signature).expect("signature must verify");
    }

    #[test]
    fn a_changed_manifest_is_rejected() {
        let key = BundleSigningKey::from_seed([3; 32]);
        let signature = key.sign(b"manifest");
        let error = verify(&key.public_key(), b"manifest!", &signature)
            .expect_err("a changed manifest must not verify");
        assert_eq!(error.code, "BUNDLE_SIGNATURE_INVALID");
    }

    #[test]
    fn another_key_is_rejected() {
        let signature = BundleSigningKey::from_seed([3; 32]).sign(b"manifest");
        let other = BundleSigningKey::from_seed([4; 32]).public_key();
        verify(&other, b"manifest", &signature).expect_err("another key must not verify");
    }

    #[test]
    fn malformed_input_is_rejected_without_panicking() {
        let key = BundleSigningKey::from_seed([3; 32]);
        let signature = key.sign(b"manifest");
        for (public_key, signature) in [
            ("ed25519:not-base64!", signature.as_str()),
            ("rsa:AAAA", signature.as_str()),
            ("ed25519:AAAA", signature.as_str()),
            (key.public_key().as_str(), "ed25519:AAAA"),
        ] {
            verify(public_key, b"manifest", signature).expect_err("malformed input must fail");
        }
    }
}
