//! Verify audience-bound STS proofs before consulting recorded component ownership.
//!
//! This is the one AWS proof verifier. Managers and machine control planes call
//! [`AwsIdentityProofVerifier::verify`] with the presigned URL and their own
//! audience, then check membership of the returned role, then call
//! [`VerifiedAwsIdentity::ensure_acceptable`] before they grant anything.
//! Proofs come from [`crate::runtime_proof_signer::presign_caller_identity`].

use std::{collections::HashMap, sync::Arc, time::Duration as StdDuration};

use alien_error::{AlienError, AlienErrorData};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};

/// Verified AWS role session. This establishes cloud identity, not permission
/// to access any deployment; callers must check recorded resource membership.
pub struct VerifiedAwsIdentity {
    pub account_id: String,
    pub role_id: String,
    pub role_session_name: String,
    pub assumed_role_arn: String,
    pub signed_at: DateTime<Utc>,
    /// Exact signed request expiry, rechecked after every authorization wait.
    pub acceptance_deadline: DateTime<Utc>,
    pub authority_deadline: DateTime<Utc>,
}

impl VerifiedAwsIdentity {
    /// Check the signed window again after any wait that follows `verify`, such
    /// as a membership lookup. Callers must do this before granting authority.
    pub fn ensure_acceptable(&self, now: DateTime<Utc>) -> Result<(), AlienError> {
        if now >= self.acceptance_deadline {
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
        message = "{reason}",
        retryable = "false",
        internal = "false",
        http_status_code = 401
    )]
    Refused { reason: String },
    #[error(
        code = "RUNTIME_PROOF_UNAVAILABLE",
        message = "Cloud identity verification unavailable",
        retryable = "true",
        internal = "false",
        http_status_code = 503
    )]
    AuthenticationUnavailable,
}

impl ErrorData {
    fn unauthorized(reason: impl Into<String>) -> AlienError<Self> {
        AlienError::new(Self::Refused {
            reason: reason.into(),
        })
    }
}

/// Verifies commercial AWS regional STS proofs. The region is trusted manager
/// configuration, never selected by a caller's URL. Redirects are disabled.
/// This adapter does not support IAM user credentials or other AWS partitions.
pub struct AwsIdentityProofVerifier {
    region: String,
    endpoint: Url,
    /// Where the validated request is sent. Always `endpoint` outside tests.
    transport: Url,
    client: reqwest::Client,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

impl AwsIdentityProofVerifier {
    pub fn new(region: String) -> Result<Self, AlienError> {
        Self::with_clock(region, Arc::new(Utc::now))
    }

    /// Inject a clock to test proof-age boundaries without sleeping.
    pub fn with_clock(
        region: String,
        clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Result<Self, AlienError> {
        if region.is_empty()
            || region.len() > 32
            || !region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(refused());
        }
        let endpoint =
            Url::parse(&format!("https://sts.{region}.amazonaws.com/")).map_err(|_| refused())?;
        let client = reqwest::Client::builder()
            // Keep the signed Host/header representation on HTTP/1.1.
            // Regional STS rejected repeated proofs intermittently over HTTP/2.
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(StdDuration::from_secs(10))
            .build()
            .map_err(|_| AlienError::new(ErrorData::AuthenticationUnavailable).into_generic())?;
        Ok(Self {
            region,
            transport: endpoint.clone(),
            endpoint,
            client,
            clock,
        })
    }

    /// Send validated proofs to a local STS stand-in. Validation still requires
    /// the real regional STS host in the proof itself.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_sts_transport(mut self, transport: Url) -> Self {
        self.transport = transport;
        self
    }

