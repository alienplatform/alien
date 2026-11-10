//! Proofs of the cloud identity a Worker runs as.
//!
//! The manager checks a proof with the cloud that issued it, so the runtime never holds a
//! credential of its own for log export: it only shows what its cloud already says about it.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fmt::Debug, time::Duration};

const AUDIENCE_HEADER: &str = "x-alien-identity-audience";
const STS_BODY: &str = "Action=GetCallerIdentity&Version=2011-06-15";
const METADATA_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_METADATA_TOKEN_BYTES: usize = 8 * 1024;

/// Produces the `proof` member of a credentials request for one manager.
#[async_trait]
pub trait IdentityProver: Send + Sync + Debug {
    /// `audience` is the id of the manager the proof is for. A proof made for another
    /// audience is refused, so a manager cannot replay a proof it received elsewhere.
    async fn prove(&self, audience: &str) -> Result<Value>;
}

/// Proves an AWS role with a signed `sts:GetCallerIdentity` request. The runtime does not
/// send it: the manager does, and STS answers with the role only if the signature holds.
#[cfg(feature = "aws")]
pub struct AwsIdentity {
    credentials: alien_aws_clients::AwsCredentialProvider,
}

#[cfg(feature = "aws")]
impl Debug for AwsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsIdentity").finish_non_exhaustive()
    }
}

#[cfg(feature = "aws")]
impl AwsIdentity {
    pub async fn from_env(env: &std::collections::HashMap<String, String>) -> Result<Self> {
        use alien_aws_clients::AwsClientConfigExt as _;

        let config = alien_aws_clients::AwsClientConfig::from_env(env)
            .await
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "AWS credentials are not available to prove the Worker's role".to_string(),
            })?;
        let credentials = alien_aws_clients::AwsCredentialProvider::from_config(config)
            .await
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "AWS credentials could not be resolved".to_string(),
            })?;
        Ok(Self { credentials })
    }

    pub fn from_provider(credentials: alien_aws_clients::AwsCredentialProvider) -> Self {
        Self { credentials }
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl IdentityProver for AwsIdentity {
    async fn prove(&self, audience: &str) -> Result<Value> {
        use alien_aws_clients::aws::aws_request_utils::{AwsRequestSigner as _, AwsSignConfig};

        self.credentials
            .ensure_fresh()
            .await
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "AWS credentials could not be refreshed".to_string(),
            })?;
        let region = self.credentials.region().to_string();
        let url = format!("https://sts.{region}.amazonaws.com/");

        // The proof is only built here, never sent, so this client needs no timeouts.
        let request = reqwest::Client::new()
            .post(&url)
            .header(
                "content-type",
                "application/x-www-form-urlencoded; charset=utf-8",
            )
            .header(AUDIENCE_HEADER, audience)
            .body(STS_BODY)
            .sign_aws_request(&AwsSignConfig {
                service_name: "sts".to_string(),
                region,
                credentials: self.credentials.get_credentials(),
                signing_region: None,
            })
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "The STS identity request could not be signed".to_string(),
            })?
            .build()
            .into_alien_error()
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "The STS identity request could not be built".to_string(),
            })?;

        let headers = request
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_string(), value.to_str().ok()?.to_string()))
            })
            .collect::<BTreeMap<_, _>>();
        Ok(json!({ "aws": { "url": url, "headers": headers, "body": STS_BODY } }))
    }
}

/// Proves a Google service account with an ID token from the metadata server. The token is
/// minted for the manager's id, so Google's other services will not accept it.
#[derive(Debug)]
pub struct GcpIdentity {
    http: reqwest::Client,
    metadata_host: String,
}

impl GcpIdentity {
    pub fn from_env(env: &std::collections::HashMap<String, String>) -> Result<Self> {
        let metadata_host = env
            .get("GCE_METADATA_HOST")
            .cloned()
            .unwrap_or_else(|| "metadata.google.internal".to_string());
        Self::new(metadata_host)
    }

    pub fn new(metadata_host: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(METADATA_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .into_alien_error()
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "The metadata client could not be built".to_string(),
            })?;
        Ok(Self {
            http,
            metadata_host,
        })
    }
}

