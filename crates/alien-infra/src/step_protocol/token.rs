//! The short-lived token sent with each step over HTTPS, so a controller's server acts only on
//! steps Alien sent for the deployment.
//!
//! ```text
//! base64url(claims_json) "." base64url(ed25519 signature)
//! ```
//!
//! The same format as `alien_core::sandbox_capability_token`: the signature covers the encoded
//! claims segment, and verification runs against the bytes that arrived, before the claims are
//! parsed. Minting and verification live together so they can't drift apart. Whoever calls them
//! holds the keys.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_compact::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};

/// What a token says about the step it was minted for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Claims {
    /// The Alien manager that signed it.
    pub iss: String,
    /// The controller URL the release pins. A token for one server is refused by another.
    pub aud: String,
    /// The deployment's project. A server takes steps only for the project it serves, so a
    /// release of another project that names its URL gets nothing done there.
    pub project: String,
    pub deployment_id: String,
    /// The extension whose controller the step is for.
    pub extension: String,
    pub resource_id: String,
    pub step_id: String,
    /// The SHA-256 of the request body, base64url: the token is good for that body only.
    pub body_sha256: String,
    /// Issued at, in Unix seconds.
    pub iat: i64,
    /// Expires at, in Unix seconds.
    pub exp: i64,
    /// The token's unique id.
    pub jti: String,
}

/// What a token must say: the manager the server trusts, the server's own URL, and the request's
/// fields.
#[derive(Debug, Clone, Copy)]
pub struct Expected<'a> {
    /// The URL of the Alien manager the server takes steps from.
    pub issuer: &'a str,
    pub audience: &'a str,
    /// The projects the server serves.
    pub projects: &'a [&'a str],
    pub deployment_id: &'a str,
    pub extension: &'a str,
    pub resource_id: &'a str,
    pub step_id: &'a str,
    /// The body that arrived.
    pub body: &'a [u8],
}

/// How long a step token stays valid. Longer than a step's deadline, so a server whose clock
/// runs a little ahead still accepts it.
pub const TOKEN_LIFETIME: std::time::Duration = std::time::Duration::from_secs(300);

/// How far ahead of the server's clock a signer's clock may run.
const CLOCK_SKEW_SECS: i64 = 60;

/// Signs claims into a token.
pub fn mint(claims: &Claims, secret_key: &SecretKey) -> Result<String> {
    let json =
        serde_json::to_vec(claims)
            .into_alien_error()
            .context(ErrorData::ExtensionStepFailed {
                resource_id: claims.resource_id.clone(),
                message: "Failed to serialize the step token's claims".to_string(),
            })?;
    let payload = URL_SAFE_NO_PAD.encode(json);
    let signature = secret_key.sign(payload.as_bytes(), None);
    Ok(format!(
        "{payload}.{}",
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}

/// Why a token was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenRefusal {
    Malformed,
    /// None of the trusted keys signed it. A server that fetches its manager's keys fetches
    /// them again for this one.
    Signature,
    Expired,
    IssuedInTheFuture,
    /// From another issuer, which it names.
    Issuer(String),
    /// For another audience, which it names.
    Audience(String),
    /// For another project, which it names.
    Project(String),
    /// For another deployment, extension, resource or step than the request's.
    Mismatch,
}

impl std::fmt::Display for TokenRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => f.write_str("token is malformed"),
            Self::Signature => f.write_str("token signature is not valid"),
            Self::Expired => f.write_str("token has expired"),
            Self::IssuedInTheFuture => f.write_str("token was issued in the future"),
            Self::Issuer(issuer) => write!(f, "token is from another issuer ({issuer})"),
            Self::Audience(audience) => write!(f, "token is for another audience ({audience})"),
            Self::Project(project) => write!(f, "token is for another project ({project})"),
            Self::Mismatch => f.write_str("token does not match the request"),
        }
    }
}

/// The extension a resource type belongs to: the part before its `/`, as `acme` in
/// `acme/dns-zone`, or the whole type when it has none. The manager mints the `extension` claim
/// with it and a server checks the claim with it, so both derive it the same way.
pub fn extension_of(resource_type: &str) -> &str {
    resource_type
        .split_once('/')
        .map_or(resource_type, |(extension, _)| extension)
}

