//! `TestDeployment` -- helpers for managing a deployment during E2E tests.
//!
//! Wraps an SDK client and deployment metadata, providing convenience methods
//! for creating deployments, waiting for status, invoking commands, upgrading,
//! and tearing down.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tracing::info;

use crate::manager::TestManager;

/// A handle to a deployment created through a [`TestManager`].
pub struct TestDeployment {
    /// Deployment ID returned by the manager.
    pub id: String,
    /// Human-readable deployment name.
    pub name: String,
    /// Target platform (e.g. `aws`, `gcp`, `azure`, `local`).
    pub platform: String,
    /// The deployment's public URL, if assigned.
    pub url: Option<String>,
    /// Reference back to the owning manager.
    manager: Arc<TestManager>,
}

impl TestDeployment {
    /// Create a new `TestDeployment` handle.
    ///
    /// This does **not** create a deployment on the manager; use `e2e::deploy_test_app()`
    /// or the SDK client directly, then wrap the response here.
    pub fn new(
        id: String,
        name: String,
        platform: String,
        url: Option<String>,
        manager: Arc<TestManager>,
    ) -> Self {
        Self {
            id,
            name,
            platform,
            url,
            manager,
        }
    }

    /// Get a reference to the owning manager.
    pub fn manager(&self) -> &Arc<TestManager> {
        &self.manager
    }

