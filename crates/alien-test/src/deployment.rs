//! `TestDeployment` -- helpers for managing a deployment during E2E tests.
//!
//! Wraps an SDK client and deployment metadata, providing convenience methods
//! for creating deployments, waiting for status, invoking commands, upgrading,
//! and tearing down.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tracing::debug;

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
    /// Deployment-scoped token for proxy auth.
    pub token: String,
    /// Reference back to the owning manager.
    manager: Arc<TestManager>,
    /// Foreground agent child process. Killed when the deployment is dropped.
    _foreground_agent: Option<tokio::process::Child>,
}

impl Drop for TestDeployment {
    fn drop(&mut self) {
        if let Some(ref mut child) = self._foreground_agent {
            let _ = child.start_kill();
        }
    }
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
        token: String,
        manager: Arc<TestManager>,
    ) -> Self {
        Self {
            id,
            name,
            platform,
            url,
            token,
            manager,
            _foreground_agent: None,
        }
    }

    /// Attach a foreground agent child process to this deployment.
    /// The process will be killed when the deployment is dropped.
    pub fn set_foreground_agent(&mut self, child: tokio::process::Child) {
        self._foreground_agent = Some(child);
    }

    /// Kill the foreground agent and wait for it to exit.
    pub async fn kill_foreground_agent(&mut self) {
        if let Some(ref mut child) = self._foreground_agent {
            let _ = child.start_kill();
            match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "Failed to wait for agent exit"),
                Err(_) => tracing::warn!("Timed out waiting for agent to exit"),
            }
        }
        self._foreground_agent = None;
    }

    /// Get a reference to the owning manager.
    pub fn manager(&self) -> &Arc<TestManager> {
        &self.manager
    }

    /// Poll the manager until the deployment reaches the `"running"` status,
    /// or until `timeout` elapses. On success, populates `self.url` from a
    /// public Worker/Container resource output URL if available.
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
            debug!(deployment = %self.id, %status, "polling deployment status");

            if status == "running" {
                // Extract the public URL from stack_state resource outputs.
                if let Some(ref state_value) = resp.stack_state {
                    if let Some(url) = public_url_from_stack_state(state_value) {
                        debug!(
                            deployment = %self.id,
                            %url,
                            "deployment URL discovered from resource outputs"
                        );
                        self.url = Some(url);
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
                    .map(|v| serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()))
                    .unwrap_or_else(|| "(no error details)".to_string());
                let stack_state_summary = resp
                    .stack_state
                    .as_ref()
                    .map(|v| serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()))
                    .unwrap_or_default();
                tracing::error!(
                    deployment = %self.id,
                    %status,
                    "Deployment failed.\nError: {}\nStack state: {}",
                    error_msg,
                    stack_state_summary,
                );
                return Err(format!(
                    "Deployment {} reached terminal status: {}\nError: {}",
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

    /// Refresh the deployment's public URL from manager stack outputs.
    pub async fn refresh_public_url(
        &mut self,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
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

        if let Some(ref state_value) = resp.stack_state {
            if let Some(url) = public_url_from_stack_state(state_value) {
                debug!(
                    deployment = %self.id,
                    %url,
                    "deployment URL refreshed from resource outputs"
                );
                self.url = Some(url);
            }
        }

        Ok(self.url.clone())
    }

    /// Wait for the manager to expose a public URL in resource outputs.
    pub async fn wait_for_public_url(
        &mut self,
        timeout: Duration,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(url) = self.refresh_public_url().await? {
                return Ok(url.trim_end_matches('/').to_string());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "Timed out waiting for deployment {} to expose a public URL",
                    self.id
                )
                .into());
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Invoke a command on the deployment and return the JSON result.
    ///
    /// Uses the commands protocol:
    /// 1. `POST /v1/commands` — create the command (inline or storage mode)
    /// 2. If storage mode: upload params to presigned URL, then confirm
    /// 3. Poll `GET /v1/commands/{id}` until the command reaches a terminal state
    /// 4. Extract and decode the response from the status response
    pub async fn invoke_command(
        &self,
        name: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        use alien_commands_client::{CommandsClient, CommandsClientConfig};
        use std::time::Duration;

        let client = CommandsClient::with_config(
            &format!("{}/v1", self.manager.url),
            &self.id,
            &self.manager.admin_token,
            CommandsClientConfig {
                allow_local_storage: true,
                timeout: Duration::from_secs(120),
                ..Default::default()
            },
        );

        client
            .invoke::<Value, Value>(name, params)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
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

        debug!(deployment = %self.id, "deployment upgrade (redeploy) triggered");
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
        self.destroy_with_scope(alien_manager_api::types::DeleteScope::Full)
            .await
    }

    /// Destroy this deployment via the SDK with an explicit delete scope.
    pub async fn destroy_with_scope(
        &self,
        delete_scope: alien_manager_api::types::DeleteScope,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.manager
            .client()
            .delete_deployment()
            .id(&self.id)
            .delete_scope(delete_scope)
            .send()
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                format!("Failed to destroy deployment {}: {}", self.id, e).into()
            })?;

        debug!(deployment = %self.id, "deployment destroyed");
        Ok(())
    }
}

fn public_url_from_resource_outputs(outputs: &alien_core::ResourceOutputs) -> Option<String> {
    if let Some(worker) = outputs.downcast_ref::<alien_core::WorkerOutputs>() {
        return worker.url.clone();
    }
    if let Some(container) = outputs.downcast_ref::<alien_core::ContainerOutputs>() {
        return container.url.clone();
    }
    None
}

fn public_url_from_stack_state(state_value: &Value) -> Option<String> {
    let stack_state = serde_json::from_value::<alien_core::StackState>(state_value.clone()).ok()?;
    for resource_state in stack_state.resources.values() {
        if let Some(ref outputs) = resource_state.outputs {
            if let Some(url) = public_url_from_resource_outputs(outputs) {
                return Some(url);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_output_url_uses_explicit_container_url_first() {
        let outputs = alien_core::ResourceOutputs::new(alien_core::ContainerOutputs {
            name: "api".to_string(),
            status: alien_core::ContainerStatus::Running,
            current_replicas: 1,
            desired_replicas: 1,
            internal_dns: "api".to_string(),
            url: Some("https://api.example.com".to_string()),
            replicas: Vec::new(),
            load_balancer_endpoint: Some(alien_core::LoadBalancerEndpoint {
                dns_name: "k8s-api.example.elb.amazonaws.com".to_string(),
                hosted_zone_id: None,
            }),
        });

        assert_eq!(
            public_url_from_resource_outputs(&outputs),
            Some("https://api.example.com".to_string())
        );
    }

    #[test]
    fn resource_output_url_does_not_derive_from_load_balancer_endpoint() {
        let outputs = alien_core::ResourceOutputs::new(alien_core::ContainerOutputs {
            name: "api".to_string(),
            status: alien_core::ContainerStatus::Running,
            current_replicas: 1,
            desired_replicas: 1,
            internal_dns: "api".to_string(),
            url: None,
            replicas: Vec::new(),
            load_balancer_endpoint: Some(alien_core::LoadBalancerEndpoint {
                dns_name: "k8s-api.example.elb.amazonaws.com".to_string(),
                hosted_zone_id: None,
            }),
        });

        assert_eq!(public_url_from_resource_outputs(&outputs), None);
    }
}
