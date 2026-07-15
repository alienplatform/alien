//! `TestDeployment` -- helpers for managing a deployment during E2E tests.
//!
//! Wraps an SDK client and deployment metadata, providing convenience methods
//! for creating deployments, waiting for status, invoking commands, upgrading,
//! and tearing down.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tracing::debug;

use crate::manager::TestManager;

const DELETION_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Terminal result of the operator-owned runtime deletion phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeletionOutcome {
    /// The deployment had no setup-owned resources and is fully deleted.
    Deleted,
    /// Runtime resources are gone; the client must delete setup-owned resources.
    SetupTeardownRequired,
}

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
    /// Local provider state used by both the operator and setup teardown.
    local_state_directory: Option<PathBuf>,
    /// Owns a foreground operator's temporary state directory.
    _foreground_data_dir: Option<tempfile::TempDir>,
}

impl Drop for TestDeployment {
    fn drop(&mut self) {
        if let Some(ref mut child) = self._foreground_agent {
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                // SAFETY: this child was created as the leader of an isolated
                // process group. SIGTERM reaches both alien-deploy and its
                // alien-operator child. Async teardown below is authoritative;
                // Drop is only a best-effort fallback.
                unsafe {
                    libc::kill(-(pid as libc::pid_t), libc::SIGTERM);
                }
            }
            #[cfg(windows)]
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
            local_state_directory: None,
            _foreground_data_dir: None,
        }
    }

    /// Attach a foreground agent and retain ownership of its state directory.
    /// The process is killed and the directory removed when the deployment is dropped.
    pub fn set_foreground_agent(
        &mut self,
        child: tokio::process::Child,
        state_directory: tempfile::TempDir,
    ) {
        self._foreground_agent = Some(child);
        self.local_state_directory = Some(state_directory.path().to_path_buf());
        self._foreground_data_dir = Some(state_directory);
    }

    /// Record the state directory owned by an installed Local operator service.
    pub(crate) fn set_local_service_state_directory(&mut self, state_directory: PathBuf) {
        self.local_state_directory = Some(state_directory);
    }

    /// Return the Local provider state directory needed for setup teardown.
    pub(crate) fn local_state_directory(&self) -> Option<&Path> {
        self.local_state_directory.as_deref()
    }

    /// Kill the foreground agent and wait for it to exit.
    pub async fn kill_foreground_agent(&mut self) {
        if let Some(ref mut child) = self._foreground_agent {
            if let Err(error) = terminate_foreground_agent(child).await {
                tracing::warn!(%error, "Failed to stop foreground agent cleanly");
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

    /// Build a commands client for this deployment.
    fn commands_client(&self) -> alien_commands_client::CommandsClient {
        use alien_commands_client::{CommandsClient, CommandsClientConfig};
        use std::time::Duration;

        CommandsClient::with_config(
            &format!("{}/v1", self.manager.url),
            &self.id,
            &self.manager.admin_token,
            CommandsClientConfig {
                allow_local_storage: true,
                timeout: Duration::from_secs(120),
                ..Default::default()
            },
        )
    }

    /// Invoke a command on the deployment and return the JSON result.
    ///
    /// No explicit target: the server resolves the target via single-target
    /// shorthand, which only works when the deployment has exactly one
    /// command-capable resource. With two or more, this must fail — the
    /// routing E2E asserts exactly that.
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
        self.commands_client()
            .invoke::<Value, Value>(name, params)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    /// Invoke a command addressed to one specific command-capable resource,
    /// mirroring the TypeScript `.target(name).invoke(...)` shorthand.
    pub async fn invoke_command_on_target(
        &self,
        target_resource_id: &str,
        name: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        self.commands_client()
            .target(target_resource_id)
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
        self.manager
            .client()
            .delete_deployment()
            .id(&self.id)
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Cleanup,
            })
            .send()
            .await
            .map_err(|e| -> Box<dyn std::error::Error> {
                format!("Failed to destroy deployment {}: {}", self.id, e).into()
            })?;

        debug!(deployment = %self.id, "deployment destroyed");
        Ok(())
    }

    /// Wait for a pull operator to finish the deletion state machine.
    ///
    /// Pull deletion is asynchronous: the manager records `delete-pending`,
    /// then the operator observes it and tears down the local workloads. The
    /// caller must keep that operator alive until this method returns.
    pub(crate) async fn wait_until_deleted(
        &self,
        timeout: Duration,
    ) -> Result<DeletionOutcome, Box<dyn std::error::Error>> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let status = match self
                .manager
                .client()
                .get_deployment()
                .id(&self.id)
                .send()
                .await
            {
                Ok(response) => response.status.to_string(),
                Err(alien_manager_api::Error::UnexpectedResponse(response))
                    if response.status() == reqwest::StatusCode::NOT_FOUND =>
                {
                    return Ok(DeletionOutcome::Deleted);
                }
                Err(alien_manager_api::Error::ErrorResponse(response))
                    if response.status() == reqwest::StatusCode::NOT_FOUND =>
                {
                    return Ok(DeletionOutcome::Deleted);
                }
                Err(error) => {
                    return Err(format!(
                        "Failed to get deployment {} while waiting for deletion: {}",
                        self.id, error
                    )
                    .into());
                }
            };

            if let Some(outcome) = deletion_outcome(&status)? {
                return Ok(outcome);
            }
            debug!(deployment_id = %self.id, %status, "waiting for deployment deletion");
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "Timed out waiting for deployment {} deletion handoff (last status: {status})",
                    self.id
                )
                .into());
            }
            tokio::time::sleep(DELETION_POLL_INTERVAL).await;
        }
    }
}