/// Whether `token` reads as one `issuer` signed for `audience`, before its signature is
/// checked. A server that fetches its manager's keys asks this first, so a token that could
/// never pass costs no fetch. Its claims are not trusted until [`verify`] passes.
pub fn addressed_to(
    token: &str,
    issuer: &str,
    audience: &str,
) -> std::result::Result<(), TokenRefusal> {
    let (payload, _) = token.split_once('.').ok_or(TokenRefusal::Malformed)?;
    let json = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| TokenRefusal::Malformed)?;
    let claims: Claims = serde_json::from_slice(&json).map_err(|_| TokenRefusal::Malformed)?;
    if claims.iss != issuer {
        return Err(TokenRefusal::Issuer(claims.iss));
    }
    if claims.aud != audience {
        return Err(TokenRefusal::Audience(claims.aud));
    }
    Ok(())
}

/// Verifies a token against the keys the server trusts and the request it came with,
/// returning its claims only if every check holds.
///
/// A server trusts more than one key while the signer rotates its key: the old one until its
/// tokens expire, and the new one.
pub fn verify(
    token: &str,
    trusted_keys: &[PublicKey],
    expected: Expected<'_>,
    now_unix: i64,
) -> std::result::Result<Claims, TokenRefusal> {
    let (payload, signature) = token.split_once('.').ok_or(TokenRefusal::Malformed)?;
    // A second separator means a third segment nobody agreed on.
    if signature.contains('.') {
        return Err(TokenRefusal::Malformed);
    }
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| TokenRefusal::Malformed)?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| TokenRefusal::Malformed)?;
    if !trusted_keys
        .iter()
        .any(|key| key.verify(payload.as_bytes(), &signature).is_ok())
    {
        return Err(TokenRefusal::Signature);
    }

    let json = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| TokenRefusal::Malformed)?;
    let claims: Claims = serde_json::from_slice(&json).map_err(|_| TokenRefusal::Malformed)?;
    if claims.exp <= now_unix {
        return Err(TokenRefusal::Expired);
    }
    if claims.iat > now_unix + CLOCK_SKEW_SECS {
        return Err(TokenRefusal::IssuedInTheFuture);
    }
    if claims.iss != expected.issuer {
        return Err(TokenRefusal::Issuer(claims.iss));
    }
    if claims.aud != expected.audience {
        return Err(TokenRefusal::Audience(claims.aud));
    }
    if !expected.projects.contains(&claims.project.as_str()) {
        return Err(TokenRefusal::Project(claims.project));
    }
    if claims.deployment_id != expected.deployment_id
        || claims.extension != expected.extension
        || claims.resource_id != expected.resource_id
        || claims.step_id != expected.step_id
        || claims.body_sha256 != body_sha256(expected.body)
    {
        return Err(TokenRefusal::Mismatch);
    }
    Ok(claims)
}

/// The `body_sha256` claim of a request body.
pub fn body_sha256(body: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(body))
}

/// A step-token key as one entry of the JWKS a manager publishes: an Ed25519 key whose `kid` is
/// its RFC 7638 thumbprint, so the same key always has the same id.
pub fn jwk(key: &PublicKey) -> serde_json::Value {
    let x = URL_SAFE_NO_PAD.encode(key.as_ref());
    // RFC 7638: the key's required members, in lexicographic order, with no whitespace.
    let thumbprint = Sha256::digest(format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{x}"}}"#));
    serde_json::json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "x": x,
        "kid": URL_SAFE_NO_PAD.encode(thumbprint),
        "alg": "EdDSA",
        "use": "sig",
    })
}

