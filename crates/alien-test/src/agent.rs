//! `TestAlienAgent` -- helpers for running alien-agent containers in pull mode.
//!
//! In pull mode the alien-agent runs inside a Docker container (or as a Helm
//! release in Kubernetes), connects to the manager, and executes deployments
//! locally. This module provides helpers to start and stop such agents during
//! E2E tests.

use std::sync::Arc;

use alien_core::Platform;
use tracing::info;

use crate::manager::TestManager;

/// Default Docker label applied to test alien-agent containers so they can be
/// cleaned up afterwards.
pub const TEST_AGENT_LABEL: &str = "alien-test-agent=true";

/// A running alien-agent container for pull-model E2E tests.
pub struct TestAlienAgent {
    /// Docker container ID (if started via `start_container`).
    pub container_id: Option<String>,
    /// Helm release name (if started via `helm_install`).
    pub helm_release: Option<String>,
    /// Kubernetes namespace for Helm installs.
    pub helm_namespace: Option<String>,
    /// Kubeconfig path for Helm installs.
    pub kubeconfig: Option<String>,
    /// The platform this agent targets.
    pub platform: Platform,
}

impl TestAlienAgent {
    /// Start an alien-agent Docker container that connects to the given
    /// `TestManager`.
    ///
    /// The container is labelled with [`TEST_AGENT_LABEL`] for easy cleanup.
    /// `image` is the alien-agent Docker image to run (e.g.
    /// `ghcr.io/alienplatform/alien-agent:latest`).
    pub async fn start_container(
        manager: &TestManager,
        image: &str,
        platform: Platform,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        info!(
            manager_url = %manager.url,
            %image,
            platform = %platform.as_str(),
            "starting alien-agent container"
        );

        let output = tokio::process::Command::new("docker")
            .args([
                "run",
                "-d",
                "--label",
                TEST_AGENT_LABEL,
                // The agent needs to reach the manager on the host network
                "--network",
                "host",
                "-e",
                &format!("ALIEN_MANAGER_URL={}", manager.url),
                "-e",
                &format!("ALIEN_API_KEY={}", manager.admin_token),
                "-e",
                &format!("ALIEN_PLATFORM={}", platform.as_str()),
                image,
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to start alien-agent container: {}", stderr).into());
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        info!(%container_id, "alien-agent container started");

        Ok(Self {
            container_id: Some(container_id),
            helm_release: None,
            helm_namespace: None,
            kubeconfig: None,
            platform,
        })
    }

    /// Install the alien-agent via Helm into a Kubernetes cluster.
    ///
    /// `release_name` is the Helm release name, `namespace` the target
    /// namespace, and `chart` the chart reference (path or repo URL).
    pub async fn helm_install(
        manager: &TestManager,
        chart: &str,
        release_name: &str,
        namespace: &str,
        kubeconfig: Option<&str>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        info!(
            %release_name,
            %namespace,
            %chart,
            manager_url = %manager.url,
            "installing alien-agent via helm"
        );

        let mut cmd = tokio::process::Command::new("helm");
        cmd.args([
            "install",
            release_name,
            chart,
            "--namespace",
            namespace,
            "--create-namespace",
            "--set",
            &format!("config.managerUrl={}", manager.url),
            "--set",
            &format!("config.apiKey={}", manager.admin_token),
            "--wait",
            "--timeout",
            "120s",
        ]);

        if let Some(kc) = kubeconfig {
            cmd.env("KUBECONFIG", kc);
        }

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Helm install failed: {}", stderr).into());
        }

        info!(%release_name, "alien-agent helm release installed");

        Ok(Self {
            container_id: None,
            helm_release: Some(release_name.to_string()),
            helm_namespace: Some(namespace.to_string()),
            kubeconfig: kubeconfig.map(String::from),
            platform: Platform::Kubernetes,
        })
    }

    /// Stop and remove the Docker container.
    pub async fn stop(self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref container_id) = self.container_id {
            info!(%container_id, "stopping alien-agent container");

            let output = tokio::process::Command::new("docker")
                .args(["rm", "-f", container_id])
                .output()
                .await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "Failed to stop alien-agent container {}: {}",
                    container_id, stderr
                )
                .into());
            }

            info!(%container_id, "alien-agent container stopped");
        }

        Ok(())
    }

    /// Uninstall the Helm release.
    pub async fn helm_uninstall(self) -> Result<(), Box<dyn std::error::Error>> {
        let release = self
            .helm_release
            .as_deref()
            .ok_or("Not a Helm-installed agent")?;
        let namespace = self
            .helm_namespace
            .as_deref()
            .ok_or("Missing Helm namespace")?;

        crate::cleanup::cleanup_helm_release(
            release,
            namespace,
            self.kubeconfig.as_deref(),
        )
        .await
    }
}

// Allow `Arc<TestManager>` usage (the agent only borrows the manager during
// construction, it does not hold a persistent reference).
impl TestAlienAgent {
    /// Convenience wrapper for `start_container` that accepts `Arc<TestManager>`.
    pub async fn start_container_arc(
        manager: &Arc<TestManager>,
        image: &str,
        platform: Platform,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::start_container(manager.as_ref(), image, platform).await
    }
}
