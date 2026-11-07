//! Verify Google-signed metadata identity tokens before consulting recorded
//! component ownership.
//!
//! Managers call [`GcpIdentityTokenVerifier::verify`] with the token and their
//! own audience, check the returned service account against recorded state,
//! then call [`VerifiedGcpIdentity::ensure_acceptable`] before granting.

use std::{collections::HashSet, sync::Arc, time::Duration as StdDuration};

use alien_error::{AlienError, AlienErrorData};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use jwt_simple::prelude::{
    Audiences, RS256PublicKey, RSAPublicKeyLike, Token, VerificationOptions,
};
use serde::{Deserialize, Serialize};

const GOOGLE_CERTS: &str = "https://www.googleapis.com/oauth2/v3/certs";
const MAX_TOKEN_BYTES: usize = 16 * 1024;
/// How far ahead of our clock a token's `iat` and `nbf` may be, so a caller
/// whose clock runs a little fast is not locked out. This matches the 5 minute
/// tolerance AWS applies to SigV4 timestamps. Expiry is still checked exactly.
pub const ALLOWED_CLOCK_SKEW_SECONDS: u64 = 300;

/// A Google service account proven by a token for this audience. This is
/// cloud identity only; callers must check recorded resource membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedGcpIdentity {
    /// Immutable numeric service account ID (`sub`).
    pub unique_id: String,
    pub email: String,
    /// Token expiry, capped at one hour after issue.
    pub authority_deadline: DateTime<Utc>,
}

impl VerifiedGcpIdentity {
    /// Check the token window again after any wait that follows `verify`.
    pub fn ensure_acceptable(&self, now: DateTime<Utc>) -> Result<(), AlienError> {
        if now >= self.authority_deadline {
            return Err(refused());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, AlienErrorData)]
#[serde(rename_all = "camelCase")]
enum ErrorData {
    #[error(
        code = "RUNTIME_PROOF_REFUSED",
        message = "GCP runtime identity proof refused",
        retryable = "false",
        internal = "false",
        http_status_code = 401
    )]
    Refused,
    #[error(
        code = "RUNTIME_PROOF_UNAVAILABLE",
        message = "Cloud identity verification unavailable",
        retryable = "true",
        internal = "false",
        http_status_code = 503
    )]
    Unavailable,
}

fn refused() -> AlienError {
    AlienError::new(ErrorData::Refused).into_generic()
}

fn unavailable() -> AlienError {
    AlienError::new(ErrorData::Unavailable).into_generic()
}

#[derive(Deserialize)]
struct GoogleKeys {
    keys: Vec<GoogleKey>,
}

#[derive(Deserialize)]
struct GoogleKey {
    kid: String,
    kty: String,
    alg: String,
    #[serde(rename = "use")]
    usage: String,
    n: String,
    e: String,
}

#[derive(Deserialize, Serialize)]
struct GoogleClaims {
    email: Option<String>,
    email_verified: Option<bool>,
}

