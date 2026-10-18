use std::collections::HashMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_compact::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

use crate::error::{denied, Result};

/// Maximum token size, before decoding. Bounds allocation before signature verification.
pub const MAX_TOKEN_BYTES: usize = 4 * 1024;

/// Signed sandbox identity. Policy is read independently from the deployment store.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Claims {
    /// Required issuer: alien-sandbox.
    pub iss: String,
    /// Deployment prefix and sandbox resource ID.
    pub aud: String,
    /// Sandbox instance ID.
    pub sub: String,
    /// Issued-at Unix timestamp.
    pub iat: u64,
    /// Expiry Unix timestamp.
    pub exp: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    kid: String,
    #[serde(default)]
    typ: Option<String>,
}

/// Verify Ed25519 JWS and all time/identity constraints. No network key discovery.
pub fn verify(token: &str, keys: &HashMap<String, PublicKey>, now: u64) -> Result<Claims> {
    let claims: Claims = verify_signed(token, keys, None)?;
    if claims.iss != "alien-sandbox"
        || claims.aud.is_empty()
        || claims.sub.is_empty()
        || claims.aud.len() > 256
        || claims.sub.len() > 256
        || claims.iat > now.saturating_add(60)
        || claims.exp <= now
        || claims.exp <= claims.iat
        || claims.exp - claims.iat > 86400
    {
        return Err(denied("invalid identity or token lifetime"));
    }
    Ok(claims)
}

pub fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn verify_signed<T: serde::de::DeserializeOwned>(
    token: &str,
    keys: &HashMap<String, PublicKey>,
    expected_type: Option<&str>,
) -> Result<T> {
    verify_signed_limit(token, keys, expected_type, MAX_TOKEN_BYTES)
}

pub(crate) fn verify_signed_limit<T: serde::de::DeserializeOwned>(
    token: &str,
    keys: &HashMap<String, PublicKey>,
    expected_type: Option<&str>,
    limit: usize,
) -> Result<T> {
    if token.len() > limit {
        return Err(denied("token too large"));
    }
    let (signed, signature) = token
        .rsplit_once('.')
        .ok_or_else(|| denied("malformed token"))?;
    let (header, payload) = signed
        .split_once('.')
        .ok_or_else(|| denied("malformed token"))?;
    let header: Header = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(header)
            .map_err(|_| denied("malformed token"))?,
    )
    .map_err(|_| denied("malformed token header"))?;
    if header.alg != "EdDSA"
        || !match expected_type {
            Some(expected) => header.typ.as_deref() == Some(expected),
            None => header.typ.as_deref().is_none_or(|typ| typ == "JWT"),
        }
    {
        return Err(denied("unsupported token algorithm or type"));
    }
    let key = keys
        .get(&header.kid)
        .ok_or_else(|| denied("unknown signing key"))?;
    let signature = Signature::from_slice(
        &URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| denied("malformed signature"))?,
    )
    .map_err(|_| denied("malformed signature"))?;
    key.verify(signed.as_bytes(), &signature)
        .map_err(|_| denied("invalid signature"))?;
    let claims: T = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| denied("malformed claims"))?,
    )
    .map_err(|_| denied("malformed claims"))?;
    Ok(claims)
}

/// An admin grant binds the request method and policy to exactly one sandbox.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminClaims {
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub method: String,
    pub iat: u64,
    pub exp: u64,
    /// Required for policy mutation; CA and metrics grants may omit it.
    #[serde(default)]
    pub policy: Option<String>,
    /// Required and request-bound for PUT and DELETE.
    #[serde(default)]
    pub version: Option<u64>,
}

