//! Per-cloud ambient credential injection.
//!
//! Each provider authorizes an outgoing upstream request with the workload's
//! ambient identity and never reads a static key from the binding or env:
//! - AWS: SigV4-sign using the SDK default chain (env / profile / SSO / IMDS /
//!   IRSA), which on a workload is the instance role. The signing service name is
//!   per-request: `bedrock` for the OpenAI-compatible endpoint, `bedrock-mantle`
//!   for the Anthropic Messages endpoint.
//! - GCP / Azure: attach a bearer token fetched from the instance metadata service
//!   (GCE metadata server / Azure IMDS), cached until shortly before it expires.

use std::sync::Arc;
use std::time::{Duration, Instant};

use alien_bindings::provider::LazyEnvBindingsProvider;
use alien_core::{AwsCredentials, AzureCredentials, ClientConfig, GcpCredentials};
use alien_error::{AlienError, Context, IntoAlienError};
use aws_credential_types::provider::error::CredentialsError;
use aws_credential_types::provider::{future, ProvideCredentials, SharedCredentialsProvider};
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use http::{HeaderName, HeaderValue};
use tokio::sync::Mutex;

use crate::error::{ErrorData, Result};

/// The shared workload credential resolver, resolved fresh on each authorization so a
/// long-lived gateway re-mints short-lived credentials before they expire. Held behind an
/// `Arc` because every route the mint-gated workload serves shares one provider (one mint
/// cache, one refresh clock).
type Managed = Arc<LazyEnvBindingsProvider>;

/// AWS credentials provider backed by the runtime-less resolver. The SigV4 signer calls
/// `provide_credentials` on every request, and `LazyEnvBindingsProvider::provider` re-mints
/// only when the cached credential is near expiry, so this refreshes transparently.
///
/// The resolver is native-first: it yields minted keys only when the workload has no
/// projected identity. So both shapes reach here, and an `Imds` / `Profile` / `WebIdentity`
/// config is resolved through the SDK default chain — the same fallback
/// [`AwsSigV4Cred::from_client_config`] uses. Rejecting it would fail every request on a
/// mint-gated workload that has a projected identity.
#[derive(Debug)]
struct ManagedAwsCredentials {
    provider: Managed,
    /// The SDK default chain, built once on the first projected-identity request.
    native: tokio::sync::OnceCell<SharedCredentialsProvider>,
}

impl ManagedAwsCredentials {
    fn new(provider: Managed) -> Self {
        Self { provider, native: tokio::sync::OnceCell::new() }
    }

    /// The SDK default chain, which resolves and refreshes the workload's projected identity.
    async fn native_chain(&self) -> std::result::Result<&SharedCredentialsProvider, CredentialsError> {
        self.native
            .get_or_try_init(|| async {
                let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
                config.credentials_provider().ok_or_else(|| {
                    CredentialsError::not_loaded("the AWS default credential chain provided no credentials provider")
                })
            })
            .await
    }

    async fn resolve(&self) -> std::result::Result<Credentials, CredentialsError> {
        let provider = self.provider.provider().await.map_err(CredentialsError::provider_error)?;
        let aws = provider
            .client_config()
            .aws_config()
            .ok_or_else(|| CredentialsError::not_loaded("the workload identity is not an AWS credential"))?;
        match &aws.credentials {
            AwsCredentials::AccessKeys { access_key_id, secret_access_key, session_token } => Ok(
                Credentials::new(access_key_id, secret_access_key, session_token.clone(), None, "alien-managed"),
            ),
            AwsCredentials::SessionCredentials { access_key_id, secret_access_key, session_token, .. } => Ok(
                Credentials::new(access_key_id, secret_access_key, Some(session_token.clone()), None, "alien-managed"),
            ),
            // The resolver selected the workload's projected identity (Imds / Profile /
            // WebIdentity); the SDK default chain resolves and refreshes it.
            _ => self.native_chain().await?.provide_credentials().await,
        }
    }
}

impl ProvideCredentials for ManagedAwsCredentials {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.resolve())
    }
}

/// Injects the workload's ambient identity into an outgoing upstream request.
pub enum AmbientCred {
    Aws(AwsSigV4Cred),
    Bearer(BearerTokenCred),
}

impl AmbientCred {
    /// Authorize an outgoing upstream request. `aws_sigv4_service` is the SigV4
    /// service name (`bedrock` or `bedrock-mantle`); it is consumed only by the AWS
    /// variant and ignored by bearer-token clouds.
    pub async fn authorize(&self, req: &mut reqwest::Request, aws_sigv4_service: &str) -> Result<()> {
        match self {
            AmbientCred::Aws(c) => c.sign(req, aws_sigv4_service).await,
            AmbientCred::Bearer(c) => c.attach(req).await,
        }
    }
}

