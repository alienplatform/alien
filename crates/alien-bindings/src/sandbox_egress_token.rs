//! Ed25519 compact JWS tokens for a sandbox egress gateway.
//!
//! The deployment controller owns the signing key. Only sandbox management identities may read it;
//! providers must deliver each minted token outside the shared image snapshot.

use std::fmt;

use alien_core::SandboxEgress;
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bon::Builder;
use ed25519_compact::{KeyPair, SecretKey, Seed};
use pkcs8::der::Decode;
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
    /// SHA-256 digest of the canonical policy stored by the gateway.
    pub policy: String,
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
    /// Loads a controller-generated Ed25519 seed and its rotation key id.
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

    /// Loads the JSON value stored in the deployment's management-only vault.
    /// Its privateKey field is standard base64 of an Ed25519 PKCS#8 DER document.
    pub fn from_vault_json(value: &str, expected_kid: &str) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct StoredKey {
            kid: String,
            private_key: String,
        }
        let stored: StoredKey = serde_json::from_str(value)
            .map_err(|_| invalid("invalid signing-key vault document"))?;
        if stored.kid != expected_kid
            || stored.kid.is_empty()
            || stored.kid.len() > 256
            || stored.kid.chars().any(char::is_control)
        {
            return Err(invalid(
                "vault signing key id differs from the gateway binding",
            ));
        }
        let der = base64::engine::general_purpose::STANDARD
            .decode(&stored.private_key)
            .map_err(|_| invalid("signing key is not base64 PKCS#8"))?;
        let info = pkcs8::PrivateKeyInfo::try_from(der.as_slice())
            .map_err(|_| invalid("signing key is not PKCS#8"))?;
        if info.algorithm.oid != pkcs8::ObjectIdentifier::new_unwrap("1.3.101.112")
            || info.algorithm.parameters.is_some()
        {
            return Err(invalid("signing key is not Ed25519 PKCS#8"));
        }
        let octets = pkcs8::der::asn1::OctetStringRef::from_der(info.private_key)
            .map_err(|_| invalid("invalid Ed25519 private key encoding"))?;
        let seed: [u8; 32] = octets
            .as_bytes()
            .try_into()
            .map_err(|_| invalid("invalid Ed25519 seed length"))?;
        let issuer = Self::from_seed(stored.kid, seed)?;
        if info
            .public_key
            .is_some_and(|public| public != issuer.key.public_key().as_ref())
        {
            return Err(invalid("PKCS#8 public key does not match its private key"));
        }
        Ok(issuer)
    }

    /// Signs an admin request for one sandbox generation, valid for only sixty seconds.
    /// The gateway compares the signed identity, generation and policy digest with the request body.
    pub fn mint_admin(
        &self,
        audience: &str,
        sandbox_id: &str,
        generation: u64,
        policy_digest: &str,
        now_unix: i64,
    ) -> Result<String> {
        validate_identity(audience, sandbox_id)?;
        if now_unix < 0 {
            return Err(invalid("issue time must be Unix seconds"));
        }
        let exp = now_unix
            .checked_add(60)
            .ok_or_else(|| invalid("issue time is outside the supported range"))?;
        if !policy_digest.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            return Err(invalid(
                "policy reference must be a lowercase SHA-256 digest",
            ));
        }
        self.sign(
            "alien-egress-admin",
            &serde_json::json!({
                "iss": "alien-sandbox", "aud": audience, "sub": sandbox_id,
                "gen": generation, "policy": policy_digest, "iat": now_unix, "exp": exp
            }),
        )
    }

    fn sign(&self, kind: &str, claims: &serde_json::Value) -> Result<String> {
        let header = serde_json::json!({ "alg": "EdDSA", "kid": self.kid, "typ": kind });
        let encode = |value| {
            serde_json::to_vec(value)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "encoding sandbox egress token".to_string(),
                })
                .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
        };
        let payload = format!("{}.{}", encode(&header)?, encode(claims)?);
        let signature = self.key.sign(payload.as_bytes(), None);
        Ok(format!(
            "{payload}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }

    /// Signs the normalized effective policy after checking its declaration bound and lifetime.
    pub fn mint(&self, request: EgressTokenRequest<'_>) -> Result<String> {
        validate_identity(request.audience, request.sandbox_id)?;
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
            policy: egress
                .policy_digest()
                .map_err(crate::providers::sandbox::egress::policy_error)?,
            iat: request.now_unix,
            exp: request
                .now_unix
                .saturating_add(86_400)
                .min(request.sandbox_expires_at),
            generation: request.generation,
        };
        let claims = serde_json::to_value(claims).into_alien_error().context(
            ErrorData::SerializationFailed {
                message: "encoding sandbox egress claims".to_string(),
            },
        )?;
        self.sign("JWT", &claims)
    }
}

