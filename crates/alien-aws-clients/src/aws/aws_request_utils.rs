use aws_credential_types::Credentials;
use aws_sigv4::{
    http_request::{sign, SignableBody, SignableRequest, SigningSettings},
    sign::v4,
};
use http;
use sha2::{Digest, Sha256};
use std::time::SystemTime;
#[cfg(target_arch = "wasm32")]
use std::time::{Duration, UNIX_EPOCH};
use tracing::{debug, trace};

use alien_client_core::RequestBuilderExt;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::RequestBuilder;
use serde::de::DeserializeOwned;

/// Get the current time in a WASM-compatible way.
///
/// On native platforms, uses `SystemTime::now()`.
/// On WASM platforms, uses `js_sys::Date` to get time from JavaScript.
#[cfg(not(target_arch = "wasm32"))]
fn get_current_time() -> SystemTime {
    SystemTime::now()
}

#[cfg(target_arch = "wasm32")]
fn get_current_time() -> SystemTime {
    let millis = js_sys::Date::now();
    let secs = (millis / 1000.0) as u64;
    let nanos = ((millis % 1000.0) * 1_000_000.0) as u32;
    UNIX_EPOCH + Duration::new(secs, nanos)
}

/// Configuration needed to sign an AWS request.
#[derive(Debug, Clone)]
pub struct AwsSignConfig {
    /// AWS service name (e.g. "s3", "lambda", "iam").
    pub service_name: String,
    /// AWS region (e.g. "us-east-1").
    pub region: String,
    /// Credentials that should be used for signing.
    pub credentials: Credentials,
    /// Optional alternate signing region (used by services such as IAM).
    pub signing_region: Option<String>,
}

/// Extension trait that enables AWS SigV4 signing on request builders (both the raw
/// `reqwest::RequestBuilder` and the retry wrapper from `crate::request_utils`).
pub trait AwsRequestSigner: Sized {
    /// Sign the request and return a new builder containing the signed request so that
    /// further combinators (e.g. `with_retry`, `send_json`) can be chained.
    fn sign_aws_request(self, config: &AwsSignConfig) -> Result<Self>;
}

impl AwsRequestSigner for reqwest::RequestBuilder {
    fn sign_aws_request(self, config: &AwsSignConfig) -> Result<Self> {
        // First build the request.
        let (client, req_result) = self.build_split();

        let reqwest_request =
            req_result
                .into_alien_error()
                .context(ErrorData::RequestSignError {
                    message: format!(
                        "Unable to build reqwest::Request for {} service",
                        config.service_name
                    ),
                })?;

        // Extract body bytes (if available).
        let body_bytes = reqwest_request
            .body()
            .and_then(|b| b.as_bytes().map(|b| b.to_vec()))
            .unwrap_or_default();

        debug!(
            service = %config.service_name,
            region = %config.region,
            method = %reqwest_request.method(),
            url = %reqwest_request.url(),
            body_len = body_bytes.len(),
            "Signing AWS request"
        );

        // Build an http::Request<String> that we can feed to the signing API.
        let mut http_request_builder = http::Request::builder()
            .method(reqwest_request.method().as_str())
            .uri(reqwest_request.url().as_str());

        for (name, value) in reqwest_request.headers() {
            if let Ok(value_str) = value.to_str() {
                http_request_builder = http_request_builder.header(name, value_str);
            }
        }

        // Build http::Request with empty body for signing (body will be added after)
        let mut http_request = http_request_builder
            .body(String::new())
            .into_alien_error()
            .context(ErrorData::RequestSignError {
                message: format!(
                    "Unable to construct http::Request for {} service",
                    config.service_name
                ),
            })?;

        // Prepare signing parameters.
        let identity = config.credentials.clone().into();
        let signing_region = config
            .signing_region
            .as_ref()
            .map(String::as_str)
            .unwrap_or(&config.region);

        let signing_time = get_current_time();

        trace!(
            signing_region = %signing_region,
            service = %config.service_name,
            "Signing parameters prepared"
        );

        let signing_settings = SigningSettings::default();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(signing_region)
            .name(&config.service_name)
            .time(signing_time)
            .settings(signing_settings)
            .build()
            .into_alien_error()
            .context(ErrorData::RequestSignError {
                message: format!(
                    "Invalid signing parameters for {} service in region {}",
                    config.service_name, signing_region
                ),
            })?
            .into();

        // Construct a SignableRequest with the actual body bytes for signature calculation.
        let signable_request = SignableRequest::new(
            http_request.method().as_str(),
            http_request.uri().to_string(),
            http_request
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str(), v.to_str().unwrap_or(""))),
            SignableBody::Bytes(&body_bytes),
        )
        .into_alien_error()
        .context(ErrorData::RequestSignError {
            message: format!(
                "Unable to create signable request for {} service",
                config.service_name
            ),
        })?;

        let (signing_instructions, signature) = sign(signable_request, &signing_params)
            .into_alien_error()
            .context(ErrorData::RequestSignError {
                message: format!(
                    "SigV4 signature generation error for {} service",
                    config.service_name
                ),
            })?
            .into_parts();

        trace!(signature = %signature, "SigV4 signature generated");

        // Apply the generated headers to our http::Request.
        signing_instructions.apply_to_request_http1x(&mut http_request);

        // Now rebuild the reqwest request with the original body bytes preserved
        let (parts, _) = http_request.into_parts();
        let mut signed_reqwest_request = client
            .request(parts.method, parts.uri.to_string())
            .body(body_bytes);

        // Copy all headers from the signed request
        for (name, value) in parts.headers {
            if let Some(name) = name {
                signed_reqwest_request = signed_reqwest_request.header(name, value);
            }
        }

        let signed_reqwest_request = signed_reqwest_request.build().into_alien_error().context(
            ErrorData::RequestSignError {
                message: format!(
                    "Unable to build final signed request for {} service",
                    config.service_name
                ),
            },
        )?;

        debug!(
            service = %config.service_name,
            url = %signed_reqwest_request.url(),
            "AWS request signed"
        );

        // Recreate a RequestBuilder from the client & signed request so the caller can
        // continue chaining.
        let signed_builder = reqwest::RequestBuilder::from_parts(client, signed_reqwest_request);

        Ok(signed_builder)
    }
}