/// The Ed25519 keys of a JWKS document. Entries of another kind are skipped.
pub fn keys_from_jwks(jwks: &serde_json::Value) -> Vec<PublicKey> {
    jwks["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|key| key["kty"] == "OKP" && key["crv"] == "Ed25519")
        .filter_map(|key| URL_SAFE_NO_PAD.decode(key["x"].as_str()?).ok())
        .filter_map(|bytes| PublicKey::from_slice(&bytes).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};

    const NOW: i64 = 1_000_000;
    const AUD: &str = "https://controllers.acme.dev";
    const ISS: &str = "https://manager.example.com";
    const BODY: &[u8] = br#"{"stepId":"s1"}"#;

    fn key(seed: u8) -> KeyPair {
        KeyPair::from_seed(Seed::new([seed; 32]))
    }

    fn claims() -> Claims {
        Claims {
            iss: ISS.into(),
            aud: AUD.into(),
            project: "prj_1".into(),
            deployment_id: "dep_1".into(),
            extension: "acme".into(),
            resource_id: "zone".into(),
            step_id: "s1".into(),
            body_sha256: body_sha256(BODY),
            iat: NOW - 10,
            exp: NOW + 300,
            jti: "j1".into(),
        }
    }

    fn expected() -> Expected<'static> {
        Expected {
            issuer: ISS,
            audience: AUD,
            projects: &["prj_1"],
            deployment_id: "dep_1",
            extension: "acme",
            resource_id: "zone",
            step_id: "s1",
            body: BODY,
        }
    }

    #[test]
    fn a_token_verifies_against_the_key_that_signed_it() {
        let key = key(1);
        let token = mint(&claims(), &key.sk).unwrap();
        assert_eq!(
            verify(&token, &[key.pk], expected(), NOW).unwrap(),
            claims()
        );
    }

    #[test]
    fn each_failed_check_names_its_reason() {
        let other = key(2).pk;
        let key = key(1);
        let sign = |c: Claims| mint(&c, &key.sk).unwrap();
        let cases: [(String, Vec<PublicKey>, &str); 12] = [
            (
                sign(Claims {
                    project: "prj_2".into(),
                    ..claims()
                }),
                vec![key.pk],
                "another project (prj_2)",
            ),
            (
                sign(Claims {
                    body_sha256: body_sha256(br#"{"stepId":"s1","state":"forged"}"#),
                    ..claims()
                }),
                vec![key.pk],
                "does not match",
            ),
            (
                sign(Claims {
                    iss: "https://another-manager.example.com".into(),
                    ..claims()
                }),
                vec![key.pk],
                "another issuer",
            ),
            (
                sign(Claims {
                    iat: NOW + 61,
                    ..claims()
                }),
                vec![key.pk],
                "issued in the future",
            ),
            (
                sign(Claims {
                    extension: "other".into(),
                    ..claims()
                }),
                vec![key.pk],
                "does not match",
            ),
            (sign(claims()), vec![other], "signature is not valid"),
            (
                sign(Claims {
                    exp: NOW,
                    ..claims()
                }),
                vec![key.pk],
                "expired",
            ),
            (
                sign(Claims {
                    aud: "https://elsewhere.dev".into(),
                    ..claims()
                }),
                vec![key.pk],
                "another audience",
            ),
            (
                sign(Claims {
                    deployment_id: "dep_2".into(),
                    ..claims()
                }),
                vec![key.pk],
                "does not match",
            ),
            (
                sign(Claims {
                    step_id: "s2".into(),
                    ..claims()
                }),
                vec![key.pk],
                "does not match",
            ),
            ("not.a.token".to_string(), vec![key.pk], "malformed"),
            (format!("{}.x", sign(claims())), vec![key.pk], "malformed"),
        ];
        for (token, keys, reason) in cases {
            let error = verify(&token, &keys, expected(), NOW)
                .unwrap_err()
                .to_string();
            assert!(error.contains(reason), "{reason}: {error}");
        }
    }

    /// The claims are read only after the signature checks out, so a forged payload with valid
    /// claims and someone else's signature is refused for its signature.
    #[test]
    fn a_payload_with_another_tokens_signature_is_refused() {
        let key = key(1);
        let token = mint(&claims(), &key.sk).unwrap();
        let other = mint(
            &Claims {
                step_id: "s2".into(),
                ..claims()
            },
            &key.sk,
        )
        .unwrap();
        let (payload, _) = token.split_once('.').unwrap();
        let (_, signature) = other.split_once('.').unwrap();
        let error = verify(
            &format!("{payload}.{signature}"),
            &[key.pk],
            expected(),
            NOW,
        )
        .unwrap_err();
        assert_eq!(error, TokenRefusal::Signature);
    }

    /// The thumbprint is RFC 7638's, checked against RFC 8037's Ed25519 example, and the JWKS
    /// reads back the key it was made from.
    #[test]
    fn a_jwk_has_the_rfc_thumbprint_and_reads_back() {
        let x = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";
        let key = PublicKey::from_slice(&URL_SAFE_NO_PAD.decode(x).unwrap()).unwrap();
        let jwk = jwk(&key);
        assert_eq!(jwk["x"], x);
        assert_eq!(jwk["kid"], "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k");
        let jwks = serde_json::json!({
            "keys": [{ "kty": "RSA", "n": "AQAB", "e": "AQAB" }, jwk]
        });
        assert_eq!(keys_from_jwks(&jwks), vec![key]);
    }

    /// A type without a `/` is its own extension, on both sides of the token.
    #[test]
    fn the_extension_of_a_type_is_its_first_part() {
        assert_eq!(extension_of("acme/dns-zone"), "acme");
        assert_eq!(extension_of("acme"), "acme");
    }
}
