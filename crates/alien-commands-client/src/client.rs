use std::time::Duration;

use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::debug;

use crate::error::CommandError;

/// Configuration for the commands client.
pub struct CommandsClientConfig {
    /// Command timeout (default: 60s)
    pub timeout: Duration,
    /// Polling interval (default: 500ms)
    pub poll_interval: Duration,
    /// Max polling interval (default: 5s)
    pub max_poll_interval: Duration,
    /// Backoff multiplier (default: 1.5)
    pub poll_backoff: f64,
    /// Allow local file:// storage backends (dev only)
    pub allow_local_storage: bool,
}

impl Default for CommandsClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            poll_interval: Duration::from_millis(500),
            max_poll_interval: Duration::from_secs(5),
            poll_backoff: 1.5,
            allow_local_storage: false,
        }
    }
}

/// Options for a single invoke call.
pub struct InvokeOptions {
    /// Override the default timeout for this invocation.
    pub timeout: Option<Duration>,
    /// Set a deadline for the command (server-side expiry).
    pub deadline: Option<DateTime<Utc>>,
    /// Idempotency key to prevent duplicate commands.
    pub idempotency_key: Option<String>,
}

/// High-level client for invoking commands on Alien deployments.
pub struct CommandsClient {
    manager_url: String,
    deployment_id: String,
    token: String,
    http_client: reqwest::Client,
    config: CommandsClientConfig,
}