/// SigV4 signer for AWS Bedrock using the SDK default credential chain.
pub struct AwsSigV4Cred {
    region: String,
    provider: SharedCredentialsProvider,
}

impl AwsSigV4Cred {
    /// Resolve credentials from the SDK default chain (workload instance role in
    /// production; env / profile / SSO locally).
    pub async fn new(region: impl Into<String>) -> Result<Self> {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let provider = config.credentials_provider().ok_or_else(|| {
            AlienError::new(ErrorData::WorkloadIdentityInvalid {
                message: "the AWS default credential chain provided no credentials provider".to_string(),
            })
        })?;
        Ok(Self { region: region.into(), provider })
    }

    /// Build from an explicit credentials provider (used by tests).
    pub fn with_provider(region: impl Into<String>, provider: SharedCredentialsProvider) -> Self {
        Self { region: region.into(), provider }
    }

    /// Sign with credentials the runtime-less resolver mints and refreshes. Each request
    /// re-resolves through the shared provider, so minted credentials never go stale.
    pub fn managed(region: impl Into<String>, provider: Managed) -> Self {
        Self {
            region: region.into(),
            provider: SharedCredentialsProvider::new(ManagedAwsCredentials::new(provider)),
        }
    }

    /// Build from the workload's resolved `ClientConfig` — the identity the runtime-less
    /// credential resolver selected: the native/projected identity, or Alien-minted
    /// short-lived credentials. Explicit keys/session credentials are signed directly;
    /// an instance-role/profile config falls back to the SDK default chain, which
    /// resolves (and refreshes) them.
    pub async fn from_client_config(region: impl Into<String>, config: &ClientConfig) -> Result<Self> {
        let region = region.into();
        let aws = config.aws_config().ok_or_else(|| {
            AlienError::new(ErrorData::WorkloadIdentityInvalid {
                message: "the workload ClientConfig is not an AWS config".to_string(),
            })
        })?;
        match &aws.credentials {
            AwsCredentials::AccessKeys {
                access_key_id,
                secret_access_key,
                session_token,
            } => {
                let creds = Credentials::new(
                    access_key_id,
                    secret_access_key,
                    session_token.clone(),
                    None,
                    "alien-workload",
                );
                Ok(Self::with_provider(region, SharedCredentialsProvider::new(creds)))
            }
            AwsCredentials::SessionCredentials {
                access_key_id,
                secret_access_key,
                session_token,
                ..
            } => {
                let creds = Credentials::new(
                    access_key_id,
                    secret_access_key,
                    Some(session_token.clone()),
                    None,
                    "alien-workload",
                );
                Ok(Self::with_provider(region, SharedCredentialsProvider::new(creds)))
            }
            // Imds / Profile: the SDK default chain resolves the projected identity.
            _ => Self::new(region).await,
        }
    }

    async fn sign(&self, req: &mut reqwest::Request, service: &str) -> Result<()> {
        let creds = self
            .provider
            .provide_credentials()
            .await
            .into_alien_error()
            .context(ErrorData::AmbientCredentialUnavailable {
                message: format!("the SigV4 signer for {service} got no credentials from the provider"),
            })?;
        let identity = creds.into();
        let params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.region)
            .name(service)
            .time(std::time::SystemTime::now())
            .settings(SigningSettings::default())
            .build()
            .into_alien_error()
            .context(ErrorData::Other { message: "could not build SigV4 params".to_string() })?;

        // SigV4 hashes the exact body; a non-in-memory (streaming) body would sign as
        // empty and silently fail upstream, so reject it rather than mis-sign.
        let body_bytes: Vec<u8> = match req.body() {
            Some(body) => body
                .as_bytes()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::Other {
                        message: "cannot SigV4-sign a streaming (non-in-memory) request body".to_string(),
                    })
                })?
                .to_vec(),
            None => Vec::new(),
        };
        let uri = req.url().to_string();
        let mut headers: Vec<(String, String)> = req
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string())))
            .collect();
        // SigV4 must cover the Host header; reqwest sets it from the URL at send time,
        // so sign the same value (with port when non-default) for the signature to match.
        if let Some(host) = req.url().host_str() {
            let host_header = match req.url().port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            };
            headers.push(("host".to_string(), host_header));
        }
        let signable = SignableRequest::new(
            req.method().as_str(),
            &uri,
            headers.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            SignableBody::Bytes(&body_bytes),
        )
        .into_alien_error()
        .context(ErrorData::Other { message: "could not build signable request".to_string() })?;

        let (instructions, _signature) = sign(signable, &params.into())
            .into_alien_error()
            .context(ErrorData::Other { message: "SigV4 signing failed".to_string() })?
            .into_parts();

        for (name, value) in instructions.headers() {
            let hn = HeaderName::from_bytes(name.as_bytes())
                .into_alien_error()
                .context(ErrorData::Other { message: format!("invalid signed header name {name}") })?;
            let hv = HeaderValue::from_str(value)
                .into_alien_error()
                .context(ErrorData::Other { message: "invalid signed header value".to_string() })?;
            req.headers_mut().insert(hn, hv);
        }
        Ok(())
    }
}

