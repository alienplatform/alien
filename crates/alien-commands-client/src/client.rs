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
    /// Explicit target resource id within the deployment's stack. When `None`
    /// the server resolves the target via single-target shorthand (exactly one
    /// command-capable resource must exist). Set this when a deployment has
    /// more than one command-capable resource.
    pub target_resource_id: Option<String>,
}

/// High-level client for invoking commands on Alien deployments.
pub struct CommandsClient {
    manager_url: String,
    deployment_id: String,
    http_client: reqwest::Client,
    config: CommandsClientConfig,
}

// -- API response types (internal) --

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCommandResponse {
    command_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandStatusResponse {
    state: String,
    #[serde(default)]
    response: Option<CommandResponseBody>,
    /// The resolved target this command was addressed to. Carried
    /// on the wire for observability; the polling logic keys off `state` only.
    #[serde(default)]
    #[allow(dead_code)]
    target: Option<alien_core::CommandTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandResponseBody {
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
            http_client,
            config,
        }
    }

    /// Build a client over a caller-supplied HTTP client, reusing the headers
    /// it already carries (the auth header, and the workspace header used in
    /// platform mode). `with_config` builds a token-only client and can't add
    /// those.
    pub fn with_http_client(
        manager_url: &str,
        deployment_id: &str,
        http_client: reqwest::Client,
        config: CommandsClientConfig,
    ) -> Self {
        Self {
            manager_url: manager_url.trim_end_matches('/').to_string(),
            deployment_id: deployment_id.to_string(),
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

    /// Scope this client to one target command-capable resource: every
    /// `invoke`/`invoke_with_options`/`create` call made through the
    /// returned builder presets `target_resource_id` to `resource_id`,
    /// mirroring the TypeScript `.target(name).invoke(...)` shorthand.
    ///
    /// If the caller also passes an [`InvokeOptions`] with its own
    /// `target_resource_id` set, the builder's target silently wins —
    /// passing two different targets is a programmer error, not a runtime
    /// conflict this builder tries to detect.
    pub fn target(&self, resource_id: impl Into<String>) -> TargetedCommands<'_> {
        TargetedCommands {
            client: self,
            resource_id: resource_id.into(),
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

        let body = self.build_create_body(command, &params_base64, options);

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
    async fn get_status(&self, command_id: &str) -> Result<CommandStatusResponse, CommandError> {
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
                        reason: format!("Storage download failed: {}", e.without_url()),
                    })?;

                if !resp.status().is_success() {
                    return Err(CommandError::StorageOperationFailed {
                        reason: format!("Storage download returned HTTP {}", resp.status()),
                    });
                }

                resp.bytes().await.map(|b| b.to_vec()).map_err(|e| {
                    CommandError::StorageOperationFailed {
                        reason: format!(
                            "Failed to read storage response bytes: {}",
                            e.without_url()
                        ),
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

    /// Build the JSON body `create` sends. Pure (no I/O) so the body shape —
    /// including a builder-preset `targetResourceId` — is directly
    /// unit-testable.
    fn build_create_body(
        &self,
        command: &str,
        params_base64: &str,
        options: Option<&InvokeOptions>,
    ) -> serde_json::Value {
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
            if let Some(ref target) = opts.target_resource_id {
                body["targetResourceId"] = serde_json::Value::String(target.clone());
            }
        }

        body
    }
}

/// A [`CommandsClient`] scoped to one target command-capable resource.
///
/// Obtained via [`CommandsClient::target`]; borrows the client rather than
/// cloning it (the client is not `Clone` and doesn't need to be for this —
/// the builder is just a thin wrapper that presets one field).
pub struct TargetedCommands<'a> {
    client: &'a CommandsClient,
    resource_id: String,
}

impl TargetedCommands<'_> {
    /// Invoke a command against this builder's target and wait for the result.
    pub async fn invoke<P: Serialize, R: DeserializeOwned>(
        &self,
        command: &str,
        params: P,
    ) -> Result<R, CommandError> {
        self.invoke_with_options(command, params, None).await
    }

    /// Invoke a command against this builder's target with options, and wait
    /// for the result.
    pub async fn invoke_with_options<P: Serialize, R: DeserializeOwned>(
        &self,
        command: &str,
        params: P,
        options: Option<InvokeOptions>,
    ) -> Result<R, CommandError> {
        self.client
            .invoke_with_options(command, params, Some(self.preset(options)))
            .await
    }

    /// Create a command against this builder's target without waiting for
    /// the result. Returns the command ID.
    pub async fn create<P: Serialize>(
        &self,
        command: &str,
        params: P,
        options: Option<InvokeOptions>,
    ) -> Result<String, CommandError> {
        let options = self.preset(options);
        self.client.create(command, params, Some(&options)).await
    }

    /// Preset `target_resource_id` to this builder's resource id, overwriting
    /// any value already set on `options` (see [`CommandsClient::target`]).
    fn preset(&self, options: Option<InvokeOptions>) -> InvokeOptions {
        let mut options = options.unwrap_or(InvokeOptions {
            timeout: None,
            deadline: None,
            idempotency_key: None,
            target_resource_id: None,
        });
        options.target_resource_id = Some(self.resource_id.clone());
        options
    }
}

#[cfg(test)]
mod target_tests {
    //! Proves the client-side surface for command targets compiles
    //! and round-trips over the wire — not the server's routing behavior
    //! (that's covered by alien-commands' integration tests).

    use super::*;

    /// `InvokeOptions` accepts a `target_resource_id`, which `create()` sends
    /// as `targetResourceId` in the request body (see the `create` body
    /// construction above).
    #[test]
    fn invoke_options_carries_target_resource_id() {
        let options = InvokeOptions {
            timeout: None,
            deadline: None,
            idempotency_key: None,
            target_resource_id: Some("worker-7".to_string()),
        };
        assert_eq!(options.target_resource_id.as_deref(), Some("worker-7"));
    }

    /// The client's internal status response type deserializes a status JSON
    /// payload that carries a resolved `target`, proving the
    /// generated/hand-written client type is
    /// wire-compatible with the server's `CommandStatusResponse`.
    #[test]
    fn command_status_response_deserializes_target() {
        let json = serde_json::json!({
            "state": "SUCCEEDED",
            "target": {
                "resourceId": "worker-7",
                "resourceType": "worker",
            },
        });

        let status: CommandStatusResponse =
            serde_json::from_value(json).expect("status JSON with target should deserialize");

        assert_eq!(status.state, "SUCCEEDED");
        let target = status.target.expect("target field should be present");
        assert_eq!(target.resource_id, "worker-7");
        assert_eq!(target.resource_type, alien_core::CommandTargetType::Worker);
    }

    /// `CommandsClient::target(...)` presets `target_resource_id`
    /// on an otherwise-empty options value.
    #[test]
    fn target_builder_presets_target_resource_id() {
        let client = CommandsClient::new("http://localhost:9090", "dep_123", "token");
        let targeted = client.target("worker-9");

        let options = targeted.preset(None);

        assert_eq!(options.target_resource_id.as_deref(), Some("worker-9"));
    }

    /// The builder's target wins over an explicit
    /// `target_resource_id` the caller already set on `InvokeOptions` — a
    /// conflict here is a programmer error, not something this builder
    /// tries to reconcile at runtime (see `CommandsClient::target` docs).
    #[test]
    fn target_builder_overrides_conflicting_explicit_target() {
        let client = CommandsClient::new("http://localhost:9090", "dep_123", "token");
        let targeted = client.target("worker-9");

        let options = targeted.preset(Some(InvokeOptions {
            timeout: None,
            deadline: None,
            idempotency_key: None,
            target_resource_id: Some("worker-other".to_string()),
        }));

        assert_eq!(options.target_resource_id.as_deref(), Some("worker-9"));
    }

    /// The target builder's preset ends up in the actual JSON
    /// body `create()` sends, as `targetResourceId`.
    #[test]
    fn target_builder_presets_field_in_create_request_body() {
        let client = CommandsClient::new("http://localhost:9090", "dep_123", "token");
        let targeted = client.target("worker-9");
        let options = targeted.preset(None);

        let body = client.build_create_body("generate-report", "e30=", Some(&options));

        assert_eq!(body["targetResourceId"], "worker-9");
    }

    #[tokio::test]
    async fn storage_transport_error_does_not_expose_presigned_url_token() {
        let secret = "do-not-log-response-token";
        let client = CommandsClient::new("http://localhost:9090", "dep_123", "token");
        let request = StorageGetRequest {
            backend: StorageBackend {
                backend_type: "http".to_string(),
                url: Some(format!(
                    "http://127.0.0.1:0/blob?response_token={secret}&expires=1"
                )),
                method: Some("GET".to_string()),
                headers: None,
                file_path: None,
            },
        };

        let error = client
            .download_from_storage(&request)
            .await
            .expect_err("port zero must reject the storage download");
        let display = error.to_string();
        let debug = format!("{error:?}");

        assert!(!display.contains(secret), "display error leaked token");
        assert!(!debug.contains(secret), "debug error leaked token");
    }
}
