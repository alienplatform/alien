use crate::kubernetes::ResolvedKubernetesConfig;
use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use backon::{ExponentialBuilder, Retryable};
use reqwest::RequestBuilder;
use serde::de::DeserializeOwned;
use std::time::Duration;

use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
use tokio::task::spawn_local;

/// Maps Kubernetes API HTTP errors to appropriate ErrorData variants
pub fn map_kubernetes_error(
    http_status: u16,
    response_text: &str,
    url: &str,
    request_body: Option<String>,
) -> ErrorData {
    // Try to extract resource information from Kubernetes Status response
    let (resource_type, resource_name) = extract_kubernetes_resource_info(response_text, url);

    match http_status {
        404 => ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
        },
        409 => ErrorData::RemoteResourceConflict {
            message: format!("Kubernetes resource conflict: {}", response_text),
            resource_type,
            resource_name,
        },
        403 | 401 => ErrorData::RemoteAccessDenied {
            resource_type,
            resource_name,
        },
        400 => ErrorData::InvalidInput {
            message: format!("Invalid Kubernetes API request: {}", response_text),
            field_name: None,
        },
        429 => ErrorData::RateLimitExceeded {
            message: format!("Kubernetes API rate limit exceeded: {}", response_text),
        },
        503 | 502 | 500 => ErrorData::RemoteServiceUnavailable {
            message: format!("Kubernetes API service unavailable: {}", response_text),
        },
        _ => ErrorData::HttpResponseError {
            message: format!(
                "Kubernetes API request failed with HTTP {}: {}",
                http_status, response_text
            ),
            url: url.to_string(),
            http_status,
            http_request_text: request_body,
            http_response_text: Some(response_text.to_string()),
        },
    }
}

/// Extracts resource type and name from Kubernetes error responses or URL
fn extract_kubernetes_resource_info(response_text: &str, url: &str) -> (String, String) {
    use serde_json::Value;

    // Try to parse Kubernetes Status response to extract resource info
    if let Ok(status) = serde_json::from_str::<Value>(response_text) {
        if let (Some(details), Some(message)) = (status.get("details"), status.get("message")) {
            let resource_type = details
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let resource_name = details
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    // Extract from message if name is not in details
                    message.as_str().and_then(|msg| {
                        if let Some(start) = msg.find('"') {
                            if let Some(end) = msg[start + 1..].find('"') {
                                return Some(&msg[start + 1..start + 1 + end]);
                            }
                        }
                        None
                    })
                })
                .unwrap_or("unknown")
                .to_string();
            return (resource_type, resource_name);
        }
    }

    // Fallback: extract from URL path segments
    let url_parts: Vec<&str> = url.split('/').collect();
    let resource_type = url_parts
        .iter()
        .rev()
        .nth(1)
        .unwrap_or(&"unknown")
        .to_string();
    let resource_name = url_parts.last().unwrap_or(&"unknown").to_string();

    (resource_type, resource_name)
}

/// Configuration needed to authenticate Kubernetes requests
#[derive(Debug, Clone)]
pub struct KubernetesAuthConfig {
    /// Bearer token for authentication
    pub bearer_token: Option<String>,
    /// Client certificate data (base64 encoded)
    pub client_certificate_data: Option<String>,
    /// Client key data (base64 encoded)
    pub client_key_data: Option<String>,
    /// Certificate authority data (base64 encoded)
    pub certificate_authority_data: Option<String>,
    /// Whether to skip TLS verification
    pub insecure_skip_tls_verify: bool,
    /// Additional headers to include in requests
    pub additional_headers: HashMap<String, String>,
}

impl From<&ResolvedKubernetesConfig> for KubernetesAuthConfig {
    fn from(config: &ResolvedKubernetesConfig) -> Self {
        Self {
            bearer_token: config.bearer_token.clone(),
            client_certificate_data: config.client_certificate_data.clone(),
            client_key_data: config.client_key_data.clone(),
            certificate_authority_data: config.certificate_authority_data.clone(),
            insecure_skip_tls_verify: config.insecure_skip_tls_verify,
            additional_headers: config.additional_headers.clone(),
        }
    }
}