fn validate_identity(audience: &str, sandbox_id: &str) -> Result<()> {
    let Some((prefix, resource)) = audience.split_once('/') else {
        return Err(invalid("audience must be resourcePrefix/sandboxResourceId"));
    };
    if prefix.is_empty() || resource.is_empty() || resource.contains('/') || sandbox_id.is_empty() {
        return Err(invalid(
            "audience and sandbox id must identify one sandbox resource and instance",
        ));
    }
    Ok(())
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
            assert_eq!(claims.policy, egress.policy_digest().unwrap());
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

#[cfg(test)]
mod admin_tests {
    use super::*;
    use ed25519_compact::Signature;

    #[test]
    fn vault_pkcs8_and_admin_tokens_match_the_public_key_contract() {
        let key = KeyPair::from_seed(Seed::new([19; 32]));
        let secret = serde_json::json!({"kid":"rotation-2", "privateKey":base64::engine::general_purpose::STANDARD.encode(key.sk.to_der())}).to_string();
        let issuer = EgressTokenIssuer::from_vault_json(&secret, "rotation-2").unwrap();
        assert!(EgressTokenIssuer::from_vault_json(&secret, "wrong-kid").is_err());
        let token = issuer
            .mint_admin(
                "prefix/box",
                "session",
                17,
                &SandboxEgress::None.policy_digest().unwrap(),
                1000,
            )
            .unwrap();
        let parts: Vec<_> = token.split('.').collect();
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["typ"], "alien-egress-admin");
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(
            claims,
            serde_json::json!({"iss":"alien-sandbox", "aud":"prefix/box", "sub":"session", "gen":17, "policy": SandboxEgress::None.policy_digest().unwrap(), "iat":1000, "exp":1060})
        );
        key.pk
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap(),
            )
            .unwrap();
        // PKCS#8 v2 embeds the public key; controllers may emit either version.
        use pkcs8::der::Encode;
        let der = key.sk.to_der();
        let mut info = pkcs8::PrivateKeyInfo::try_from(der.as_ref()).unwrap();
        info.public_key = Some(key.pk.as_ref());
        let document = |info: &pkcs8::PrivateKeyInfo<'_>| {
            serde_json::json!({"kid":"rotation-2", "privateKey":base64::engine::general_purpose::STANDARD.encode(info.to_der().unwrap())}).to_string()
        };
        assert!(EgressTokenIssuer::from_vault_json(&document(&info), "rotation-2").is_ok());
        let other = KeyPair::from_seed(Seed::new([20; 32]));
        info.public_key = Some(other.pk.as_ref());
        assert!(EgressTokenIssuer::from_vault_json(&document(&info), "rotation-2").is_err());
        let zero_pkcs8 = serde_json::json!({"kid":"zero", "privateKey":base64::engine::general_purpose::STANDARD.encode([vec![48,46,2,1,0,48,5,6,3,43,101,112,4,34,4,32], vec![0;32]].concat())}).to_string();
        assert!(EgressTokenIssuer::from_vault_json(&zero_pkcs8, "zero").is_err());
    }
}