    /// Poll the manager until the deployment reaches the `"running"` status,
    /// or until `timeout` elapses. On success, populates `self.url` from the
    /// deployment's `stackState.publicUrl` if available.
    pub async fn wait_until_running(
        &mut self,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = tokio::time::Instant::now() + timeout;
        let poll_interval = Duration::from_secs(2);

        loop {
            let resp = self
                .manager
                .client()
                .get_deployment()
                .id(&self.id)
                .send()
                .await
                .map_err(|e| -> Box<dyn std::error::Error> {
                    format!("Failed to get deployment {}: {}", self.id, e).into()
                })?;

            let status = resp.status.as_str();
            info!(deployment = %self.id, %status, "polling deployment status");

            if status == "running" {
                // Extract the public URL from stack_state resource outputs.
                if let Some(ref state_value) = resp.stack_state {
                    if let Ok(stack_state) =
                        serde_json::from_value::<alien_core::StackState>(state_value.clone())
                    {
                        for (resource_id, resource_state) in &stack_state.resources {
                            if let Some(ref outputs) = resource_state.outputs {
                                let url = if let Some(f) =
                                    outputs.downcast_ref::<alien_core::FunctionOutputs>()
                                {
                                    f.url.as_deref()
                                } else if let Some(c) =
                                    outputs.downcast_ref::<alien_core::ContainerOutputs>()
                                {
                                    c.url.as_deref()
                                } else {
                                    None
                                };
                                if let Some(url) = url {
                                    self.url = Some(url.to_string());
                                    info!(
                                        deployment = %self.id,
                                        resource = %resource_id,
                                        %url,
                                        "deployment URL discovered from resource outputs"
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
                // Fallback: also try environment_info
                if self.url.is_none() {
                    if let Some(ref env_info) = resp.environment_info {
                        if let Some(url) = env_info.get("url").and_then(|v| v.as_str()) {
                            self.url = Some(url.to_string());
                            info!(deployment = %self.id, %url, "deployment URL from environment_info");
                        }
                    }
                }
                return Ok(());
            }
            if status == "failed"
                || status == "destroyed"
                || status == "deleted"
                || status.ends_with("-failed")
            {
                let error_msg = resp
                    .error
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                return Err(format!(
                    "Deployment {} reached terminal status: {} {}",
                    self.id, status, error_msg
                )
                .into());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "Timed out waiting for deployment {} to reach running (last status: {})",
                    self.id, status
                )
                .into());
            }
            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Invoke a command on the deployment and return the JSON result.
    ///
    /// Uses the commands protocol:
    /// 1. `POST /v1/commands` — create the command with inline params
    /// 2. Poll `GET /v1/commands/{id}` until the command reaches a terminal state
    /// 3. Extract and decode the response from the status response
    pub async fn invoke_command(
        &self,
        name: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        use base64::{engine::general_purpose, Engine as _};

        let http = self.manager.http_client();

        // Step 1: Create the command with inline params
        let params_bytes = serde_json::to_vec(&params)?;
        let params_base64 = general_purpose::STANDARD.encode(&params_bytes);

        let create_body = serde_json::json!({
            "deploymentId": self.id,
            "command": name,
            "params": {
                "mode": "inline",
                "inlineBase64": params_base64,
            },
        });

        let create_url = format!("{}/v1/commands", self.manager.url);
        let create_resp = http.post(&create_url).json(&create_body).send().await?;

        if !create_resp.status().is_success() {
            let status = create_resp.status();
            let body_text = create_resp.text().await.unwrap_or_default();
            return Err(format!("Command create failed ({}): {}", status, body_text).into());
        }

        let create_result: Value = create_resp.json().await?;
        let command_id = create_result
            .get("commandId")
            .and_then(|v| v.as_str())
            .ok_or("Command response missing 'commandId'")?
            .to_string();

        info!(command_id = %command_id, command = %name, "Command created, polling for result");

        // Step 2: Poll for command completion
        let poll_interval = Duration::from_secs(2);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        let status_url = format!("{}/v1/commands/{}", self.manager.url, command_id);

        loop {
            let status_resp = http.get(&status_url).send().await?;

            if !status_resp.status().is_success() {
                let status = status_resp.status();
                let body_text = status_resp.text().await.unwrap_or_default();
                return Err(
                    format!("Command status check failed ({}): {}", status, body_text).into(),
                );
            }

            let status_data: Value = status_resp.json().await?;
            let state = status_data
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match state {
                "SUCCEEDED" => {
                    // Extract response from CommandResponse::Success
                    // Structure: { response: { status: "success", response: { mode: "inline"|"storage", ... } } }
                    if let Some(body_spec) = status_data
                        .get("response")
                        .and_then(|r| r.get("response"))
                    {
                        let mode = body_spec
                            .get("mode")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        info!(
                            command_id = %command_id,
                            mode = %mode,
                            "Command succeeded, decoding response"
                        );

                        match mode {
                            "inline" => {
                                if let Some(inline_b64) =
                                    body_spec.get("inlineBase64").and_then(|v| v.as_str())
                                {
                                    info!(
                                        command_id = %command_id,
                                        b64_len = inline_b64.len(),
                                        "Decoding inline base64 response"
                                    );
                                    let decoded =
                                        general_purpose::STANDARD.decode(inline_b64)?;
                                    let result: Value = serde_json::from_slice(&decoded)?;
                                    return Ok(result);
                                }
                            }
                            "storage" => {
                                if let Some(backend) = body_spec
                                    .get("storageGetRequest")
                                    .and_then(|r| r.get("backend"))
                                {
                                    let backend_type = backend
                                        .get("type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    info!(
                                        command_id = %command_id,
                                        backend_type = %backend_type,
                                        "Downloading response from storage"
                                    );

                                    let bytes = match backend_type {
                                        "http" => {
                                            // HTTP backend: fetch from presigned URL
                                            let url = backend
                                                .get("url")
                                                .and_then(|v| v.as_str())
                                                .ok_or("Storage HTTP backend missing 'url'")?;
                                            let method = backend
                                                .get("method")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("GET");
                                            let mut req = match method {
                                                "POST" => http.post(url),
                                                "PUT" => http.put(url),
                                                _ => http.get(url),
                                            };
                                            // Add any headers from the backend
                                            if let Some(headers) =
                                                backend.get("headers").and_then(|h| h.as_object())
                                            {
                                                for (k, v) in headers {
                                                    if let Some(v_str) = v.as_str() {
                                                        req = req.header(k.as_str(), v_str);
                                                    }
                                                }
                                            }
                                            let resp = req.send().await?;
                                            let status = resp.status();
                                            let body_bytes = resp.bytes().await?.to_vec();
                                            info!(
                                                command_id = %command_id,
                                                http_status = %status,
                                                body_len = body_bytes.len(),
                                                "Storage download complete"
                                            );
                                            if !status.is_success() {
                                                let preview = String::from_utf8_lossy(
                                                    &body_bytes[..body_bytes.len().min(500)]
                                                );
                                                return Err(format!(
                                                    "Storage download failed ({}): {}",
                                                    status, preview
                                                ).into());
                                            }
                                            body_bytes
                                        }
                                        _ => {
                                            // File backend: read from local file path
                                            let file_path = backend
                                                .get("filePath")
                                                .and_then(|v| v.as_str())
                                                .ok_or("Storage backend missing 'filePath'")?;
                                            tokio::fs::read(file_path).await?
                                        }
                                    };
                                    info!(
                                        command_id = %command_id,
                                        response_bytes = bytes.len(),
                                        "Parsing storage response as JSON"
                                    );
                                    let result: Value = serde_json::from_slice(&bytes)
                                        .map_err(|e| {
                                            let preview = String::from_utf8_lossy(
                                                &bytes[..bytes.len().min(200)]
                                            );
                                            format!(
                                                "JSON parse error: {}. First 200 bytes: {}",
                                                e, preview
                                            )
                                        })?;
                                    return Ok(result);
                                } else {
                                    info!(
                                        command_id = %command_id,
                                        body_spec = %body_spec,
                                        "Storage mode but no storageGetRequest found"
                                    );
                                }
                            }
                            _ => {
                                info!(
                                    command_id = %command_id,
                                    body_spec = %body_spec,
                                    "Unknown response mode"
                                );
                            }
                        }
                    } else {
                        info!(
                            command_id = %command_id,
                            status_data = %status_data,
                            "Command succeeded but no response body found"
                        );
                    }
                    // If no decodable response, return the status data itself
                    return Ok(status_data);
                }
                "FAILED" => {
                    let error_msg = status_data
                        .get("response")
                        .and_then(|r| r.get("error"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    return Err(format!("Command '{}' failed: {}", name, error_msg).into());
                }
                "EXPIRED" => {
                    return Err(format!("Command '{}' expired", name).into());
                }
                _ => {
                    // Still processing (PENDING, DISPATCHED, PENDING_UPLOAD)
                    if tokio::time::Instant::now() >= deadline {
                        return Err(format!(
                            "Timed out waiting for command '{}' (last state: {})",
                            name, state
                        )
                        .into());
                    }
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }

    /// Trigger a redeploy (upgrade) of this deployment via the SDK.
    pub async fn upgrade(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.manager
            .client()
            .redeploy()
            .id(&self.id)
            .send()
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                format!("Failed to upgrade deployment {}: {}", self.id, e).into()
            })?;

        info!(deployment = %self.id, "deployment upgrade (redeploy) triggered");
        Ok(())
    }

    /// Quick health check: returns `Ok(())` if the deployment is in `"running"`
    /// status, or an error otherwise.
    pub async fn check_health(&self) -> Result<(), Box<dyn std::error::Error>> {
        let resp = self
            .manager
            .client()
            .get_deployment()
            .id(&self.id)
            .send()
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                format!("Failed to get deployment {}: {}", self.id, e).into()
            })?;

        let status = resp.status.as_str();
        if status != "running" {
            return Err(
                format!("Deployment {} is not healthy (status: {})", self.id, status).into(),
            );
        }

        Ok(())
    }

    /// Destroy this deployment via the SDK.
    pub async fn destroy(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.manager
            .client()
            .delete_deployment()
            .id(&self.id)
            .send()
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                format!("Failed to destroy deployment {}: {}", self.id, e).into()
            })?;

        info!(deployment = %self.id, "deployment destroyed");
        Ok(())
    }
}
