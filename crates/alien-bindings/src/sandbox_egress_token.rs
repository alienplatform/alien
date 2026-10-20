//! Ed25519 compact JWS tokens for a sandbox egress gateway.
//!
//! The deployment controller owns the signing key. Only sandbox management identities may read it;
//! providers must deliver each minted token outside the shared image snapshot.

use std::fmt;

use alien_core::EgressPolicy;
use alien_error::{AlienError, Context, IntoAlienError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use bon::Builder;
use ed25519_compact::{KeyPair, SecretKey, Seed};
use pkcs8::der::Decode;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};

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
    /// Issue time, Unix seconds.
    pub iat: i64,
    /// Expiry, Unix seconds, at most 24 hours after issue and before sandbox expiry.
    pub exp: i64,
}

/// Inputs owned by the provider creating or updating the sandbox.
#[derive(Debug, Builder)]
pub struct EgressTokenRequest<'a> {
    /// Deployment-scoped gateway audience.
    pub audience: &'a str,
    /// Runtime sandbox id.
    pub sandbox_id: &'a str,
    /// Time of issue, Unix seconds.
    pub now_unix: i64,
    /// Sandbox's absolute maximum lifetime deadline, Unix seconds.
    pub sandbox_expires_at: i64,
}

/// The exact policy operation authorized by a short-lived admin token.
pub enum EgressAdminOperation {
    /// Read the latest signed record, including a deletion tombstone.
    Read,
    /// Replace the policy with a signed record.
    Put,
    /// Store a signed deletion tombstone.
    Delete,
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

    /// The key id tokens from this issuer carry.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// Loads the JSON value stored in the deployment's management-only vault, requiring the key
    /// id `expected_kid`. Its privateKey field is standard base64 of an Ed25519 PKCS#8 DER
    /// document.
    pub fn from_vault_json(value: &str, expected_kid: &str) -> Result<Self> {
        let issuer = Self::from_vault_document(value)?;
        if issuer.kid != expected_kid {
            return Err(invalid(
                "vault signing key id differs from the gateway binding",
            ));
        }
        Ok(issuer)
    }

    /// Loads the JSON value stored in the deployment's management-only vault, with the key id it
    /// records. The vault holds the key the gateway configuration trusts, so a workload signing
    /// with it needs no key id of its own: one copied into its binding goes stale when the key
    /// changes.
    pub fn from_vault_document(value: &str) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct StoredKey {
            kid: String,
            private_key: String,
        }
        let stored: StoredKey = serde_json::from_str(value)
            .map_err(|_| invalid("invalid signing-key vault document"))?;
        if stored.kid.is_empty()
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

    /// Authorizes the identity and HTTP method with a sixty-second lifetime.
    pub fn mint_admin(
        &self,
        audience: &str,
        sandbox_id: &str,
        operation: EgressAdminOperation,
        now_unix: i64,
    ) -> Result<String> {
        validate_identity(audience, sandbox_id)?;
        if now_unix < 0 {
            return Err(invalid("issue time must be Unix seconds"));
        }
        let exp = now_unix
            .checked_add(60)
            .ok_or_else(|| invalid("issue time is outside the supported range"))?;
        let method = match operation {
            EgressAdminOperation::Read => "GET",
            EgressAdminOperation::Put => "PUT",
            EgressAdminOperation::Delete => "DELETE",
        };
        let claims = serde_json::json!({"iss":"alien-sandbox","aud":audience,"sub":sandbox_id,
            "method":method,"iat":now_unix,"exp":exp});
        self.sign("alien-egress-admin", &claims)
    }

    /// Sign a persistent policy record. Only its outer admin request expires.
    pub fn mint_record(
        &self,
        audience: &str,
        sandbox_id: &str,
        version: (u64, &str),
        policy: Option<&EgressPolicy>,
    ) -> Result<String> {
        validate_identity(audience, sandbox_id)?;
        let mut claims = serde_json::json!({"v":1,"aud":audience,"sub":sandbox_id,"ver":version});
        if let Some(policy) = policy {
            claims["policy"] = serde_json::from_slice(
                &policy
                    .canonical_json()
                    .map_err(crate::providers::sandbox::egress::policy_error)?,
            )
            .map_err(|_| invalid("could not encode policy record"))?;
        } else {
            claims["deleted"] = true.into();
        }
        self.sign("alien-egress-record", &claims)
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

    /// Signs sandbox identity only, bounded by its absolute lifetime.
    pub fn mint(&self, request: EgressTokenRequest<'_>) -> Result<String> {
        validate_identity(request.audience, request.sandbox_id)?;
        if request.now_unix < 0 || request.sandbox_expires_at <= request.now_unix {
            return Err(invalid(
                "sandbox lifetime must end after the token is issued",
            ));
        }
        let claims = EgressTokenClaims {
            iss: "alien-sandbox".to_string(),
            aud: request.audience.to_string(),
            sub: request.sandbox_id.to_string(),
            iat: request.now_unix,
            exp: request
                .now_unix
                .saturating_add(86_400)
                .min(request.sandbox_expires_at),
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
    fn identity_tokens_verify_and_expire_with_the_sandbox() {
        let issuer = EgressTokenIssuer::from_seed("key-1".into(), [7; 32]).unwrap();
        for (deadline, exp) in [(1200, 1200), (200000, 87400)] {
            let token = issuer
                .mint(
                    EgressTokenRequest::builder()
                        .audience("stack/box")
                        .sandbox_id("s1")
                        .now_unix(1000)
                        .sandbox_expires_at(deadline)
                        .build(),
                )
                .unwrap();
            let parts: Vec<_> = token.split('.').collect();
            let claims: serde_json::Value =
                serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
            assert_eq!(
                claims,
                serde_json::json!({"iss":"alien-sandbox","aud":"stack/box","sub":"s1","iat":1000,"exp":exp})
            );
            let payload = format!("{}.{}", parts[0], parts[1]);
            let signature =
                Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
            let key = KeyPair::from_seed(Seed::new([7; 32])).pk;
            key.verify(payload.as_bytes(), &signature).unwrap();
            assert!(key
                .verify(format!("{payload}tampered").as_bytes(), &signature)
                .is_err());
        }
        assert!(issuer
            .mint(
                EgressTokenRequest::builder()
                    .audience("stack/box")
                    .sandbox_id("s1")
                    .now_unix(1000)
                    .sandbox_expires_at(1000)
                    .build()
            )
            .is_err());
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
            .mint_admin("prefix/box", "session", EgressAdminOperation::Put, 1000)
            .unwrap();
        let parts: Vec<_> = token.split('.').collect();
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["typ"], "alien-egress-admin");
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(
            claims,
            serde_json::json!({"iss":"alien-sandbox", "aud":"prefix/box", "sub":"session", "method":"PUT", "iat":1000, "exp":1060})
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