/// Checks Google's signature, issuer, exact audience and lifetime. The key set
/// URL is fixed; a token header can only select a key inside it.
pub struct GcpIdentityTokenVerifier {
    client: reqwest::Client,
    certs: String,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl GcpIdentityTokenVerifier {
    pub fn new() -> Result<Self, AlienError> {
        Self::with_clock(Arc::new(Utc::now))
    }

    pub fn with_clock(
        clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Result<Self, AlienError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(StdDuration::from_secs(10))
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            client,
            certs: GOOGLE_CERTS.into(),
            clock,
        })
    }

    /// Read keys from a local stand-in instead of Google.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_certs_url(mut self, url: String) -> Self {
        self.certs = url;
        self
    }

    pub async fn verify(
        &self,
        token: &str,
        audience: &str,
    ) -> Result<VerifiedGcpIdentity, AlienError> {
        if token.len() > MAX_TOKEN_BYTES {
            return Err(refused());
        }
        let metadata = Token::decode_metadata(token).map_err(|_| refused())?;
        let kid = metadata
            .key_id()
            .filter(|value| value.len() <= 128)
            .ok_or_else(refused)?
            .to_owned();
        let mut response = self
            .client
            .get(&self.certs)
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(unavailable());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if body.len() + chunk.len() > 64 * 1024 {
                return Err(unavailable());
            }
            body.extend_from_slice(&chunk);
        }
        let keys: GoogleKeys = serde_json::from_slice(&body).map_err(|_| unavailable())?;
        let mut matching = keys.keys.iter().filter(|key| key.kid == kid);
        let key = matching.next().ok_or_else(refused)?;
        if matching.next().is_some() || key.kty != "RSA" || key.alg != "RS256" || key.usage != "sig"
        {
            return Err(refused());
        }
        let n = URL_SAFE_NO_PAD.decode(&key.n).map_err(|_| unavailable())?;
        let e = URL_SAFE_NO_PAD.decode(&key.e).map_err(|_| unavailable())?;
        let key = RS256PublicKey::from_components(&n, &e).map_err(|_| unavailable())?;
        decode(token, audience, &key, (self.clock)())
    }
}

