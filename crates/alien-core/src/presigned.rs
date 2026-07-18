use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// A presigned request that can be serialized, stored, and executed later.
/// Hides implementation details for different storage backends.
#[derive(Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct PresignedRequest {
    /// The storage backend this request targets
    pub backend: PresignedRequestBackend,
    /// When this presigned request expires
    pub expiration: DateTime<Utc>,
    /// The operation this request performs
    pub operation: PresignedOperation,
    /// The path this request operates on
    pub path: String,
}

/// Storage backend representation for different presigned request types
#[derive(Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum PresignedRequestBackend {
    /// HTTP-based request (AWS S3, GCP GCS, Azure Blob)
    #[serde(rename_all = "camelCase")]
    Http {
        url: String,
        method: String,
        headers: HashMap<String, String>,
    },
    /// Local filesystem operation
    #[serde(rename_all = "camelCase")]
    Local {
        file_path: String,
        operation: LocalOperation,
    },
}

/// The type of operation a presigned request performs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum PresignedOperation {
    /// Upload/put operation
    Put,
    /// Download/get operation  
    Get,
    /// Delete operation
    Delete,
}

/// Local filesystem operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum LocalOperation {
    Put,
    Get,
    Delete,
}

/// Response from executing a presigned request
#[derive(Debug)]
pub struct PresignedResponse {
    /// HTTP status code (200, 404, etc.) or equivalent
    pub status_code: u16,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body (for GET operations)
    pub body: Option<Bytes>,
}

/// Remove credentials from a URL before storing it in an error or log record.
///
/// Presigned storage URLs and command response URLs carry bearer-equivalent
/// query values. User info, query parameters, and fragments are therefore
/// never safe diagnostic context; the origin and path are enough to identify
/// the failed endpoint.
pub fn redact_url_for_error(raw: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(raw) {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.set_query(None);
        parsed.set_fragment(None);
        return parsed.to_string();
    }

    if raw.starts_with('/') && !raw.starts_with("//") {
        return raw
            .split(['?', '#'])
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or("<invalid-url>")
            .to_string();
    }

    "<invalid-url>".to_string()
}

impl PresignedRequest {
    /// Create a new HTTP-based presigned request
    pub fn new_http(
        url: String,
        method: String,
        headers: HashMap<String, String>,
        operation: PresignedOperation,
        path: String,
        expiration: DateTime<Utc>,
    ) -> Self {
        Self {
            backend: PresignedRequestBackend::Http {
                url,
                method,
                headers,
            },
            expiration,
            operation,
            path,
        }
    }

    /// Create a new local filesystem presigned request
    pub fn new_local(
        file_path: String,
        operation: PresignedOperation,
        path: String,
        expiration: DateTime<Utc>,
    ) -> Self {
        let local_op = match operation {
            PresignedOperation::Put => LocalOperation::Put,
            PresignedOperation::Get => LocalOperation::Get,
            PresignedOperation::Delete => LocalOperation::Delete,
        };

        Self {
            backend: PresignedRequestBackend::Local {
                file_path,
                operation: local_op,
            },
            expiration,
            operation,
            path,
        }
    }

    /// Execute this presigned request with optional body data.
    /// For PUT operations, body should contain the data to upload.
    /// For GET/DELETE operations, body is typically None.
    pub async fn execute(&self, body: Option<Bytes>) -> Result<PresignedResponse> {
        let client = reqwest::Client::new();
        self.execute_with_client(&client, body).await
    }

    /// Execute this presigned request with a caller-owned HTTP client.
    ///
    /// Multi-step protocols should use this form so retries and adjacent HTTP
    /// operations share one connection pool. Local requests ignore the client.
    pub async fn execute_with_client(
        &self,
        client: &reqwest::Client,
        body: Option<Bytes>,
    ) -> Result<PresignedResponse> {
        match &self.backend {
            PresignedRequestBackend::Http {
                url,
                method,
                headers,
            } => self.execute_http(client, url, method, headers, body).await,
            PresignedRequestBackend::Local {
                file_path,
                operation,
            } => {
                #[cfg(feature = "local")]
                {
                    self.execute_local(file_path, *operation, body).await
                }
                #[cfg(not(feature = "local"))]
                {
                    let _ = (file_path, operation);
                    Err(AlienError::new(ErrorData::FeatureNotEnabled {
                        feature: "local".to_string(),
                    }))
                }
            }
        }
    }

    /// Get a URL representation of this presigned request.
    /// For local storage, returns a local:// URL.
    /// For cloud storage, returns the actual presigned URL.
    pub fn url(&self) -> String {
        match &self.backend {
            PresignedRequestBackend::Http { url, .. } => url.clone(),
            PresignedRequestBackend::Local { file_path, .. } => {
                format!("local://{}", file_path)
            }
        }
    }

