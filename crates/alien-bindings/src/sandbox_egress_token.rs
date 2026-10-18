//! Ed25519 compact JWS tokens for a sandbox egress gateway.
//!
//! Deployment setup owns the signing key. Only sandbox management identities may read it;
//! providers must deliver each minted token outside the shared image snapshot.

use std::fmt;

use alien_core::SandboxEgress;
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bon::Builder;
use ed25519_compact::{KeyPair, SecretKey, Seed};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::providers::sandbox::egress::check_bound;

/// Wire claims checked by the egress gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EgressTokenClaims {
    /// Always `alien-sandbox`.
    pub iss: String,
    /// Resource prefix and sandbox resource id, separated by `/`.
    pub aud: String,
    /// Runtime sandbox id.
    pub sub: String,
    /// Effective policy within the declaration.
    pub egress: SandboxEgress,
    /// Issue time, Unix seconds.
    pub iat: i64,
    /// Expiry, Unix seconds, at most 24 hours after issue and before sandbox expiry.
    pub exp: i64,
    /// Monotonically increasing policy generation; independent of lifecycle generation.
    #[serde(rename = "gen")]
    pub generation: u64,
}

/// Inputs owned by the provider creating or updating the sandbox.
#[derive(Debug, Builder)]
pub struct EgressTokenRequest<'a> {
    /// Deployment-scoped gateway audience.
    pub audience: &'a str,
    /// Runtime sandbox id.
    pub sandbox_id: &'a str,
    /// Declaration's upper bound.
    pub bound: &'a SandboxEgress,
    /// Per-sandbox override; absence uses the bound.
    pub egress: Option<&'a SandboxEgress>,
    /// Policy generation allocated durably by the provider.
    pub generation: u64,
    /// Time of issue, Unix seconds.
    pub now_unix: i64,
    /// Sandbox's absolute maximum lifetime deadline, Unix seconds.
    pub sandbox_expires_at: i64,
}

/// An issuer with a deployment key. Debug output never includes the private key.
pub struct EgressTokenIssuer {
    kid: String,
    key: SecretKey,
}

impl fmt::Debug for EgressTokenIssuer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EgressTokenIssuer")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

impl EgressTokenIssuer {
    /// Loads a setup-generated Ed25519 seed and its rotation key id.
    pub fn from_seed(kid: String, seed: [u8; 32]) -> Result<Self> {
        if kid.is_empty() || kid.len() > 256 || kid.chars().any(char::is_control) || seed == [0; 32]
        {
            return Err(invalid(
                "a nonempty key id and nonzero Ed25519 seed are required",
            ));
        }
        Ok(Self {
            kid,
            key: KeyPair::from_seed(Seed::new(seed)).sk,
        })
    }

    /// Signs the normalized effective policy after checking its declaration bound and lifetime.
    pub fn mint(&self, request: EgressTokenRequest<'_>) -> Result<String> {
        let Some((prefix, resource)) = request.audience.split_once('/') else {
            return Err(invalid("audience must be resourcePrefix/sandboxResourceId"));
        };
        if prefix.is_empty()
            || resource.is_empty()
            || resource.contains('/')
            || request.sandbox_id.is_empty()
        {
            return Err(invalid(
                "audience and sandbox id must identify one sandbox resource and instance",
            ));
        }
        if request.now_unix < 0 || request.sandbox_expires_at <= request.now_unix {
            return Err(invalid(
                "sandbox lifetime must end after the token is issued",
            ));
        }
        let egress = request.egress.unwrap_or(request.bound);
        check_bound(egress, Some(request.bound))?;
        let claims = EgressTokenClaims {
            iss: "alien-sandbox".to_string(),
            aud: request.audience.to_string(),
            sub: request.sandbox_id.to_string(),
            egress: egress.clone(),
            iat: request.now_unix,
            exp: request
                .now_unix
                .saturating_add(86_400)
                .min(request.sandbox_expires_at),
            generation: request.generation,
        };
        let header = serde_json::json!({ "alg": "EdDSA", "kid": self.kid, "typ": "JWT" });
        let encode = |value| {
            serde_json::to_vec(value)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "encoding sandbox egress token".to_string(),
                })
                .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
        };
        let claims = serde_json::to_value(claims).into_alien_error().context(
            ErrorData::SerializationFailed {
                message: "encoding sandbox egress claims".to_string(),
            },
        )?;
        let payload = format!("{}.{}", encode(&header)?, encode(&claims)?);
        let signature = self.key.sign(payload.as_bytes(), None);
        Ok(format!(
            "{payload}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
}

fn invalid(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxEgressInvalid {
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::Signature;

    #[test]
    fn minted_jws_verifies_and_carries_the_bounded_policy_and_generation() {
        let issuer = EgressTokenIssuer::from_seed("key-1".to_string(), [7; 32]).unwrap();
        let bound = SandboxEgress::Internet;
        let egress = SandboxEgress::Allow {
            allow: vec!["API.Example.COM.".to_string()],
        };
        for (deadline, expected_expiry) in [(1200, 1200), (200000, 87400)] {
            let token = issuer
                .mint(
                    EgressTokenRequest::builder()
                        .audience("stack/box")
                        .sandbox_id("session-1")
                        .bound(&bound)
                        .egress(&egress)
                        .generation(42)
                        .now_unix(1000)
                        .sandbox_expires_at(deadline)
                        .build(),
                )
                .unwrap();
            let parts: Vec<_> = token.split('.').collect();
            assert_eq!(parts.len(), 3);
            let header: serde_json::Value =
                serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
            assert_eq!(header["alg"], "EdDSA");
            assert_eq!(header["kid"], "key-1");
            let claims: EgressTokenClaims =
                serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
            assert_eq!(
                (
                    claims.iss.as_str(),
                    claims.aud.as_str(),
                    claims.sub.as_str()
                ),
                ("alien-sandbox", "stack/box", "session-1")
            );
            assert_eq!(
                (claims.iat, claims.exp, claims.generation),
                (1000, expected_expiry, 42)
            );
            assert_eq!(claims.egress, egress.normalized().unwrap());
            let payload = format!("{}.{}", parts[0], parts[1]);
            let signature =
                Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
            let public = KeyPair::from_seed(Seed::new([7; 32])).pk;
            public.verify(payload.as_bytes(), &signature).unwrap();
            assert!(public
                .verify(format!("{payload}tampered").as_bytes(), &signature)
                .is_err());
        }
    }

    #[test]
    fn minting_refuses_widening_and_expired_sandboxes() {
        let issuer = EgressTokenIssuer::from_seed("key-1".to_string(), [7; 32]).unwrap();
        let bound = SandboxEgress::None;
        let egress = SandboxEgress::Internet;
        let error = issuer
            .mint(
                EgressTokenRequest::builder()
                    .audience("stack/box")
                    .sandbox_id("s1")
                    .bound(&bound)
                    .egress(&egress)
                    .generation(0)
                    .now_unix(1000)
                    .sandbox_expires_at(1100)
                    .build(),
            )
            .unwrap_err();
        assert_eq!(error.code, "SANDBOX_EGRESS_WIDENS");
        assert!(issuer
            .mint(
                EgressTokenRequest::builder()
                    .audience("stack/box")
                    .sandbox_id("s1")
                    .bound(&bound)
                    .generation(0)
                    .now_unix(1000)
                    .sandbox_expires_at(1000)
                    .build()
            )
            .is_err());
        assert!(EgressTokenIssuer::from_seed("key-1".to_string(), [0; 32]).is_err());
    }
}