/// Where a bearer token comes from: the GCE metadata server, Azure IMDS, or a
/// pre-supplied token (local-dev ADC / `az` CLI token, or tests).
enum BearerSource {
    Gcp,
    Azure { resource: String },
    Static { token: String },
    /// The runtime-less resolver: re-resolve the workload's bearer token on each request,
    /// re-minting when the short-lived credential is near expiry. The resolver is
    /// native-first, so it may instead select the workload's projected identity, which
    /// carries no bearer token — `native` is the metadata source that issues one.
    Managed { provider: Managed, native: Box<BearerSource> },
}

/// Attaches an ambient bearer token — fetched and cached from the cloud metadata endpoint,
/// or supplied directly (static / minted).
pub struct BearerTokenCred {
    source: BearerSource,
    client: reqwest::Client,
    cache: Mutex<Option<(String, Instant)>>,
}

impl BearerTokenCred {
    pub fn gcp() -> Self {
        Self { source: BearerSource::Gcp, client: reqwest::Client::new(), cache: Mutex::new(None) }
    }

    /// `resource` is the audience, e.g. `https://cognitiveservices.azure.com`.
    pub fn azure(resource: impl Into<String>) -> Self {
        Self {
            source: BearerSource::Azure { resource: resource.into() },
            client: reqwest::Client::new(),
            cache: Mutex::new(None),
        }
    }

    /// Use a pre-supplied bearer token instead of the metadata service — for
    /// local development (an ADC / `az` CLI token) or tests.
    pub fn static_token(token: impl Into<String>) -> Self {
        Self {
            source: BearerSource::Static { token: token.into() },
            client: reqwest::Client::new(),
            cache: Mutex::new(None),
        }
    }

    /// Resolve the GCP bearer token from the runtime-less resolver, falling back to the
    /// metadata service when the resolver selects the workload's projected identity.
    pub fn managed_gcp(provider: Managed) -> Self {
        Self::managed(provider, BearerSource::Gcp)
    }

    /// Resolve the Azure bearer token from the runtime-less resolver, falling back to IMDS
    /// for `resource` when the resolver selects the workload's projected identity.
    pub fn managed_azure(provider: Managed, resource: impl Into<String>) -> Self {
        Self::managed(provider, BearerSource::Azure { resource: resource.into() })
    }

    fn managed(provider: Managed, native: BearerSource) -> Self {
        Self {
            source: BearerSource::Managed { provider, native: Box::new(native) },
            client: reqwest::Client::new(),
            cache: Mutex::new(None),
        }
    }

    async fn attach(&self, req: &mut reqwest::Request) -> Result<()> {
        let token = self.token().await?;
        let hv = HeaderValue::from_str(&format!("Bearer {token}"))
            .into_alien_error()
            .context(ErrorData::Other { message: "invalid bearer token".to_string() })?;
        req.headers_mut().insert(http::header::AUTHORIZATION, hv);
        Ok(())
    }

    async fn token(&self) -> Result<String> {
        match &self.source {
            // A supplied token has no refresh clock of its own, and the resolver holds one
            // for minted credentials, so neither uses the local metadata cache.
            BearerSource::Static { token } => Ok(token.clone()),
            BearerSource::Managed { provider, native } => match self.managed_token(provider, native).await? {
                Some(token) => Ok(token),
                None => self.metadata_token(native).await,
            },
            native => self.metadata_token(native).await,
        }
    }

    /// The bearer token the resolver selected, or `None` when it selected the workload's
    /// projected identity — which carries no token of its own, so the metadata service
    /// issues one.
    async fn managed_token(&self, provider: &Managed, native: &BearerSource) -> Result<Option<String>> {
        let resolved = provider.provider().await.context(ErrorData::AmbientCredentialUnavailable {
            message: "the workload credential resolver failed".to_string(),
        })?;
        Ok(match native {
            BearerSource::Gcp => resolved.client_config().gcp_config().and_then(|g| match &g.credentials {
                GcpCredentials::AccessToken { token } => Some(token.clone()),
                _ => None,
            }),
            BearerSource::Azure { .. } => resolved.client_config().azure_config().and_then(|a| match &a.credentials {
                AzureCredentials::AccessToken { token } => Some(token.clone()),
                _ => None,
            }),
            // `managed` only ever builds a Gcp / Azure native source.
            BearerSource::Static { .. } | BearerSource::Managed { .. } => None,
        })
    }

