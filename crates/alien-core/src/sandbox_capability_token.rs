//! Wire format for a sandbox capability: the bytes that carry [`SandboxCapabilityClaims`]
//! from the manager that mints one to the agent that checks it.
//!
//! Minting and verification live in the same module because a difference between them is a
//! security bug that presents as "it works" right up until it does not.
//!
//! ```text
//! base64url(claims_json) "." base64url(signature)
//! ```
//!
//! The signature covers the **encoded** claims segment, and verification runs against the
//! exact bytes that arrived. That removes canonicalisation from the trust path entirely: there
//! is no re-serialization step whose output could differ from what was signed.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_compact::{PublicKey, SecretKey, Signature};

use crate::error::{ErrorData, Result};
use crate::sandbox_capability::{
    SandboxCapabilityClaims, SandboxOperationClass, SandboxSessionIdentity,
};
use alien_error::{AlienError, Context, IntoAlienError};

/// Signs claims into a capability token.
///
/// The manager holds the secret key; it never enters deployment state and never reaches a
/// sandbox — see `crates/alien-infra/AGENTS.md` on raw secrets in resource configs.
pub fn mint(claims: &SandboxCapabilityClaims, secret_key: &SecretKey) -> Result<String> {
    let json = serde_json::to_vec(claims).into_alien_error().context(
        ErrorData::JsonSerializationFailed {
            reason: "Failed to serialize sandbox capability claims".to_string(),
        },
    )?;

    let payload = URL_SAFE_NO_PAD.encode(json);
    let signature = secret_key.sign(payload.as_bytes(), None);

    Ok(format!(
        "{payload}.{}",
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}

/// Verifies a token's signature and its claims, returning them only if both hold.
///
/// Order matters: the signature is checked before the claims are parsed, so malformed JSON
/// from an unsigned source is never handed to the deserializer.
pub fn verify(
    token: &str,
    public_key: &PublicKey,
    identity: &SandboxSessionIdentity,
    required: SandboxOperationClass,
    now_unix: i64,
) -> Result<SandboxCapabilityClaims> {
    let (payload, signature) = token
        .split_once('.')
        .ok_or_else(|| refused("token is malformed"))?;

    // A second separator means a third segment nobody agreed on. Refusing beats ignoring it.
    if signature.contains('.') {
        return Err(refused("token is malformed"));
    }

    let signature_bytes = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| refused("token is malformed"))?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| refused("token is malformed"))?;

    public_key
        .verify(payload.as_bytes(), &signature)
        .map_err(|_| refused("token signature is not valid"))?;

    let json = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| refused("token is malformed"))?;
    let claims: SandboxCapabilityClaims =
        serde_json::from_slice(&json).map_err(|_| refused("token is malformed"))?;

    claims.verify(identity, required, now_unix)?;

    Ok(claims)
}

fn refused(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxCapabilityRefused {
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::KeyPair;

    const NOW: i64 = 1_000_000;

    fn identity() -> SandboxSessionIdentity {
        SandboxSessionIdentity {
            session_id: "s1".to_string(),
            generation: 2,
        }
    }

    fn claims() -> SandboxCapabilityClaims {
        SandboxCapabilityClaims {
            session_id: "s1".to_string(),
            operation: SandboxOperationClass::Execute,
            generation: 2,
            expires_at: NOW + 300,
            key_id: "k1".to_string(),
        }
    }

    fn keypair() -> KeyPair {
        KeyPair::from_seed(ed25519_compact::Seed::new([7u8; 32]))
    }

    #[test]
    fn a_minted_token_verifies_and_returns_its_claims() {
        let keys = keypair();
        let token = mint(&claims(), &keys.sk).expect("mints");

        let verified = verify(
            &token,
            &keys.pk,
            &identity(),
            SandboxOperationClass::Execute,
            NOW,
        )
        .expect("a token this key signed, for this session, is valid");

        assert_eq!(verified, claims());
    }

    /// The claims travel in the clear, so the signature is the only thing stopping a caller
    /// from writing its own. This is the test that proves it is load-bearing.
    #[test]
    fn claims_edited_after_minting_are_refused() {
        let keys = keypair();
        let token = mint(&claims(), &keys.sk).expect("mints");

        let mut escalated = claims();
        escalated.operation = SandboxOperationClass::Manage;
        let forged_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&escalated).unwrap());
        let signature = token.split_once('.').unwrap().1;

        let error = verify(
            &format!("{forged_payload}.{signature}"),
            &keys.pk,
            &identity(),
            SandboxOperationClass::Manage,
            NOW,
        )
        .expect_err("swapping the claims must invalidate the signature");
        assert!(error.to_string().contains("signature"));
    }

    #[test]
    fn a_token_signed_by_another_key_is_refused() {
        let token = mint(&claims(), &keypair().sk).expect("mints");
        let other = KeyPair::from_seed(ed25519_compact::Seed::new([9u8; 32]));

        verify(
            &token,
            &other.pk,
            &identity(),
            SandboxOperationClass::Execute,
            NOW,
        )
        .expect_err("a key the agent does not hold must not authorise anything");
    }

    /// The claim checks are not bypassed by a good signature: a correctly signed capability
    /// for another session is still refused.
    #[test]
    fn a_validly_signed_token_still_fails_its_claim_checks() {
        let keys = keypair();
        let mut other_session = claims();
        other_session.session_id = "s2".to_string();
        let token = mint(&other_session, &keys.sk).expect("mints");

        let error = verify(
            &token,
            &keys.pk,
            &identity(),
            SandboxOperationClass::Execute,
            NOW,
        )
        .expect_err("a signature does not make a capability applicable");
        assert!(error.to_string().contains("different session"));
    }

    #[test]
    fn a_malformed_token_is_refused_without_panicking() {
        let keys = keypair();

        for token in ["", ".", "not-a-token", "a.b", "a.b.c", "$$$.$$$"] {
            verify(
                token,
                &keys.pk,
                &identity(),
                SandboxOperationClass::Execute,
                NOW,
            )
            .expect_err("malformed input must be refused, never parsed leniently");
        }
    }
}
