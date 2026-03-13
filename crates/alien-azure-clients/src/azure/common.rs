// -----------------------------------------------------------------------------
// Generic helpers and base client for Azure Storage REST APIs. This **does not**
// contain service-specific logic – those live in their own modules (e.g. `abs`).
// -----------------------------------------------------------------------------

use crate::azure::{AzureClientConfig, AzureClientConfigExt};
use alien_client_core::{Error, ErrorData, Result};

use crate::azure::long_running_operation::{LongRunningOperation, OperationResult};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use backon::{ExponentialBuilder, Retryable};
use chrono::Utc;
use http::{header::HeaderName, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;

// -----------------------------------------------------------------------------
// Client-base for Azure Storage using bearer token authentication
// -----------------------------------------------------------------------------

/// A thin wrapper that handles bearer token auth, retry/back-off and simple response
/// parsing (XML or JSON). Each storage service (Blob, Queue, …) composes this
/// with its own higher-level helpers.
#[derive(Debug)]
pub struct AzureClientBase {
    pub client: Client,
    /// REST API version sent in the `x-ms-version` header.
    pub api_version: String,
    /// Fully-qualified base endpoint – e.g. "https://myacct.blob.core.windows.net".
    pub endpoint: String,
    /// Platform configuration for endpoint overrides and other settings
    pub client_config: Option<AzureClientConfig>,
}

impl AzureClientBase {
    pub fn new(client: Client, endpoint: String) -> Self {
        Self {
            client,
            api_version: "2023-11-03".into(),
            endpoint,
            client_config: None,
        }
    }

    /// Create a new AzureClientBase with platform config for endpoint override support
    pub fn with_client_config(
        client: Client,
        endpoint: String,
        client_config: AzureClientConfig,
    ) -> Self {
        Self {
            client,
            api_version: "2023-11-03".into(),
            endpoint,
            client_config: Some(client_config),
        }
    }

    /// Get the endpoint for a specific service, checking for overrides first
    pub fn get_service_endpoint(&self, service_name: &str, default_endpoint: &str) -> String {
        if let Some(config) = &self.client_config {
            if let Some(override_endpoint) = config.get_service_endpoint(service_name) {
                return override_endpoint.to_string();
            }
        }
        default_endpoint.to_string()
    }

    // ------------- Retry helpers -------------

    fn create_backoff() -> ExponentialBuilder {
        ExponentialBuilder::default()
            .with_max_times(3)
            .with_max_delay(Duration::from_secs(20))
            .with_jitter()
    }

    fn is_retryable_error(e: &AlienError<ErrorData>) -> bool {
        e.retryable
    }

    /// Executes a retryable operation with exponential backoff.
    #[cfg(target_arch = "wasm32")]
    pub async fn with_retry<F, Fut, T>(&self, retryable: F) -> Result<T>
    where
        F: Fn() -> Fut + 'static,
        Fut: std::future::Future<Output = Result<T>> + 'static,
        T: 'static,
    {
        let backoff = Self::create_backoff();
        use tokio::task::spawn_local;
        spawn_local(async move {
            retryable
                .retry(backoff)
                .when(Self::is_retryable_error)
                .await
        })
        .await
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: "WASM task join failed".to_string(),
        })?
    }

    /// Executes a retryable operation with exponential backoff.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn with_retry<F, Fut, T>(&self, retryable: F) -> Result<T>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let backoff = Self::create_backoff();
        retryable
            .retry(backoff)
            .when(Self::is_retryable_error)
            .await
    }

    // ------------- Bearer token authentication -------------

    pub async fn sign_request(
        &self,
        mut req: http::Request<String>,
        bearer_token: &str,
    ) -> Result<reqwest::Request> {
        // Inject mandatory headers if absent.
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let body_len = req.body().len().to_string();
        {
            let h = req.headers_mut();
            if !h.contains_key("x-ms-date") {
                h.insert(
                    HeaderName::from_static("x-ms-date"),
                    HeaderValue::from_str(&date).unwrap(),
                );
            }
            if !h.contains_key("x-ms-version") {
                h.insert(
                    HeaderName::from_static("x-ms-version"),
                    HeaderValue::from_str(&self.api_version).unwrap(),
                );
            }
            if !h.contains_key("content-length") {
                h.insert(
                    HeaderName::from_static("content-length"),
                    HeaderValue::from_str(&body_len).unwrap(),
                );
            }
            // Add Authorization header with bearer token
            let auth_value = format!("Bearer {}", bearer_token);
            h.insert(
                HeaderName::from_static("authorization"),
                HeaderValue::from_str(&auth_value).unwrap(),
            );
        }

        req.try_into()
            .into_alien_error()
            .context(ErrorData::RequestSignError {
                message: "Failed to convert HTTP request for Azure Bearer Token authentication"
                    .to_string(),
            })
    }

    // ------------- URL builder -------------

    pub fn build_url(&self, path: &str, query: Option<Vec<(&str, String)>>) -> String {
        let mut url = format!("{}{}", self.endpoint.trim_end_matches('/'), path);
        if let Some(qs) = query {
            if !qs.is_empty() {
                url.push('?');
                url.push_str(
                    &qs.into_iter()
                        .map(|(k, v)| format!("{k}={}", urlencoding::encode(&v)))
                        .collect::<Vec<_>>()
                        .join("&"),
                );
            }
        }
        url
    }

    // ------------- Low-level executor -------------

    /// Executes an HTTP request with retry logic and returns the response if successful.
    #[cfg(target_arch = "wasm32")]
    pub async fn execute_request(
        &self,
        req: reqwest::Request,
        op: &str,
        res_name: &str,
    ) -> Result<reqwest::Response> {
        let op = op.to_string();
        let res_name = res_name.to_string();
        let client = self.client.clone();
        let retryable = move || {
            let req_clone = req.try_clone();
            let client = client.clone();
            let op = op.clone();
            let res_name = res_name.clone();
            async move {
                let req_clone = req_clone.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("Azure {}: cannot clone req", op),
                    })
                })?;

                // Capture request details before execution consumes the request
                let request_url = req_clone.url().to_string();
                let request_body = req_clone
                    .body()
                    .and_then(|b| b.as_bytes())
                    .map(|b| String::from_utf8_lossy(b).to_string());

                let resp = client.execute(req_clone).await.into_alien_error().context(
                    ErrorData::HttpRequestFailed {
                        message: format!("Azure {}: HTTP error for {}", op, res_name),
                    },
                )?;
                let status = resp.status();
                if status.is_success()
                    || status == StatusCode::CREATED
                    || status == StatusCode::ACCEPTED
                {
                    Ok(resp)
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    Err(create_azure_http_error_with_context(
                        status,
                        &op,
                        "Resource",
                        &res_name,
                        &body,
                        &request_url,
                        request_body,
                    ))
                }
            }
        };
        self.with_retry(retryable).await
    }

    /// Executes an HTTP request with retry logic and returns the response if successful.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn execute_request(
        &self,
        req: reqwest::Request,
        op: &str,
        res_name: &str,
    ) -> Result<reqwest::Response> {
        let op = op.to_string();
        let res_name = res_name.to_string();
        let client = self.client.clone();
        let retryable = move || {
            let req_clone = req.try_clone();
            let client = client.clone();
            let op = op.clone();
            let res_name = res_name.clone();
            async move {
                let req_clone = req_clone.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("Azure {}: cannot clone req", op),
                    })
                })?;

                // Capture request details before execution consumes the request
                let request_url = req_clone.url().to_string();
                let request_body = req_clone
                    .body()
                    .and_then(|b| b.as_bytes())
                    .map(|b| String::from_utf8_lossy(b).to_string());

                let resp = client.execute(req_clone).await.into_alien_error().context(
                    ErrorData::HttpRequestFailed {
                        message: format!("Azure {}: HTTP error for {}", op, res_name),
                    },
                )?;
                let status = resp.status();
                if status.is_success()
                    || status == StatusCode::CREATED
                    || status == StatusCode::ACCEPTED
                {
                    Ok(resp)
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    Err(create_azure_http_error_with_context(
                        status,
                        &op,
                        "Resource",
                        &res_name,
                        &body,
                        &request_url,
                        request_body,
                    ))
                }
            }
        };
        self.with_retry(retryable).await
    }

    /// Executes an HTTP request with support for long-running operations.
    ///
    /// This method handles the common Azure pattern where operations can either:
    /// - Complete synchronously (200/204, or 201 without async headers) with the result in the response body or no body  
    /// - Start asynchronously (201/202 with Azure-AsyncOperation or Location headers)
    ///
    /// Returns an OperationResult that can be used to get the final result.
    #[cfg(target_arch = "wasm32")]
    pub async fn execute_request_with_long_running_support<T>(
        &self,
        req: reqwest::Request,
        op: &str,
        res_name: &str,
    ) -> Result<OperationResult<T>>
    where
        T: DeserializeOwned + 'static,
    {
        let op = op.to_string();
        let res_name = res_name.to_string();
        let client = self.client.clone();

        let retryable = move || {
            let req_clone = req.try_clone();
            let client = client.clone();
            let op = op.clone();
            let res_name = res_name.clone();
            async move {
                let req_clone = req_clone.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("Azure {}: cannot clone req", op),
                    })
                })?;

                // Capture request details before execution consumes the request
                let request_url = req_clone.url().to_string();
                let request_body = req_clone
                    .body()
                    .and_then(|b| b.as_bytes())
                    .map(|b| String::from_utf8_lossy(b).to_string());

                let resp = client.execute(req_clone).await.into_alien_error().context(
                    ErrorData::HttpRequestFailed {
                        message: format!("Azure {}: HTTP error for {}", op, res_name),
                    },
                )?;
                let status = resp.status();

                match status {
                    StatusCode::OK => {
                        // 200 OK - Operation completed synchronously with response body
                        let body = resp.text().await.into_alien_error().context(
                            ErrorData::HttpRequestFailed {
                                message: format!("Azure {}: failed to read response body", op),
                            },
                        )?;

                        let result: T = serde_json::from_str(&body).into_alien_error().context(
                            ErrorData::HttpResponseError {
                                message: format!("Azure {}: JSON parse error. Body: {}", op, body),
                                url: request_url.clone(),
                                http_status: 200,
                                http_response_text: Some(body.clone()),
                                http_request_text: request_body.clone(),
                            },
                        )?;

                        Ok(OperationResult::Completed(result))
                    }
                    StatusCode::CREATED => {
                        // 201 Created - Could be synchronous completion OR async operation
                        // Check for async operation headers first
                        use LongRunningOperation;
                        if let Some(long_running_op) =
                            LongRunningOperation::from_response_headers(&resp)?
                        {
                            // Has async headers - this is a long-running operation
                            Ok(OperationResult::LongRunning(long_running_op))
                        } else {
                            // No async headers - operation completed synchronously
                            let body = resp.text().await.into_alien_error().context(
                                ErrorData::HttpRequestFailed {
                                    message: format!("Azure {}: failed to read response body", op),
                                },
                            )?;

                            let result: T = serde_json::from_str(&body)
                                .into_alien_error()
                                .context(ErrorData::HttpResponseError {
                                    message: format!(
                                        "Azure {}: JSON parse error. Body: {}",
                                        op, body
                                    ),
                                    url: request_url.clone(),
                                    http_status: 201,
                                    http_response_text: Some(body.clone()),
                                    http_request_text: request_body.clone(),
                                })?;

                            Ok(OperationResult::Completed(result))
                        }
                    }
                    StatusCode::NO_CONTENT => {
                        // Operation completed synchronously with no response body (typically DELETE)
                        // For unit type (), we can deserialize from empty string
                        let result: T = serde_json::from_str("null").into_alien_error().context(
                            ErrorData::HttpResponseError {
                                message: format!(
                                "Azure {}: failed to deserialize unit type for NO_CONTENT response",
                                op
                            ),
                                url: request_url.clone(),
                                http_status: 204,
                                http_response_text: Some("null".to_string()),
                                http_request_text: request_body.clone(),
                            },
                        )?;

                        Ok(OperationResult::Completed(result))
                    }
                    StatusCode::ACCEPTED => {
                        // 202 Accepted - Operation is running asynchronously
                        use LongRunningOperation;
                        if let Some(long_running_op) =
                            LongRunningOperation::from_response_headers(&resp)?
                        {
                            Ok(OperationResult::LongRunning(long_running_op))
                        } else {
                            Err(AlienError::new(ErrorData::GenericError {
                                message: format!("Azure {}: got 202 Accepted but no long-running operation headers found", op),
                            }))
                        }
                    }
                    _ => {
                        let body = resp.text().await.unwrap_or_default();
                        Err(create_azure_http_error_with_context(
                            status,
                            &op,
                            "Resource",
                            &res_name,
                            &body,
                            &request_url,
                            request_body,
                        ))
                    }
                }
            }
        };

        self.with_retry(retryable).await
    }

    /// Executes an HTTP request with support for long-running operations.
    ///
    /// This method handles the common Azure pattern where operations can either:
    /// - Complete synchronously (200/204, or 201 without async headers) with the result in the response body or no body  
    /// - Start asynchronously (201/202 with Azure-AsyncOperation or Location headers)
    ///
    /// Returns an OperationResult that can be used to get the final result.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn execute_request_with_long_running_support<T>(
        &self,
        req: reqwest::Request,
        op: &str,
        res_name: &str,
    ) -> Result<OperationResult<T>>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let op = op.to_string();
        let res_name = res_name.to_string();
        let client = self.client.clone();

        let retryable = move || {
            let req_clone = req.try_clone();
            let client = client.clone();
            let op = op.clone();
            let res_name = res_name.clone();
            async move {
                let req_clone = req_clone.ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("Azure {}: cannot clone req", op),
                    })
                })?;

                // Capture request details before execution consumes the request
                let request_url = req_clone.url().to_string();
                let request_body = req_clone
                    .body()
                    .and_then(|b| b.as_bytes())
                    .map(|b| String::from_utf8_lossy(b).to_string());

                let resp = client.execute(req_clone).await.into_alien_error().context(
                    ErrorData::HttpRequestFailed {
                        message: format!("Azure {}: HTTP error for {}", op, res_name),
                    },
                )?;
                let status = resp.status();

                match status {
                    StatusCode::OK => {
                        // 200 OK - Operation completed synchronously with response body
                        let body = resp.text().await.into_alien_error().context(
                            ErrorData::HttpRequestFailed {
                                message: format!("Azure {}: failed to read response body", op),
                            },
                        )?;

                        let result: T = serde_json::from_str(&body).into_alien_error().context(
                            ErrorData::HttpResponseError {
                                message: format!("Azure {}: JSON parse error. Body: {}", op, body),
                                url: request_url.clone(),
                                http_status: 200,
                                http_response_text: Some(body.clone()),
                                http_request_text: request_body.clone(),
                            },
                        )?;

                        Ok(OperationResult::Completed(result))
                    }
                    StatusCode::CREATED => {
                        // 201 Created - Could be synchronous completion OR async operation
                        // Check for async operation headers first
                        if let Some(long_running_op) =
                            LongRunningOperation::from_response_headers(&resp)?
                        {
                            // Has async headers - this is a long-running operation
                            Ok(OperationResult::LongRunning(long_running_op))
                        } else {
                            // No async headers - operation completed synchronously
                            let body = resp.text().await.into_alien_error().context(
                                ErrorData::HttpRequestFailed {
                                    message: format!("Azure {}: failed to read response body", op),
                                },
                            )?;

                            let result: T = serde_json::from_str(&body)
                                .into_alien_error()
                                .context(ErrorData::HttpResponseError {
                                    message: format!(
                                        "Azure {}: JSON parse error. Body: {}",
                                        op, body
                                    ),
                                    url: request_url.clone(),
                                    http_status: 201,
                                    http_response_text: Some(body.clone()),
                                    http_request_text: request_body.clone(),
                                })?;

                            Ok(OperationResult::Completed(result))
                        }
                    }
                    StatusCode::NO_CONTENT => {
                        // Operation completed synchronously with no response body (typically DELETE)
                        // For unit type (), we can deserialize from empty string
                        let result: T = serde_json::from_str("null").into_alien_error().context(
                            ErrorData::HttpResponseError {
                                message: format!(
                                "Azure {}: failed to deserialize unit type for NO_CONTENT response",
                                op
                            ),
                                url: request_url.clone(),
                                http_status: 204,
                                http_response_text: Some("null".to_string()),
                                http_request_text: request_body.clone(),
                            },
                        )?;

                        Ok(OperationResult::Completed(result))
                    }
                    StatusCode::ACCEPTED => {
                        // 202 Accepted - Operation is running asynchronously
                        use LongRunningOperation;
                        if let Some(long_running_op) =
                            LongRunningOperation::from_response_headers(&resp)?
                        {
                            Ok(OperationResult::LongRunning(long_running_op))
                        } else {
                            Err(AlienError::new(ErrorData::GenericError {
                                message: format!("Azure {}: got 202 Accepted but no long-running operation headers found", op),
                            }))
                        }
                    }
                    _ => {
                        let body = resp.text().await.unwrap_or_default();
                        Err(create_azure_http_error_with_context(
                            status,
                            &op,
                            "Resource",
                            &res_name,
                            &body,
                            &request_url,
                            request_body,
                        ))
                    }
                }
            }
        };

        self.with_retry(retryable).await
    }
}