// ---- New helper extension methods for convenient AWS header manipulation ----

/// Additional convenience methods for building AWS requests with `reqwest::RequestBuilder`.
/// These helpers keep the call sites concise and readable while ensuring that the
/// signing logic in [`AwsRequestSigner`] still sees exactly the headers that will
/// be sent over the wire.
pub trait AwsRequestBuilderExt {
    /// Add/override the `Host` header.
    fn host(self, host: &str) -> Self;

    /// Set `Content-Type: application/json`.
    fn content_type_json(self) -> Self;

    /// Set `Content-Type: application/xml`.
    fn content_type_xml(self) -> Self;

    /// Set `Content-Type: application/x-www-form-urlencoded`.
    fn content_type_form(self) -> Self;

    /// Compute the SHA-256 hash of `body` (hex-encoded) and store it in the
    /// `x-amz-content-sha256` header – required by some S3 operations.
    fn content_sha256(self, body: &str) -> Self;

    /// Compute the SHA-256 hash of binary `body` (hex-encoded) and store it in the
    /// `x-amz-content-sha256` header – required for binary payloads like Lambda invocations.
    fn content_sha256_bytes(self, body: &[u8]) -> Self;

    /// Set `Content-Type: application/x-amz-json-1.1` for AWS JSON RPC services
    /// (KMS, CloudWatch Logs, Secrets Manager, ACM, etc.).
    fn content_type_amz_json(self) -> Self;
}

impl AwsRequestBuilderExt for reqwest::RequestBuilder {
    fn host(self, host: &str) -> Self {
        self.header("host", host)
    }

    fn content_type_json(self) -> Self {
        self.header("content-type", "application/json")
    }

    fn content_type_amz_json(self) -> Self {
        self.header("content-type", "application/x-amz-json-1.1")
    }

    fn content_type_xml(self) -> Self {
        self.header("content-type", "application/xml")
    }

    fn content_type_form(self) -> Self {
        self.header("content-type", "application/x-www-form-urlencoded")
    }

    fn content_sha256(self, body: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        let digest = hasher.finalize();
        let hex_digest = hex::encode(digest);
        self.header("x-amz-content-sha256", hex_digest)
    }

    fn content_sha256_bytes(self, body: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(body);
        let digest = hasher.finalize();
        let hex_digest = hex::encode(digest);
        self.header("x-amz-content-sha256", hex_digest)
    }
}

// ---- End helper extension methods ----

// =============================================================================================
// Generic helpers: sign + retry + send (JSON / XML / no-body)
// These avoid duplicating the same sequence in every AWS service client.
// =============================================================================================

/// Sign the request, apply our retry policy and deserialize a JSON response into `T`.
pub async fn sign_send_json<T: DeserializeOwned + Send + 'static>(
    builder: RequestBuilder,
    config: &AwsSignConfig,
) -> Result<T> {
    builder
        .sign_aws_request(config)?
        .with_retry()
        .send_json::<T>()
        .await
}

/// Sign, retry and deserialize an XML response into `T`.
pub async fn sign_send_xml<T: DeserializeOwned + Send + 'static>(
    builder: RequestBuilder,
    config: &AwsSignConfig,
) -> Result<T> {
    builder
        .sign_aws_request(config)?
        .with_retry()
        .send_xml::<T>()
        .await
}

/// Sign the request, retry, and expect no body (return `()` on HTTP success).
pub async fn sign_send_no_response(builder: RequestBuilder, config: &AwsSignConfig) -> Result<()> {
    builder
        .sign_aws_request(config)?
        .with_retry()
        .send_no_response()
        .await
}