fn deletion_outcome(status: &str) -> Result<Option<DeletionOutcome>, Box<dyn std::error::Error>> {
    match status {
        "deleted" | "destroyed" => Ok(Some(DeletionOutcome::Deleted)),
        "teardown-required" | "teardown-failed" => Ok(Some(DeletionOutcome::SetupTeardownRequired)),
        "delete-failed" => {
            Err(format!("Deployment deletion reached terminal status: {status}").into())
        }
        _ => Ok(None),
    }
}

pub(crate) async fn terminate_foreground_agent(
    child: &mut tokio::process::Child,
) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: this child is the leader of the isolated foreground process
        // group created by the E2E harness. SIGTERM reaches the deploy wrapper
        // and operator; operator shutdown drains separately-grouped runtimes.
        let result = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
    }

    #[cfg(windows)]
    child.start_kill()?;

    match tokio::time::timeout(Duration::from_secs(45), child.wait()).await {
        Ok(status) => {
            status?;
        }
        Err(_) => {
            tracing::warn!("Foreground agent group did not exit gracefully; killing it");
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                // SAFETY: same isolated process group as above.
                unsafe {
                    libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
                }
            }
            #[cfg(windows)]
            child.start_kill()?;
            child.wait().await?;
        }
    }

    Ok(())
}

fn public_url_from_resource_outputs(outputs: &alien_core::ResourceOutputs) -> Option<String> {
    if let Some(worker) = outputs.downcast_ref::<alien_core::WorkerOutputs>() {
        return worker
            .public_endpoints
            .values()
            .next()
            .map(|endpoint| endpoint.url.clone());
    }
    if let Some(container) = outputs.downcast_ref::<alien_core::ContainerOutputs>() {
        return container
            .public_endpoints
            .values()
            .next()
            .map(|endpoint| endpoint.url.clone());
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
    fn deletion_statuses_distinguish_progress_handoff_and_completion() {
        for (status, expected) in [
            ("delete-pending", None),
            ("deleting", None),
            (
                "teardown-required",
                Some(DeletionOutcome::SetupTeardownRequired),
            ),
            (
                "teardown-failed",
                Some(DeletionOutcome::SetupTeardownRequired),
            ),
            ("deleted", Some(DeletionOutcome::Deleted)),
            ("destroyed", Some(DeletionOutcome::Deleted)),
        ] {
            assert_eq!(
                deletion_outcome(status).expect("status should be accepted"),
                expected,
                "{status}"
            );
        }

        let error = deletion_outcome("delete-failed")
            .expect_err("runtime deletion failure must not be treated as setup handoff");
        assert_eq!(
            error.to_string(),
            "Deployment deletion reached terminal status: delete-failed"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn foreground_agent_teardown_signals_the_process_group() {
        let temp = tempfile::tempdir().expect("create test directory");
        let marker = temp.path().join("terminated");
        let ready = temp.path().join("ready");

        let mut child = tokio::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(
                r#"
trap 'echo wrapper >> "$MARKER"; exit 0' TERM
/bin/sh -c 'trap '\''echo child >> "$MARKER"; exit 0'\'' TERM; echo ready > "$READY"; while :; do sleep 1; done' &
while [ ! -f "$READY" ]; do sleep 0.01; done
wait
"#,
            )
            .env("MARKER", &marker)
            .env("READY", &ready)
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .expect("spawn isolated process group");

        tokio::time::timeout(Duration::from_secs(5), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("descendant should become ready");

        terminate_foreground_agent(&mut child)
            .await
            .expect("terminate foreground process group");

        let terminated = std::fs::read_to_string(marker).expect("read termination marker");
        assert!(terminated.lines().any(|line| line == "wrapper"));
        assert!(terminated.lines().any(|line| line == "child"));
    }

    #[test]
    fn resource_output_url_uses_explicit_container_url_first() {
        let outputs = alien_core::ResourceOutputs::new(alien_core::ContainerOutputs {
            name: "api".to_string(),
            status: alien_core::ContainerStatus::Running,
            current_replicas: 1,
            desired_replicas: 1,
            internal_dns: "api".to_string(),
            replicas: Vec::new(),
            public_endpoints: std::collections::HashMap::from([(
                "api".to_string(),
                alien_core::PublicEndpointOutput {
                    url: "https://api.example.com".to_string(),
                    host: "api.example.com".to_string(),
                    wildcard_host: None,
                    load_balancer_endpoint: Some(alien_core::LoadBalancerEndpoint {
                        dns_name: "k8s-api.example.elb.amazonaws.com".to_string(),
                        hosted_zone_id: None,
                    }),
                },
            )]),
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
            replicas: Vec::new(),
            public_endpoints: std::collections::HashMap::new(),
        });

        assert_eq!(public_url_from_resource_outputs(&outputs), None);
    }
}