// -----------------------------------------------------------------------------
// Light request-builder (service-agnostic)
// -----------------------------------------------------------------------------

pub struct AzureRequestBuilder {
    method: Method,
    uri: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl AzureRequestBuilder {
    pub fn new(method: Method, uri: String) -> Self {
        Self {
            method,
            uri,
            headers: vec![],
            body: String::new(),
        }
    }
    pub fn header(mut self, name: &str, val: &str) -> Self {
        self.headers.push((name.into(), val.into()));
        self
    }
    pub fn x_ms_version(self, v: &str) -> Self {
        self.header("x-ms-version", v)
    }
    pub fn content_type_xml(self) -> Self {
        self.header("content-type", "application/xml")
    }
    pub fn content_type_json(self) -> Self {
        self.header("content-type", "application/json")
    }
    pub fn content_length(self, body: &str) -> Self {
        self.header("content-length", &body.len().to_string())
    }
    pub fn body(mut self, body: String) -> Self {
        self.body = body;
        self
    }
    pub fn build(self) -> Result<http::Request<String>> {
        let mut b = http::Request::builder().method(self.method).uri(&self.uri);
        for (k, v) in self.headers {
            b = b.header(&k, &v);
        }
        b.body(self.body)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: "AzureRequestBuilder build failed".to_string(),
            })
    }
}