    fn validate_request(
        &self,
        proof: &str,
        now: DateTime<Utc>,
    ) -> Result<(Url, DateTime<Utc>, DateTime<Utc>), AlienError> {
        if proof.len() > 8192 {
            return Err(refused());
        }
        let url = Url::parse(proof).map_err(|_| refused())?;
        if url.scheme() != "https"
            || url.host_str() != self.endpoint.host_str()
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.fragment().is_some()
        {
            return Err(refused());
        }
        let mut query = HashMap::new();
        for (key, value) in url.query_pairs() {
            if !matches!(
                key.as_ref(),
                "Action"
                    | "Version"
                    | "X-Amz-Algorithm"
                    | "X-Amz-Credential"
                    | "X-Amz-Date"
                    | "X-Amz-Expires"
                    | "X-Amz-SignedHeaders"
                    | "X-Amz-Signature"
                    | "X-Amz-Security-Token"
            ) || value.is_empty()
                || query.insert(key.into_owned(), value.into_owned()).is_some()
            {
                return Err(refused());
            }
        }
        let get = |key: &str| query.get(key).map(String::as_str).ok_or_else(refused);
        if get("Action")? != "GetCallerIdentity"
            || get("Version")? != "2011-06-15"
            || get("X-Amz-Algorithm")? != "AWS4-HMAC-SHA256"
            || get("X-Amz-SignedHeaders")? != "host;x-alien-manager-audience"
        {
            return Err(refused());
        }
        // Require temporary role credentials. STS still determines validity;
        // presence of this field alone is never treated as proof.
        get("X-Amz-Security-Token")?;
        let signature = get("X-Amz-Signature")?;
        if signature.len() != 64 || !signature.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(refused());
        }
        let signed_at = NaiveDateTime::parse_from_str(get("X-Amz-Date")?, "%Y%m%dT%H%M%SZ")
            .map_err(|_| refused())?
            .and_utc();
        let expires: i64 = get("X-Amz-Expires")?.parse().map_err(|_| refused())?;
        if !(1..=300).contains(&expires)
            || signed_at > now
            || now >= signed_at + Duration::seconds(expires)
        {
            return Err(refused());
        }
        let scope: Vec<_> = get("X-Amz-Credential")?.split('/').collect();
        if scope.len() != 5
            || scope[0].is_empty()
            || scope[1] != signed_at.format("%Y%m%d").to_string()
            || scope[2] != self.region
            || scope[3] != "sts"
            || scope[4] != "aws4_request"
        {
            return Err(refused());
        }
        Ok((url, signed_at, signed_at + Duration::seconds(expires)))
    }
}

