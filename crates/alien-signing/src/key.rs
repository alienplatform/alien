//! ECDSA P-256 keys in the text forms statements and signer lists carry.
//!
//! A public key travels as `ecdsa-p256:<base64 DER SubjectPublicKeyInfo>` and a
//! signature as base64 DER ECDSA over the SHA-256 of the signed bytes. Those
//! are the forms cloud key services, Secure Enclave, TPMs and PIV tokens
//! produce, so a key can live in any of them.

use std::fmt;

use alien_error::AlienError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use p256::ecdsa::signature::{Signer as _, Verifier as _};
use p256::ecdsa::{DerSignature, SigningKey, VerifyingKey};
use p256::pkcs8::{
    DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding,
};
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};

/// Prefix of a public key in text form.
pub const PUBLIC_KEY_PREFIX: &str = "ecdsa-p256:";

/// A P-256 public key that parsed, so it is a valid point on the curve.
#[derive(Clone, PartialEq, Eq)]
pub struct PublicKey {
    key: VerifyingKey,
    text: String,
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PublicKey {
    /// Parse `ecdsa-p256:<base64 DER SPKI>`. The point decoder rejects points
    /// that are not on the curve and the identity, so an invalid-curve or
    /// low-order key can't enter a signer list.
    pub fn parse(text: &str) -> Result<Self> {
        let encoded = text.strip_prefix(PUBLIC_KEY_PREFIX).ok_or_else(|| {
            key_invalid(format!("the key does not start with '{PUBLIC_KEY_PREFIX}'"))
        })?;
        let der = STANDARD
            .decode(encoded)
            .map_err(|_| key_invalid("the key is not valid base64"))?;
        let key = VerifyingKey::from_public_key_der(&der)
            .map_err(|_| key_invalid("the key is not a DER P-256 SubjectPublicKeyInfo"))?;
        Ok(Self::from_verifying_key(key))
    }

    fn from_verifying_key(key: VerifyingKey) -> Self {
        let der = key
            .to_public_key_der()
            .expect("a valid P-256 public key always encodes");
        let text = format!("{PUBLIC_KEY_PREFIX}{}", STANDARD.encode(der.as_bytes()));
        Self { key, text }
    }

    /// The key in text form. Parsing re-encodes, so two spellings of one key
    /// give the same text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// `SHA256:<hex>` of the DER SubjectPublicKeyInfo, for display.
    pub fn fingerprint(&self) -> String {
        let der = self
            .key
            .to_public_key_der()
            .expect("a valid P-256 public key always encodes");
        format!("SHA256:{}", hex::encode(Sha256::digest(der.as_bytes())))
    }

    /// Whether `signature` (base64 DER) is this key's signature over `message`.
    pub fn verify(&self, message: &[u8], signature: &str) -> Result<()> {
        let der = STANDARD
            .decode(signature)
            .map_err(|_| signature_invalid("the signature is not valid base64"))?;
        let signature = DerSignature::try_from(der.as_slice())
            .map_err(|_| signature_invalid("the signature is not DER ECDSA"))?;
        self.key
            .verify(message, &signature)
            .map_err(|_| signature_invalid("the signature does not match the signed bytes"))
    }
}

/// A private key held in memory: the CI key, the recovery key, or the
/// `software` fallback for machines without secure hardware.
pub struct SoftwareKey(SigningKey);

impl fmt::Debug for SoftwareKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SoftwareKey")
            .field(&self.public_key())
            .finish()
    }
}

impl SoftwareKey {
    /// A new random key from the operating system's generator.
    pub fn generate() -> Result<Self> {
        // Random bytes are a valid scalar except with negligible probability.
        loop {
            let mut secret = [0u8; 32];
            getrandom::fill(&mut secret).map_err(|error| {
                key_invalid(format!("the system random source failed: {error}"))
            })?;
            if let Ok(key) = Self::from_secret_bytes(&secret) {
                return Ok(key);
            }
        }
    }

    /// The key for 32 secret bytes, which must be a valid P-256 scalar.
    pub fn from_secret_bytes(bytes: &[u8; 32]) -> Result<Self> {
        SigningKey::from_slice(bytes)
            .map(Self)
            .map_err(|_| key_invalid("the secret bytes are not a valid P-256 scalar"))
    }

    /// Read a PKCS#8 PEM private key.
    pub fn from_pkcs8_pem(pem: &str) -> Result<Self> {
        SigningKey::from_pkcs8_pem(pem.trim())
            .map(Self)
            .map_err(|_| key_invalid("the text is not a PKCS#8 PEM ECDSA P-256 private key"))
    }

    /// The key as PKCS#8 PEM.
    pub fn to_pkcs8_pem(&self) -> Result<String> {
        self.0
            .to_pkcs8_pem(LineEnding::LF)
            .map(|pem| pem.as_str().to_string())
            .map_err(|_| key_invalid("the key could not be encoded"))
    }

    /// The public half.
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_verifying_key(*self.0.verifying_key())
    }

    /// Base64 DER signature over `message`.
    pub fn sign(&self, message: &[u8]) -> String {
        let signature: DerSignature = self.0.sign(message);
        STANDARD.encode(signature.as_bytes())
    }
}

fn key_invalid(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::KeyInvalid {
        reason: reason.into(),
    })
}

fn signature_invalid(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SignatureRefused {
        signer: String::new(),
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_verifies_only_for_its_key_and_message() {
        let key = SoftwareKey::generate().unwrap();
        let other = SoftwareKey::generate().unwrap();
        let signature = key.sign(b"hello");
        key.public_key().verify(b"hello", &signature).unwrap();
        key.public_key()
            .verify(b"hellp", &signature)
            .expect_err("changed message");
        other
            .public_key()
            .verify(b"hello", &signature)
            .expect_err("other key");
    }

    #[test]
    fn public_keys_round_trip_through_text_and_pem() {
        let key = SoftwareKey::generate().unwrap();
        let text = key.public_key().as_str().to_string();
        assert_eq!(PublicKey::parse(&text).unwrap(), key.public_key());
        let restored = SoftwareKey::from_pkcs8_pem(&key.to_pkcs8_pem().unwrap()).unwrap();
        assert_eq!(restored.public_key(), key.public_key());
    }

    #[test]
    fn points_off_the_curve_are_refused() {
        let key = SoftwareKey::generate().unwrap();
        let der = STANDARD
            .decode(
                key.public_key()
                    .as_str()
                    .strip_prefix(PUBLIC_KEY_PREFIX)
                    .unwrap(),
            )
            .unwrap();
        // The uncompressed point is the last 64 bytes; change one byte of y.
        let mut off_curve = der.clone();
        let last = off_curve.len() - 1;
        off_curve[last] ^= 0x01;
        let text = format!("{PUBLIC_KEY_PREFIX}{}", STANDARD.encode(&off_curve));
        let error = PublicKey::parse(&text).expect_err("off-curve point");
        assert_eq!(error.code, "TRUST_KEY_INVALID");

        PublicKey::parse("ed25519:AAAA").expect_err("other algorithm");
    }
}