#[async_trait]
impl IdentityProver for GcpIdentity {
    async fn prove(&self, audience: &str) -> Result<Value> {
        let response = self
            .http
            .get(format!(
                "http://{}/computeMetadata/v1/instance/service-accounts/default/identity",
                self.metadata_host
            ))
            .query(&[("audience", audience), ("format", "full")])
            .header("metadata-flavor", "Google")
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::TelemetryCredentialsFailed {
                message: "The metadata server could not be reached".to_string(),
            })?;
        if !response.status().is_success() {
            return Err(AlienError::new(ErrorData::TelemetryCredentialsFailed {
                message: format!("The metadata server answered {}", response.status()),
            }));
        }
        let mut body = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await.into_alien_error().context(
            ErrorData::TelemetryCredentialsFailed {
                message: "The metadata server reply could not be read".to_string(),
            },
        )? {
            if body.len() + chunk.len() > MAX_METADATA_TOKEN_BYTES {
                return Err(AlienError::new(ErrorData::TelemetryCredentialsFailed {
                    message: "The metadata server reply is too large".to_string(),
                }));
            }
            body.extend_from_slice(&chunk);
        }
        let token = String::from_utf8(body).into_alien_error().context(
            ErrorData::TelemetryCredentialsFailed {
                message: "The metadata server reply is not text".to_string(),
            },
        )?;
        let token = token.trim();
        if token.is_empty() || token.len() > MAX_METADATA_TOKEN_BYTES {
            return Err(AlienError::new(ErrorData::TelemetryCredentialsFailed {
                message: "The metadata server returned no usable ID token".to_string(),
            }));
        }
        Ok(json!({ "gcp": { "token": token } }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{Query, State},
        http::{HeaderMap, StatusCode},
        routing::get,
        Router,
    };
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    type Seen = Arc<Mutex<Vec<(HashMap<String, String>, String)>>>;

    async fn metadata_token(
        State((seen, status)): State<(Seen, StatusCode)>,
        Query(query): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> (StatusCode, &'static str) {
        let flavor = headers
            .get("metadata-flavor")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        seen.lock().unwrap().push((query, flavor));
        (status, "header.payload.signature\n")
    }

    async fn metadata_server(status: StatusCode) -> (String, Seen) {
        let seen = Seen::default();
        let app = Router::new()
            .route(
                "/computeMetadata/v1/instance/service-accounts/default/identity",
                get(metadata_token),
            )
            .with_state((seen.clone(), status));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (host, seen)
    }

    #[tokio::test]
    async fn gcp_asks_the_metadata_server_for_a_token_made_for_the_manager() {
        let (host, seen) = metadata_server(StatusCode::OK).await;

        let proof = GcpIdentity::new(host)
            .unwrap()
            .prove("mgr_42")
            .await
            .unwrap();

        assert_eq!(
            proof,
            json!({ "gcp": { "token": "header.payload.signature" } })
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0["audience"], "mgr_42");
        assert_eq!(seen[0].0["format"], "full");
        assert_eq!(seen[0].1, "Google");
    }

    #[tokio::test]
    async fn gcp_stops_reading_a_metadata_reply_that_never_ends() {
        use futures_util::stream;

        let app = Router::new().route(
            "/computeMetadata/v1/instance/service-accounts/default/identity",
            get(|| async {
                axum::body::Body::from_stream(stream::repeat_with(|| {
                    Ok::<_, std::convert::Infallible>(bytes::Bytes::from(vec![b'a'; 4096]))
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let started = std::time::Instant::now();
        let error = GcpIdentity::new(host)
            .unwrap()
            .prove("mgr_42")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("too large"), "{error}");
        assert!(started.elapsed() < METADATA_TIMEOUT / 2);
    }

    #[tokio::test]
    async fn gcp_fails_when_the_metadata_server_refuses() {
        let (host, _) = metadata_server(StatusCode::NOT_FOUND).await;

        let error = GcpIdentity::new(host)
            .unwrap()
            .prove("mgr_42")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("404"), "{error}");
    }

    #[cfg(feature = "aws")]
    #[tokio::test]
    async fn aws_proof_is_a_signed_sts_request_bound_to_the_manager() {
        use alien_core::{AwsClientConfig, AwsCredentials};

        let credentials = alien_aws_clients::AwsCredentialProvider::from_config(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "eu-west-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "ASIAEXAMPLE".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: Some("session-token".to_string()),
            },
            service_overrides: None,
        })
        .await
        .unwrap();

        let proof = AwsIdentity::from_provider(credentials)
            .prove("mgr_42")
            .await
            .unwrap();

        let aws = &proof["aws"];
        assert_eq!(aws["url"], "https://sts.eu-west-1.amazonaws.com/");
        assert_eq!(aws["body"], "Action=GetCallerIdentity&Version=2011-06-15");
        let headers = aws["headers"].as_object().unwrap();
        assert_eq!(headers["host"], "sts.eu-west-1.amazonaws.com");
        assert_eq!(headers[AUDIENCE_HEADER], "mgr_42");
        assert_eq!(headers["x-amz-security-token"], "session-token");
        let date = headers["x-amz-date"].as_str().unwrap();
        let signed_at = chrono::NaiveDateTime::parse_from_str(date, "%Y%m%dT%H%M%SZ").unwrap();
        assert!(
            (chrono::Utc::now() - signed_at.and_utc())
                .num_seconds()
                .abs()
                < 30
        );

        let authorization = headers["authorization"].as_str().unwrap();
        let credential_scope = format!(
            "Credential=ASIAEXAMPLE/{}/eu-west-1/sts/aws4_request",
            &date[..8]
        );
        assert!(
            authorization.starts_with("AWS4-HMAC-SHA256 "),
            "{authorization}"
        );
        assert!(authorization.contains(&credential_scope), "{authorization}");
        let signed = authorization
            .split("SignedHeaders=")
            .nth(1)
            .and_then(|rest| rest.split(',').next())
            .unwrap()
            .split(';')
            .collect::<Vec<_>>();
        for required in [
            "host",
            "x-amz-date",
            AUDIENCE_HEADER,
            "x-amz-security-token",
        ] {
            assert!(
                signed.contains(&required),
                "{required} is not signed: {signed:?}"
            );
        }
        assert!(!headers.contains_key("content-length"));
    }
}