// -----------------------------------------------------------------------------
// Azure HTTP error handling helpers
// -----------------------------------------------------------------------------

/// Creates an HttpResponseError with full HTTP details and adds appropriate service-specific context
pub fn create_azure_http_error_with_context(
    status: StatusCode,
    op: &str,
    res_type: &str,
    res_name: &str,
    body: &str,
    url: &str,
    request_body: Option<String>,
) -> Error {
    // First create the HttpResponseError with all HTTP details
    let http_error = AlienError::new(ErrorData::HttpResponseError {
        message: format!("Azure {op} failed for {res_type} '{res_name}': HTTP {status}"),
        url: url.to_string(),
        http_status: status.as_u16(),
        http_response_text: Some(body.to_string()),
        http_request_text: request_body,
    });

    // Then add service-specific context based on status code
    let service_context = match status {
        StatusCode::BAD_REQUEST => ErrorData::InvalidInput {
            message: format!("Bad request for {res_type} '{res_name}': {body}"),
            field_name: None,
        },
        StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
            message: format!("Resource conflict for {res_type} '{res_name}': {body}"),
            resource_type: res_type.into(),
            resource_name: res_name.into(),
        },
        StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
            resource_type: res_type.into(),
            resource_name: res_name.into(),
        },
        StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
            resource_type: res_type.into(),
            resource_name: res_name.into(),
        },
        StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded {
            message: format!("Rate limit exceeded for {res_type} '{res_name}': {body}"),
        },
        StatusCode::SERVICE_UNAVAILABLE | StatusCode::INTERNAL_SERVER_ERROR => {
            ErrorData::RemoteServiceUnavailable {
                message: format!("Service unavailable for {res_type} '{res_name}': {body}"),
            }
        }
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => ErrorData::Timeout {
            message: format!("Timeout for {res_type} '{res_name}': {body}"),
        },
        // 499 is a non-standard status code that indicates "Client Closed Request" - typically due to timeout
        status if status.as_u16() == 499 => ErrorData::Timeout {
            message: format!("Client closed request for {res_type} '{res_name}': {body}"),
        },
        _ => ErrorData::GenericError {
            message: format!("Unknown error for {res_type} '{res_name}': {body}"),
        },
    };

    // Add the service-specific context to the HTTP error
    http_error.context(service_context)
}