/// Extension trait that enables Kubernetes authentication on request builders
pub trait KubernetesRequestSigner: Sized {
    /// Authenticate the request and return a new builder containing the authenticated request
    fn sign_kubernetes_request(self, config: &KubernetesAuthConfig) -> Result<Self>;
}

impl KubernetesRequestSigner for reqwest::RequestBuilder {
    fn sign_kubernetes_request(self, config: &KubernetesAuthConfig) -> Result<Self> {
        let mut builder = self;

        // Add bearer token if provided
        if let Some(ref token) = config.bearer_token {
            builder = builder.bearer_auth(token);
        }

        // Add additional headers
        for (key, value) in &config.additional_headers {
            builder = builder.header(key, value);
        }

        // Note: Client certificate authentication would require configuring the HTTP client
        // with certificates, which is more complex and would need to be done at client creation time
        // For now, we'll rely on bearer token authentication

        Ok(builder)
    }
}

/// Extension trait for `reqwest::RequestBuilder` to add Kubernetes-specific response handling
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait KubernetesRequestBuilderExt {
    /// Enable retries with an exponential back-off strategy.
    fn with_retry(self) -> RetriableKubernetesRequestBuilder;

    /// Send the request and parse the response as JSON
    async fn send_json<T: DeserializeOwned + 'static>(self) -> Result<T>;

    /// Send the request without parsing the response body
    async fn send_no_response(self) -> Result<()>;
}

/// A `reqwest::RequestBuilder` wrapper that automatically retries failed
/// requests using an exponential back-off strategy
pub struct RetriableKubernetesRequestBuilder {
    inner: reqwest::RequestBuilder,
    backoff: ExponentialBuilder,
}

impl RetriableKubernetesRequestBuilder {
    /// Overrides the default back-off settings.
    pub fn backoff(mut self, backoff: ExponentialBuilder) -> Self {
        self.backoff = backoff;
        self
    }

    /// Determine if a given error is retry-able using the retryable field.
    fn is_retryable_error(e: &AlienError<ErrorData>) -> bool {
        e.retryable
    }

    /// Creates a default exponential back-off (max 3 attempts, up to 20s).
    fn default_backoff() -> ExponentialBuilder {
        ExponentialBuilder::default()
            .with_max_times(3)
            .with_max_delay(Duration::from_secs(20))
            .with_jitter()
    }

    /// Execute the request, applying retries, and parse the body as JSON.
    pub async fn send_json<T: DeserializeOwned + Send + 'static>(self) -> Result<T> {
        let backoff = self.backoff;
        let builder = self.inner;