    /// Cache-then-fetch the workload's projected-identity token from the instance metadata
    /// service. `source` must be `Gcp` or `Azure`.
    async fn metadata_token(&self, source: &BearerSource) -> Result<String> {
        if let Some((tok, exp)) = self.cache.lock().await.as_ref() {
            if Instant::now() < *exp {
                return Ok(tok.clone());
            }
        }

        let (url, header_name, header_value) = match source {
            BearerSource::Gcp => (
                "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token".to_string(),
                "Metadata-Flavor",
                "Google".to_string(),
            ),
            BearerSource::Azure { resource } => (
                format!("http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource={resource}"),
                "Metadata",
                "true".to_string(),
            ),
            BearerSource::Static { .. } | BearerSource::Managed { .. } => {
                return Err(AlienError::new(ErrorData::Other {
                    message: "no metadata endpoint for this credential source".to_string(),
                }))
            }
        };

        let resp = self
            .client
            .get(&url)
            .header(header_name, header_value)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::AmbientCredentialUnavailable {
                message: "could not reach the instance metadata service".to_string(),
            })?;
        if !resp.status().is_success() {
            return Err(AlienError::new(ErrorData::AmbientCredentialUnavailable {
                message: format!("metadata token endpoint returned {}", resp.status()),
            }));
        }
        // GCP returns expires_in as a number; Azure IMDS returns it as a string.
        let v: serde_json::Value = resp
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::Other { message: "could not parse the metadata token response".to_string() })?;
        let access_token = v["access_token"]
            .as_str()
            .ok_or_else(|| AlienError::new(ErrorData::Other {
                message: "metadata token response had no access_token".to_string(),
            }))?
            .to_string();
        // A wrong shape here would silently cache a token past its real expiry, and there is
        // no 401-triggered invalidation to recover — so fail rather than invent a lifetime.
        let expires_in = v["expires_in"]
            .as_u64()
            .or_else(|| v["expires_in"].as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| {
                AlienError::new(ErrorData::AmbientCredentialUnavailable {
                    message: "metadata token response had no usable expires_in".to_string(),
                })
            })?;

        let exp = Instant::now() + Duration::from_secs(expires_in.saturating_sub(60));
        *self.cache.lock().await = Some((access_token.clone(), exp));
        Ok(access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn aws_signer_adds_sigv4_headers() {
        // Hermetic: sign with explicit static creds (no env / network needed).
        let creds = aws_credential_types::Credentials::new(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            None,
            None,
            "test",
        );
        let cred = AwsSigV4Cred::with_provider("us-east-2", SharedCredentialsProvider::new(creds));

        let mut req = reqwest::Client::new()
            .post("https://bedrock-runtime.us-east-2.amazonaws.com/openai/v1/chat/completions")
            .body(r#"{"model":"openai.gpt-oss-20b-1:0","messages":[]}"#)
            .build()
            .expect("request builds");

        cred.sign(&mut req, "bedrock").await.expect("signing succeeds");

        let auth = req
            .headers()
            .get(http::header::AUTHORIZATION)
            .expect("authorization header present")
            .to_str()
            .unwrap();
        assert!(auth.contains("AWS4-HMAC-SHA256"), "must be a SigV4 auth header: {auth}");
        assert!(auth.contains("/bedrock/"), "credential scope must name the bedrock service: {auth}");
        assert!(req.headers().contains_key("x-amz-date"), "must add x-amz-date");
    }

    #[tokio::test]
    async fn aws_signer_signs_classic_invoke_stream_url() {
        // Claude routes through classic InvokeModel; the signer must handle the
        // profile id in the URL path and sign it under the same `bedrock` service.
        let creds = aws_credential_types::Credentials::new(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            None,
            None,
            "test",
        );
        let cred = AwsSigV4Cred::with_provider("us-east-2", SharedCredentialsProvider::new(creds));

        let mut req = reqwest::Client::new()
            .post("https://bedrock-runtime.us-east-2.amazonaws.com/model/us.anthropic.claude-haiku-4-5-20251001-v1:0/invoke-with-response-stream")
            .body(r#"{"anthropic_version":"bedrock-2023-05-31","messages":[]}"#)
            .build()
            .expect("request builds");

        cred.sign(&mut req, "bedrock").await.expect("signing succeeds");

        let auth = req
            .headers()
            .get(http::header::AUTHORIZATION)
            .expect("authorization header present")
            .to_str()
            .unwrap();
        assert!(
            auth.contains("/bedrock/"),
            "credential scope must name the bedrock service: {auth}"
        );
    }
}