// -----------------------------------------------------------------------------
// Azure metadata validation utilities
// -----------------------------------------------------------------------------

/// Validates that a metadata key follows Azure's C# identifier naming rules.
///
/// According to Azure documentation, metadata names must adhere to C# identifier rules:
/// - Start with a letter or underscore
/// - Contain only letters, digits, and underscores
/// - Be 1-64 characters long
///
/// Returns `Ok(())` if valid, or `Err(Error)` with a descriptive message if invalid.
pub fn validate_azure_metadata_key(key: &str) -> Result<()> {
    let is_valid_csharp_identifier = key
        .chars()
        .next()
        .map_or(false, |c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !key.is_empty()
        && key.len() <= 64;

    if !is_valid_csharp_identifier {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: format!("Invalid metadata key '{}': must be a valid C# identifier (1-64 characters, start with letter/underscore, contain only letters/digits/underscores)", key),
            field_name: None,
        }));
    }

    Ok(())
}

/// Validates that a metadata value is a valid HTTP header value (no control characters).
pub fn validate_azure_metadata_value(key: &str, value: &str) -> Result<()> {
    if value.chars().any(|c| c.is_control()) {
        return Err(AlienError::new(ErrorData::InvalidInput {
            message: format!(
                "Invalid metadata value for key '{}': contains control characters",
                key
            ),
            field_name: None,
        }));
    }

    Ok(())
}