        let retryable = move || {
            let attempt_builder = builder.try_clone();
            async move {
                let attempt_builder = attempt_builder.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: "Request retry preparation failed".into(),
                    })
                })?;

                #[cfg(target_arch = "wasm32")]
                {
                    let resp = spawn_local(async move { attempt_builder.send().await })
                        .await
                        .map_err(|e| {
                            AlienError::new(ErrorData::GenericError {
                                message: format!("WASM task join failed: {}", e),
                            })
                        })?
                        .into_alien_error()
                        .context(ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        })?;
                    handle_kubernetes_json_response(resp).await
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let resp = attempt_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )?;
                    handle_kubernetes_json_response(resp).await
                }
            }
        };

        #[cfg(target_arch = "wasm32")]
        {
            spawn_local(async move {
                retryable
                    .retry(backoff)
                    .when(Self::is_retryable_error)
                    .await
            })
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::GenericError {
                    message: e.to_string(),
                })
            })?
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            retryable
                .retry(backoff)
                .when(Self::is_retryable_error)
                .await
        }
    }

    /// Execute the request, applying retries, without parsing the response body.
    pub async fn send_no_response(self) -> Result<()> {
        let backoff = self.backoff;
        let builder = self.inner;

        let retryable = move || {
            let attempt_builder = builder.try_clone();
            async move {
                let attempt_builder = attempt_builder.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: "Request retry preparation failed".into(),
                    })
                })?;

                #[cfg(target_arch = "wasm32")]
                {
                    let resp = spawn_local(async move { attempt_builder.send().await })
                        .await
                        .map_err(|e| {
                            AlienError::new(ErrorData::GenericError {
                                message: format!("WASM task join failed: {}", e),
                            })
                        })?
                        .into_alien_error()
                        .context(ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        })?;
                    handle_kubernetes_no_response(resp).await
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let resp = attempt_builder.send().await.into_alien_error().context(
                        ErrorData::HttpRequestFailed {
                            message: "Network error during HTTP request".to_string(),
                        },
                    )?;
                    handle_kubernetes_no_response(resp).await
                }
            }
        };

        #[cfg(target_arch = "wasm32")]
        {
            spawn_local(async move {
                retryable
                    .retry(backoff)
                    .when(Self::is_retryable_error)
                    .await
            })
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::GenericError {
                    message: e.to_string(),
                })
            })?
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            retryable
                .retry(backoff)
                .when(Self::is_retryable_error)
                .await
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl KubernetesRequestBuilderExt for reqwest::RequestBuilder {
    fn with_retry(self) -> RetriableKubernetesRequestBuilder {
        RetriableKubernetesRequestBuilder {
            inner: self,
            backoff: RetriableKubernetesRequestBuilder::default_backoff(),
        }
    }

    async fn send_json<T: DeserializeOwned + 'static>(self) -> Result<T> {
        #[cfg(target_arch = "wasm32")]
        {
            let resp = spawn_local(async move { self.send().await })
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("WASM task join failed: {}", e),
                    })
                })?
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Network error during HTTP request".to_string(),
                })?;
            handle_kubernetes_json_response(resp).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let resp =
                self.send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Network error during HTTP request".to_string(),
                    })?;
            handle_kubernetes_json_response(resp).await
        }
    }

    async fn send_no_response(self) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            let resp = spawn_local(async move { self.send().await })
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("WASM task join failed: {}", e),
                    })
                })?
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Network error during HTTP request".to_string(),
                })?;
            handle_kubernetes_no_response(resp).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let resp =
                self.send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        message: "Network error during HTTP request".to_string(),
                    })?;
            handle_kubernetes_no_response(resp).await
        }
    }
}

/// Handle a Kubernetes API response by checking status and parsing JSON on success
async fn handle_kubernetes_json_response<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T> {
    let status = response.status();
    let url = response.url().to_string();
    let response_text =
        response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Failed to read response body".to_string(),
            })?;

    if !status.is_success() {
        return Err(AlienError::new(map_kubernetes_error(
            status.as_u16(),
            &response_text,
            &url,
            None,
        )));
    }

    // Parse the JSON response using serde_path_to_error for better error messages
    let jd = &mut serde_json::Deserializer::from_str(&response_text);
    let parsed_response: T = serde_path_to_error::deserialize(jd).map_err(|err| {
        AlienError::new(ErrorData::HttpResponseError {
            message: format!(
                "Invalid JSON response at field '{}': {}",
                err.path(),
                err.inner()
            ),
            url,
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(response_text),
        })
    })?;

    Ok(parsed_response)
}

/// Handle a Kubernetes API response by checking status without parsing the body
async fn handle_kubernetes_no_response(response: reqwest::Response) -> Result<()> {
    let status = response.status();
    let url = response.url().to_string();

    if !status.is_success() {
        let response_text =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Failed to read error response body".to_string(),
                })?;
        return Err(AlienError::new(map_kubernetes_error(
            status.as_u16(),
            &response_text,
            &url,
            None,
        )));
    }

    Ok(())
}

/// Generic helpers: sign + retry + send (JSON / no-body)
/// These avoid duplicating the same sequence in every Kubernetes service client.

/// Sign the request, apply our retry policy and deserialize a JSON response into `T`.
pub async fn sign_send_json<T: DeserializeOwned + Send + 'static>(
    builder: RequestBuilder,
    config: &KubernetesAuthConfig,
) -> Result<T> {
    builder
        .sign_kubernetes_request(config)?
        .with_retry()
        .send_json::<T>()
        .await
}

/// Sign the request, retry, and expect no body (return `()` on HTTP success).
pub async fn sign_send_no_response(
    builder: RequestBuilder,
    config: &KubernetesAuthConfig,
) -> Result<()> {
    builder
        .sign_kubernetes_request(config)?
        .with_retry()
        .send_no_response()
        .await
}