fn decode(
    token: &str,
    audience: &str,
    key: &RS256PublicKey,
    now: DateTime<Utc>,
) -> Result<VerifiedGcpIdentity, AlienError> {
    let now_secs: u64 = now.timestamp().try_into().map_err(|_| refused())?;
    let claims = key
        .verify_token::<GoogleClaims>(
            token,
            Some(VerificationOptions {
                allowed_issuers: Some(HashSet::from([
                    "https://accounts.google.com".to_owned(),
                    "accounts.google.com".to_owned(),
                ])),
                allowed_audiences: Some(HashSet::from([audience.to_owned()])),
                // The library applies this to `nbf` and `exp`. Expiry is
                // checked again below without any tolerance.
                time_tolerance: Some(jwt_simple::prelude::Duration::from_secs(
                    ALLOWED_CLOCK_SKEW_SECONDS,
                )),
                artificial_time: Some(jwt_simple::prelude::Duration::from_secs(now_secs)),
                max_token_length: Some(MAX_TOKEN_BYTES),
                max_header_length: Some(2048),
                ..Default::default()
            }),
        )
        .map_err(|_| refused())?;
    // One exact audience, never a set that merely contains this manager.
    if !matches!(claims.audiences.as_ref(), Some(Audiences::AsString(value)) if value == audience)
        || claims.custom.email_verified != Some(true)
    {
        return Err(refused());
    }
    let issued = claims.issued_at.ok_or_else(refused)?.as_secs();
    let expiry = claims.expires_at.ok_or_else(refused)?.as_secs();
    let deadline = expiry.min(issued.checked_add(3600).ok_or_else(refused)?);
    if issued > now_secs.saturating_add(ALLOWED_CLOCK_SKEW_SECONDS)
        || now_secs >= deadline
        || expiry <= issued
    {
        return Err(refused());
    }
    let unique_id = claims.subject.ok_or_else(refused)?;
    if unique_id.is_empty() || !unique_id.bytes().all(|value| value.is_ascii_digit()) {
        return Err(refused());
    }
    let email = claims
        .custom
        .email
        .filter(|value| !value.is_empty())
        .ok_or_else(refused)?;
    Ok(VerifiedGcpIdentity {
        unique_id,
        email,
        authority_deadline: DateTime::from_timestamp(
            deadline.try_into().map_err(|_| refused())?,
            0,
        )
        .ok_or_else(refused)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jwt_simple::prelude::{Claims, Duration, JWTClaims, RS256KeyPair, RSAKeyPairLike};

    const AUDIENCE: &str = "https://manager-one.example";

    fn claims(now: DateTime<Utc>) -> JWTClaims<GoogleClaims> {
        let mut claims = Claims::with_custom_claims(
            GoogleClaims {
                email: Some("worker@example.iam.gserviceaccount.com".into()),
                email_verified: Some(true),
            },
            Duration::from_secs(3600),
        )
        .with_issuer("https://accounts.google.com")
        .with_audience(AUDIENCE)
        .with_subject("123456789012345678901");
        let timestamp = now.timestamp() as u64;
        claims.issued_at = Some(Duration::from_secs(timestamp));
        claims.invalid_before = Some(Duration::from_secs(timestamp));
        claims.expires_at = Some(Duration::from_secs(timestamp + 3600));
        claims
    }

    #[test]
    fn signed_identity_requires_exact_issuer_audience_key_and_window() {
        let key = RS256KeyPair::generate(2048).unwrap();
        let other = RS256KeyPair::generate(2048).unwrap();
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let token = key.sign(claims(now)).unwrap();
        let verified = decode(&token, AUDIENCE, &key.public_key(), now).unwrap();
        assert_eq!(verified.unique_id, "123456789012345678901");
        assert_eq!(verified.email, "worker@example.iam.gserviceaccount.com");
        assert_eq!(
            verified.authority_deadline,
            now + chrono::Duration::hours(1)
        );
        assert!(verified
            .ensure_acceptable(now + chrono::Duration::minutes(59))
            .is_ok());
        assert!(verified
            .ensure_acceptable(verified.authority_deadline)
            .is_err());
        for result in [
            decode(
                &token,
                "https://manager-two.example",
                &key.public_key(),
                now,
            ),
            decode(&token, AUDIENCE, &other.public_key(), now),
            decode(
                &token,
                AUDIENCE,
                &key.public_key(),
                verified.authority_deadline,
            ),
            decode(
                &token,
                AUDIENCE,
                &key.public_key(),
                now - chrono::Duration::seconds(ALLOWED_CLOCK_SKEW_SECONDS as i64 + 1),
            ),
        ] {
            assert!(result.is_err());
        }
        for variant in 0..5 {
            let mut invalid = claims(now);
            match variant {
                0 => invalid.issuer = Some("https://untrusted.example".into()),
                1 => {
                    invalid.audiences = Some(Audiences::AsSet(HashSet::from([
                        AUDIENCE.into(),
                        "https://manager-two.example".into(),
                    ])))
                }
                2 => invalid.custom.email_verified = Some(false),
                3 => invalid.subject = Some("worker@example.iam.gserviceaccount.com".into()),
                _ => invalid.issued_at = None,
            }
            let signed = key.sign(invalid).unwrap();
            assert!(decode(&signed, AUDIENCE, &key.public_key(), now).is_err());
        }
    }

    #[test]
    fn token_from_a_fast_clock_is_accepted_within_five_minutes() {
        let key = RS256KeyPair::generate(2048).unwrap();
        let issued = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let token = key.sign(claims(issued)).unwrap();
        // The caller's clock runs ahead of ours, so the token looks issued in
        // the future.
        let two_minutes_ahead = decode(
            &token,
            AUDIENCE,
            &key.public_key(),
            issued - chrono::Duration::minutes(2),
        );
        assert_eq!(
            two_minutes_ahead.unwrap().authority_deadline,
            issued + chrono::Duration::hours(1)
        );
        assert!(decode(
            &token,
            AUDIENCE,
            &key.public_key(),
            issued - chrono::Duration::minutes(6),
        )
        .is_err());
        // The tolerance never extends the token past its expiry.
        assert!(decode(
            &token,
            AUDIENCE,
            &key.public_key(),
            issued + chrono::Duration::hours(1),
        )
        .is_err());
    }

    #[test]
    fn a_long_lived_token_is_capped_at_one_hour_after_issue() {
        let key = RS256KeyPair::generate(2048).unwrap();
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let mut long = claims(now);
        long.expires_at = Some(Duration::from_secs(now.timestamp() as u64 + 86_400));
        let token = key.sign(long).unwrap();
        assert_eq!(
            decode(&token, AUDIENCE, &key.public_key(), now)
                .unwrap()
                .authority_deadline,
            now + chrono::Duration::hours(1)
        );
    }
}
