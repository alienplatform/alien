use std::collections::HashMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_compact::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

use crate::{
    error::{denied, Result},
    policy::Policy,
};

/// Maximum token size, before decoding. Bounds allocation before signature verification.
pub const MAX_TOKEN_BYTES: usize = 24 * 1024;

/// Signed identity and narrowed egress grant.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Claims {
    /// Required issuer: alien-sandbox.
    pub iss: String,
    /// Deployment prefix and sandbox resource ID.
    pub aud: String,
    /// Sandbox instance ID.
    pub sub: String,
    /// Narrowed policy.
    pub egress: Policy,
    /// Issued-at Unix timestamp.
    pub iat: u64,
    /// Expiry Unix timestamp.
    pub exp: u64,
    /// Policy generation.
    pub gen: u64,
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
    if token.len() > MAX_TOKEN_BYTES {
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
    if header.alg != "EdDSA" || header.typ.as_deref().is_some_and(|typ| typ != "JWT") {
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
    let claims: Claims = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| denied("malformed claims"))?,
    )
    .map_err(|_| denied("malformed claims"))?;
    if claims.iss != "alien-sandbox"
        || claims.aud.is_empty()
        || claims.sub.is_empty()
        || claims.aud.len() > 256
        || claims.sub.len() > 256
        || claims.iat > now
        || claims.exp <= now
        || claims.exp <= claims.iat
        || claims.exp - claims.iat > 86400
    {
        return Err(denied("invalid identity or token lifetime"));
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
        let payload = serde_json::json!({"iss":"alien-sandbox", "aud":"prefix/resource", "sub":"box", "egress":"internet", "iat":100, "exp":200, "gen":0});
        let token = signed(&pair, payload.clone());
        assert_eq!(verify(&token, &keys, 100).unwrap().sub, "box");
        assert!(verify(&token, &keys, 200).is_err());
        assert!(verify(&token, &keys, 99).is_err());
        let other = KeyPair::from_seed(Seed::new([8; 32]));
        assert!(verify(&signed(&other, payload.clone()), &keys, 100).is_err());
        let mut long = payload.clone();
        long["exp"] = 100000.into();
        assert!(verify(&signed(&pair, long), &keys, 100).is_err());
        let mut wrong = payload;
        wrong["iss"] = "other".into();
        assert!(verify(&signed(&pair, wrong), &keys, 100).is_err());
    }
}