/// Verify a short-lived admin JWS, distinct from proxy tokens by its mandatory type.
pub fn verify_admin(
    token: &str,
    keys: &HashMap<String, PublicKey>,
    now: u64,
) -> Result<AdminClaims> {
    let claims: AdminClaims = verify_signed(token, keys, Some("alien-egress-admin"))?;
    if claims.iss != "alien-sandbox"
        || claims.aud.is_empty()
        || claims.sub.is_empty()
        || claims.aud.len() > 256
        || claims.sub.len() > 256
        || claims.iat > now.saturating_add(60)
        || claims.exp <= now
        || claims.exp <= claims.iat
        || claims.exp - claims.iat > 60
    {
        return Err(denied("invalid admin identity or lifetime"));
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};

    fn signed(pair: &KeyPair, payload: serde_json::Value) -> String {
        let data = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"test"}"#),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
        );
        format!(
            "{data}.{}",
            URL_SAFE_NO_PAD.encode(pair.sk.sign(data.as_bytes(), None).as_ref())
        )
    }

    #[test]
    fn signature_and_lifetime_are_required() {
        let pair = KeyPair::from_seed(Seed::new([7; 32]));
        let keys = HashMap::from([("test".into(), pair.pk)]);
        let payload = serde_json::json!({"iss":"alien-sandbox", "aud":"prefix/resource", "sub":"box", "iat":100, "exp":200});
        let token = signed(&pair, payload.clone());
        assert_eq!(verify(&token, &keys, 100).unwrap().sub, "box");
        assert!(verify(&token, &keys, 200).is_err());
        assert!(verify(&token, &keys, 40).is_ok());
        assert!(verify(&token, &keys, 39).is_err());
        let other = KeyPair::from_seed(Seed::new([8; 32]));
        assert!(verify(&signed(&other, payload.clone()), &keys, 100).is_err());
        let mut long = payload.clone();
        long["exp"] = 100000.into();
        assert!(verify(&signed(&pair, long), &keys, 100).is_err());
        let mut wrong = payload;
        wrong["iss"] = "other".into();
        assert!(verify(&signed(&pair, wrong), &keys, 100).is_err());
    }

    #[test]
    fn identity_tokens_are_bounded() {
        let pair = KeyPair::from_seed(Seed::new([7; 32]));
        let keys = HashMap::from([("test".into(), pair.pk)]);
        let payload = serde_json::json!({"iss":"alien-sandbox", "aud":"prefix/resource", "sub":"box", "iat":100, "exp":200});
        let token = signed(&pair, payload);
        assert!(token.len() < MAX_TOKEN_BYTES);
        verify(&token, &keys, 100).unwrap();
        assert!(verify(&"x".repeat(MAX_TOKEN_BYTES + 1), &keys, 100).is_err());
    }
}

#[cfg(test)]
mod admin_tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};

    #[test]
    fn admin_type_and_sixty_second_lifetime_are_mandatory() {
        let pair = KeyPair::from_seed(Seed::new([11; 32]));
        let keys = HashMap::from([("admin".into(), pair.pk)]);
        let sign = |typ: &str, exp: u64| {
            let header = serde_json::json!({"alg":"EdDSA","kid":"admin","typ":typ});
            let payload = serde_json::json!({"iss":"alien-sandbox","aud":"p/r","sub":"box","method":"PUT","iat":100,"exp":exp});
            let input = format!(
                "{}.{}",
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap()),
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap())
            );
            format!(
                "{input}.{}",
                URL_SAFE_NO_PAD.encode(pair.sk.sign(input.as_bytes(), None).as_ref())
            )
        };
        assert_eq!(
            verify_admin(&sign("alien-egress-admin", 160), &keys, 100)
                .unwrap()
                .method,
            "PUT"
        );
        assert!(verify_admin(&sign("alien-egress-admin", 160), &keys, 40).is_ok());
        assert!(verify_admin(&sign("alien-egress-admin", 160), &keys, 39).is_err());
        assert!(verify_admin(&sign("JWT", 160), &keys, 100).is_err());
        assert!(verify_admin(&sign("alien-egress-admin", 161), &keys, 100).is_err());
        assert!(verify_admin(&sign("alien-egress-admin", 160), &keys, 160).is_err());
        assert!(verify(&sign("alien-egress-admin", 160), &keys, 100).is_err());
    }
}
