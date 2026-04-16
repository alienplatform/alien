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
        match &self.backend {
            PresignedRequestBackend::Http {
                url,
                method,
                headers,
            } => self.execute_http(url, method, headers, body).await,
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

        let client = reqwest::Client::new();
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

        let response =
            request
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    url: url.to_string(),
                    method: method.to_string(),
                })?;

        let status_code = response.status().as_u16();
        let response_headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let response_body = if matches!(self.operation, PresignedOperation::Get) {
            Some(response.bytes().await.into_alien_error().context(
                ErrorData::HttpRequestFailed {
                    url: url.to_string(),
                    method: method.to_string(),
                },
            )?)
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
