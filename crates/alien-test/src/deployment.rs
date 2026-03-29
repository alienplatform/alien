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
                // StackState is: { resources: { <id>: { type, status, outputs: { url, ... } } } }
                if let Some(ref stack_state) = resp.stack_state {
                    if let Some(resources) =
                        stack_state.get("resources").and_then(|v| v.as_object())
                    {
                        for (_resource_id, resource_state) in resources {
                            let resource_type = resource_state
                                .get("type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if resource_type == "function" || resource_type == "container" {
                                if let Some(url) = resource_state
                                    .get("outputs")
                                    .and_then(|o| o.get("url"))
                                    .and_then(|v| v.as_str())
                                {
                                    self.url = Some(url.to_string());
                                    info!(
                                        deployment = %self.id,
                                        resource = %_resource_id,
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
                    // Extract inline response
                    if let Some(response) = status_data.get("response") {
                        if let Some(inline_b64) = response
                            .get("body")
                            .and_then(|b| b.get("inlineBase64"))
                            .and_then(|v| v.as_str())
                        {
                            let decoded = general_purpose::STANDARD.decode(inline_b64)?;
                            let result: Value = serde_json::from_slice(&decoded)?;
                            return Ok(result);
                        }
                    }
                    // If no inline response, return the status data itself
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