// -- API response types (internal) --

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCommandResponse {
    command_id: String,
    state: String,
    next: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandStatusResponse {
    command_id: String,
    state: String,
    #[serde(default)]
    response: Option<CommandResponseBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandResponseBody {
    status: String,
    #[serde(default)]
    response: Option<BodySpecResponse>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BodySpecResponse {
    mode: String,
    #[serde(default)]
    inline_base64: Option<String>,
    #[serde(default)]
    storage_get_request: Option<StorageGetRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageGetRequest {
    backend: StorageBackend,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageBackend {
    #[serde(rename = "type")]
    backend_type: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default, rename = "filePath")]
    file_path: Option<String>,
}

impl CommandsClient {
    /// Create a new commands client with default config.
    pub fn new(manager_url: &str, deployment_id: &str, token: &str) -> Self {
        Self::with_config(
            manager_url,
            deployment_id,
            token,
            CommandsClientConfig::default(),
        )
    }

    /// Create a new commands client with custom config.
    pub fn with_config(
        manager_url: &str,
        deployment_id: &str,
        token: &str,
        config: CommandsClientConfig,
    ) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token))
                .expect("invalid token"),
        );

        let http_client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("failed to build HTTP client");

        Self {
            manager_url: manager_url.trim_end_matches('/').to_string(),
            deployment_id: deployment_id.to_string(),
            token: token.to_string(),
            http_client,
            config,
        }
    }

    /// Invoke a command and wait for the result.
    ///
    /// Sends params inline, polls for completion, and decodes the response.
    pub async fn invoke<P: Serialize, R: DeserializeOwned>(
        &self,
        command: &str,
        params: P,
    ) -> Result<R, CommandError> {
        self.invoke_with_options(command, params, None).await
    }

    /// Invoke a command with options and wait for the result.
    pub async fn invoke_with_options<P: Serialize, R: DeserializeOwned>(
        &self,
        command: &str,
        params: P,
        options: Option<InvokeOptions>,
    ) -> Result<R, CommandError> {
        let timeout = options
            .as_ref()
            .and_then(|o| o.timeout)
            .unwrap_or(self.config.timeout);

        // Step 1: Create the command (always inline — server handles storage)
        let command_id = self.create(command, params, options.as_ref()).await?;

        debug!(command_id = %command_id, command = %command, "Command created, polling for result");

        // Step 2: Poll for completion with exponential backoff
        let start = tokio::time::Instant::now();
        let mut interval = self.config.poll_interval;

        loop {
            if start.elapsed() > timeout {
                return Err(CommandError::Timeout {
                    command_id,
                    last_state: "polling".to_string(),
                });
            }

            tokio::time::sleep(interval).await;

            let status = self.get_status(&command_id).await?;

            match status.state.as_str() {
                "SUCCEEDED" => {
                    return self.decode_response(&command_id, status.response).await;
                }
                "FAILED" => {
                    let (code, message) = status
                        .response
                        .as_ref()
                        .map(|r| {
                            (
                                r.code.clone().unwrap_or_default(),
                                r.message.clone().unwrap_or_default(),
                            )
                        })
                        .unwrap_or_default();
                    return Err(CommandError::DeploymentError {
                        command_id,
                        code,
                        message,
                    });
                }
                "EXPIRED" => {
                    return Err(CommandError::Expired { command_id });
                }
                _ => {
                    // Still in progress — backoff
                    interval = Duration::from_secs_f64(
                        (interval.as_secs_f64() * self.config.poll_backoff)
                            .min(self.config.max_poll_interval.as_secs_f64()),
                    );
                }
            }
        }
    }

    /// Create a command without waiting for the result. Returns the command ID.
    pub async fn create<P: Serialize>(
        &self,
        command: &str,
        params: P,
        options: Option<&InvokeOptions>,
    ) -> Result<String, CommandError> {
        let params_json = serde_json::to_vec(&params)?;
        let params_base64 = general_purpose::STANDARD.encode(&params_json);

        let mut body = serde_json::json!({
            "deploymentId": self.deployment_id,
            "command": command,
            "params": {
                "mode": "inline",
                "inlineBase64": params_base64,
            },
        });

        if let Some(opts) = options {
            if let Some(deadline) = opts.deadline {
                body["deadline"] = serde_json::Value::String(deadline.to_rfc3339());
            }
            if let Some(ref key) = opts.idempotency_key {
                body["idempotencyKey"] = serde_json::Value::String(key.clone());
            }
        }

        let url = format!("{}/commands", self.manager_url);
        let resp = self.http_client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(CommandError::CreationFailed { status, body });
        }

        let result: CreateCommandResponse = resp.json().await?;
        Ok(result.command_id)
    }

    /// Poll for a command's status.
    pub async fn get_status(
        &self,
        command_id: &str,
    ) -> Result<CommandStatusResponse, CommandError> {
        let url = format!("{}/commands/{}", self.manager_url, command_id);
        let resp = self.http_client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(CommandError::CreationFailed { status, body });
        }

        Ok(resp.json().await?)
    }

    // -- Internal helpers --

    async fn decode_response<R: DeserializeOwned>(
        &self,
        command_id: &str,
        response: Option<CommandResponseBody>,
    ) -> Result<R, CommandError> {
        let resp = response.ok_or_else(|| CommandError::ResponseDecodingFailed {
            command_id: command_id.to_string(),
            reason: "No response body in SUCCEEDED status".to_string(),
        })?;

        let body = resp
            .response
            .ok_or_else(|| CommandError::ResponseDecodingFailed {
                command_id: command_id.to_string(),
                reason: "No response field in success response".to_string(),
            })?;

        let bytes = match body.mode.as_str() {
            "inline" => {
                let base64_data =
                    body.inline_base64
                        .ok_or_else(|| CommandError::ResponseDecodingFailed {
                            command_id: command_id.to_string(),
                            reason: "Inline response missing inlineBase64 field".to_string(),
                        })?;

                general_purpose::STANDARD
                    .decode(&base64_data)
                    .map_err(|e| CommandError::ResponseDecodingFailed {
                        command_id: command_id.to_string(),
                        reason: format!("Base64 decode failed: {}", e),
                    })?
            }
            "storage" => {
                let get_request = body.storage_get_request.ok_or_else(|| {
                    CommandError::ResponseDecodingFailed {
                        command_id: command_id.to_string(),
                        reason: "Storage response missing storageGetRequest".to_string(),
                    }
                })?;

                self.download_from_storage(&get_request).await?
            }
            other => {
                return Err(CommandError::ResponseDecodingFailed {
                    command_id: command_id.to_string(),
                    reason: format!("Unknown response mode: {}", other),
                })
            }
        };

        serde_json::from_slice(&bytes).map_err(|e| CommandError::ResponseDecodingFailed {
            command_id: command_id.to_string(),
            reason: format!("JSON decode failed: {}", e),
        })
    }

    async fn download_from_storage(
        &self,
        get_request: &StorageGetRequest,
    ) -> Result<Vec<u8>, CommandError> {
        match get_request.backend.backend_type.as_str() {
            "http" => {
                let url = get_request.backend.url.as_deref().ok_or_else(|| {
                    CommandError::StorageOperationFailed {
                        reason: "HTTP storage backend missing url".to_string(),
                    }
                })?;

                let method = get_request.backend.method.as_deref().unwrap_or("GET");

                // Use a plain client (no auth headers — presigned URL carries auth)
                let plain_http = reqwest::Client::new();
                let mut req = match method {
                    "PUT" => plain_http.put(url),
                    "POST" => plain_http.post(url),
                    _ => plain_http.get(url),
                };

                if let Some(headers) = &get_request.backend.headers {
                    for (k, v) in headers {
                        req = req.header(k.as_str(), v.as_str());
                    }
                }

                let resp = req
                    .send()
                    .await
                    .map_err(|e| CommandError::StorageOperationFailed {
                        reason: format!("Storage download failed: {}", e),
                    })?;

                if !resp.status().is_success() {
                    return Err(CommandError::StorageOperationFailed {
                        reason: format!("Storage download returned HTTP {}", resp.status()),
                    });
                }

                resp.bytes().await.map(|b| b.to_vec()).map_err(|e| {
                    CommandError::StorageOperationFailed {
                        reason: format!("Failed to read storage response bytes: {}", e),
                    }
                })
            }
            "local" if self.config.allow_local_storage => {
                let file_path = get_request.backend.file_path.as_deref().ok_or_else(|| {
                    CommandError::StorageOperationFailed {
                        reason: "Local storage backend missing filePath".to_string(),
                    }
                })?;

                let path = std::path::Path::new(file_path);
                if path.is_absolute() || file_path.contains("..") {
                    return Err(CommandError::StorageOperationFailed {
                        reason: "Local storage path traversal detected".to_string(),
                    });
                }

                tokio::fs::read(file_path)
                    .await
                    .map_err(|e| CommandError::StorageOperationFailed {
                        reason: format!("Failed to read local file {}: {}", file_path, e),
                    })
            }
            "local" => Err(CommandError::StorageOperationFailed {
                reason: "Local storage backend not allowed (set allow_local_storage: true)"
                    .to_string(),
            }),
            other => Err(CommandError::StorageOperationFailed {
                reason: format!("Unknown storage backend type: {}", other),
            }),
        }
    }
}