fn refused() -> AlienError {
    ErrorData::unauthorized("AWS runtime identity proof refused").into_generic()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct IdentityResponse {
    get_caller_identity_result: Identity,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Identity {
    account: String,
    user_id: String,
    arn: String,
}

impl AwsIdentityProofVerifier {
    pub async fn verify(
        &self,
        presigned_url: &str,
        audience: &str,
    ) -> Result<VerifiedAwsIdentity, AlienError> {
        // First clock sample: the proof must be inside its signed window now.
        let (mut url, signed_at, acceptance_deadline) =
            self.validate_request(presigned_url, (self.clock)())?;
        if self.transport != self.endpoint {
            url.set_scheme(self.transport.scheme())
                .map_err(|_| refused())?;
            url.set_host(self.transport.host_str())
                .map_err(|_| refused())?;
            url.set_port(self.transport.port()).map_err(|_| refused())?;
        }
        // The caller must have signed this exact configured audience. STS
        // verifies the header's signature; an unsigned audience is refused above.
        let mut response = self
            .client
            .get(url)
            .header("x-alien-manager-audience", audience)
            .send()
            .await
            .map_err(|_| AlienError::new(ErrorData::AuthenticationUnavailable).into_generic())?;
        if response.status().is_server_error()
            || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err(AlienError::new(ErrorData::AuthenticationUnavailable).into_generic());
        }
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AlienError::new(ErrorData::AuthenticationUnavailable).into_generic())?
        {
            if body.len() + chunk.len() > 16384 {
                return Err(refused());
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            #[derive(Deserialize)]
            #[serde(rename_all = "PascalCase")]
            struct Rejection {
                error: RejectionCode,
            }
            #[derive(Deserialize)]
            #[serde(rename_all = "PascalCase")]
            struct RejectionCode {
                code: String,
            }
            let code = quick_xml::de::from_reader::<_, Rejection>(body.as_slice())
                .ok()
                .map(|error| error.error.code);
            let reason = match code.as_deref() {
                Some("SignatureDoesNotMatch" | "InvalidSignatureException") => {
                    "STS signature mismatch"
                }
                Some("ExpiredToken" | "RequestExpired") => "STS proof expired",
                Some("InvalidClientTokenId") => "STS source credential refused",
                _ => "STS runtime identity request refused",
            };
            return Err(ErrorData::unauthorized(reason).into_generic());
        }
        let identity: IdentityResponse =
            quick_xml::de::from_reader(body.as_slice()).map_err(|_| refused())?;
        let identity = identity.get_caller_identity_result;
        let (role_id, session) = identity.user_id.split_once(':').ok_or_else(refused)?;
        if identity.account.len() != 12
            || !identity.account.bytes().all(|b| b.is_ascii_digit())
            || !role_id.starts_with("AROA")
            || !role_id.bytes().all(|b| b.is_ascii_alphanumeric())
            || session.is_empty()
            || !identity
                .arn
                .starts_with(&format!("arn:aws:sts::{}:assumed-role/", identity.account))
        {
            return Err(refused());
        }
        // Second clock sample: the STS round trip may have crossed the deadline.
        if (self.clock)() >= acceptance_deadline {
            return Err(refused());
        }
        // STS does not reveal source credential expiry. Replaying this proof
        // must never reset its fixed signing-time authority deadline.
        Ok(VerifiedAwsIdentity {
            account_id: identity.account,
            role_id: role_id.to_owned(),
            role_session_name: session.to_owned(),
            assumed_role_arn: identity.arn,
            signed_at,
            acceptance_deadline,
            authority_deadline: signed_at + Duration::hours(1),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicI64, Ordering},
        time::{Duration as StdDuration, SystemTime},
    };

    use super::*;
    use crate::fake_sts::{Answer, FakeSts, ROLE_ID, SESSION};

    const REGION: &str = "us-east-1";
    const AUDIENCE: &str = "https://manager-one.example";

    fn signed_at() -> DateTime<Utc> {
        DateTime::from_timestamp(2_000_000_000, 0).unwrap()
    }

    /// A verifier whose clock returns each queued time once, then the last one.
    fn verifier(sts: &FakeSts, clock: &[DateTime<Utc>]) -> AwsIdentityProofVerifier {
        let times: Vec<i64> = clock.iter().map(DateTime::timestamp).collect();
        let next = Arc::new(AtomicI64::new(0));
        AwsIdentityProofVerifier::with_clock(
            REGION.into(),
            Arc::new(move || {
                let index = next.fetch_add(1, Ordering::SeqCst) as usize;
                DateTime::from_timestamp(times[index.min(times.len() - 1)], 0).unwrap()
            }),
        )
        .unwrap()
        .with_sts_transport(sts.transport.clone())
    }

    fn proof(sts: &FakeSts, audience: &str, expires: u64) -> String {
        sts.proof(
            audience,
            SystemTime::from(signed_at()),
            StdDuration::from_secs(expires),
        )
    }

    #[tokio::test]
    async fn valid_proof_names_the_role_and_keeps_the_signed_deadlines() {
        let sts = FakeSts::start(REGION, Answer::Identity).await;
        let verified = verifier(&sts, &[signed_at() + Duration::seconds(10)])
            .verify(&proof(&sts, AUDIENCE, 300), AUDIENCE)
            .await
            .unwrap();
        assert_eq!(verified.role_id, ROLE_ID);
        assert_eq!(verified.role_session_name, SESSION);
        assert_eq!(verified.signed_at, signed_at());
        assert_eq!(
            verified.acceptance_deadline,
            signed_at() + Duration::seconds(300)
        );
        // Verifying later never moves the authority deadline.
        assert_eq!(
            verified.authority_deadline,
            signed_at() + Duration::hours(1)
        );
    }

    #[tokio::test]
    async fn proof_for_another_audience_is_refused_by_the_signature_check() {
        let sts = FakeSts::start(REGION, Answer::Identity).await;
        let error = verifier(&sts, &[signed_at()])
            .verify(&proof(&sts, "https://manager-two.example", 300), AUDIENCE)
            .await
            .err()
            .unwrap();
        assert!(!error.retryable);
        assert_eq!(sts.requests(), 1);
    }

    #[tokio::test]
    async fn tampered_proofs_are_refused_before_reaching_sts() {
        let sts = FakeSts::start(REGION, Answer::Identity).await;
        let valid = proof(&sts, AUDIENCE, 300);
        let unsigned_audience = valid.replace(
            "X-Amz-SignedHeaders=host%3Bx-alien-manager-audience",
            "X-Amz-SignedHeaders=host",
        );
        assert_ne!(unsigned_audience, valid);
        for tampered in [
            unsigned_audience,
            valid.replace("sts.us-east-1.amazonaws.com", "127.0.0.1"),
            valid.replace("sts.us-east-1.amazonaws.com", "sts.eu-west-1.amazonaws.com"),
            valid.replace("Action=GetCallerIdentity", "Action=AssumeRole"),
            format!("{valid}&Action=GetCallerIdentity"),
            valid.replace("https://", "http://"),
            format!("{valid}&unexpected=value"),
            valid.replace("X-Amz-Expires=300", "X-Amz-Expires=3600"),
        ] {
            let error = verifier(&sts, &[signed_at()])
                .verify(&tampered, AUDIENCE)
                .await
                .err()
                .unwrap_or_else(|| panic!("accepted {tampered}"));
            assert!(!error.retryable);
        }
        assert_eq!(sts.requests(), 0);
    }

    #[tokio::test]
    async fn signed_window_is_enforced_at_both_edges() {
        let sts = FakeSts::start(REGION, Answer::Identity).await;
        let valid = proof(&sts, AUDIENCE, 300);
        for (offset, accepted) in [(-1, false), (0, true), (299, true), (300, false)] {
            let result = verifier(&sts, &[signed_at() + Duration::seconds(offset)])
                .verify(&valid, AUDIENCE)
                .await;
            assert_eq!(result.is_ok(), accepted, "offset {offset}");
        }
    }

    #[tokio::test]
    async fn clock_is_checked_again_after_the_sts_round_trip() {
        let sts = FakeSts::start(REGION, Answer::Identity).await;
        let short = proof(&sts, AUDIENCE, 60);
        let result = verifier(
            &sts,
            &[
                signed_at() + Duration::seconds(59),
                signed_at() + Duration::seconds(60),
            ],
        )
        .verify(&short, AUDIENCE)
        .await;
        assert!(result.is_err());
        assert_eq!(sts.requests(), 1);
    }

    #[tokio::test]
    async fn sts_outage_is_retryable_and_never_an_identity() {
        let sts = FakeSts::start(REGION, Answer::Unavailable).await;
        let error = verifier(&sts, &[signed_at()])
            .verify(&proof(&sts, AUDIENCE, 300), AUDIENCE)
            .await
            .err()
            .unwrap();
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn acceptance_is_rechecked_after_a_later_wait() {
        let sts = FakeSts::start(REGION, Answer::Identity).await;
        let verified = verifier(&sts, &[signed_at()])
            .verify(&proof(&sts, AUDIENCE, 60), AUDIENCE)
            .await
            .unwrap();
        assert!(verified
            .ensure_acceptable(signed_at() + Duration::seconds(59))
            .is_ok());
        assert!(verified
            .ensure_acceptable(signed_at() + Duration::seconds(60))
            .is_err());
    }
}