    /// Check if this presigned request has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expiration
    }

    /// Get the HTTP method for this request (PUT, GET, DELETE)
    pub fn method(&self) -> &str {
        match &self.backend {
            PresignedRequestBackend::Http { method, .. } => method,
            PresignedRequestBackend::Local { operation, .. } => match operation {
                LocalOperation::Put => "PUT",
                LocalOperation::Get => "GET",
                LocalOperation::Delete => "DELETE",
            },
        }
    }

    /// Get any headers that should be included with this request
    pub fn headers(&self) -> HashMap<String, String> {
        match &self.backend {
            PresignedRequestBackend::Http { headers, .. } => headers.clone(),
            _ => HashMap::new(),
        }
    }

    async fn execute_http(
        &self,
        client: &reqwest::Client,
        url: &str,
        method: &str,
        headers: &HashMap<String, String>,
        body: Option<Bytes>,
    ) -> Result<PresignedResponse> {
        if self.is_expired() {
            return Err(AlienError::new(ErrorData::PresignedRequestExpired {
                path: self.path.clone(),
                expired_at: self.expiration,
            }));
        }

        let mut request = match method {
            "PUT" => client.put(url),
            "GET" => client.get(url),
            "DELETE" => client.delete(url),
            _ => {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: format!("HTTP method: {}", method),
                    reason: "Only PUT, GET, and DELETE are supported".to_string(),
                }))
            }
        };

        // Add headers
        for (key, value) in headers {
            request = request.header(key, value);
        }

        // Add body for PUT requests
        if let Some(data) = body {
            request = request.body(data);
        }

        let safe_url = redact_url_for_error(url);
        let response = request
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: safe_url.clone(),
                method: method.to_string(),
            })?;

        let status_code = response.status().as_u16();
        let response_headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let response_body = if matches!(self.operation, PresignedOperation::Get) {
            Some(
                response
                    .bytes()
                    .await
                    .map_err(reqwest::Error::without_url)
                    .into_alien_error()
                    .context(ErrorData::HttpRequestFailed {
                        url: safe_url,
                        method: method.to_string(),
                    })?,
            )
        } else {
            None
        };

        Ok(PresignedResponse {
            status_code,
            headers: response_headers,
            body: response_body,
        })
    }

    #[cfg(feature = "local")]
    async fn execute_local(
        &self,
        file_path: &str,
        operation: LocalOperation,
        body: Option<Bytes>,
    ) -> Result<PresignedResponse> {
        use std::path::Path as StdPath;
        use tokio::fs;

        if self.is_expired() {
            return Err(AlienError::new(ErrorData::PresignedRequestExpired {
                path: self.path.clone(),
                expired_at: self.expiration,
            }));
        }

        let path = StdPath::new(file_path);

        match operation {
            LocalOperation::Put => {
                let data = body.ok_or_else(|| {
                    AlienError::new(ErrorData::OperationNotSupported {
                        operation: "Local PUT without body".to_string(),
                        reason: "PUT operations require body data".to_string(),
                    })
                })?;

                // Create parent directories if needed
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .await
                        .into_alien_error()
                        .context(ErrorData::LocalFilesystemError {
                            path: file_path.to_string(),
                            operation: "create_parent_dirs".to_string(),
                        })?;
                }

                let write_result: std::io::Result<()> = fs::write(path, data.as_ref()).await;
                write_result
                    .into_alien_error()
                    .context(ErrorData::LocalFilesystemError {
                        path: file_path.to_string(),
                        operation: "write".to_string(),
                    })?;

                Ok(PresignedResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: None,
                })
            }
            LocalOperation::Get => {
                let data = fs::read(path).await.into_alien_error().context(
                    ErrorData::LocalFilesystemError {
                        path: file_path.to_string(),
                        operation: "read".to_string(),
                    },
                )?;

                Ok(PresignedResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: Some(Bytes::from(data)),
                })
            }
            LocalOperation::Delete => {
                fs::remove_file(path).await.into_alien_error().context(
                    ErrorData::LocalFilesystemError {
                        path: file_path.to_string(),
                        operation: "delete".to_string(),
                    },
                )?;

                Ok(PresignedResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::redact_url_for_error;

    #[test]
    fn redacts_query_fragment_and_user_info_from_diagnostic_urls() {
        let secret = "do-not-log-this-token";
        let sanitized = redact_url_for_error(&format!(
            "https://user:{secret}@storage.example.com/object?X-Amz-Signature={secret}#fragment"
        ));

        assert_eq!(sanitized, "https://storage.example.com/object");
        assert!(!sanitized.contains(secret));
    }

    #[test]
    fn redacts_query_from_relative_urls() {
        assert_eq!(
            redact_url_for_error("/v1/commands/cmd/response?response_token=secret"),
            "/v1/commands/cmd/response"
        );
    }

    #[test]
    fn does_not_echo_unparseable_urls() {
        let secret = "do-not-log-this-token";
        assert_eq!(
            redact_url_for_error(&format!("not a URL containing {secret}")),
            "<invalid-url>"
        );
    }
}
