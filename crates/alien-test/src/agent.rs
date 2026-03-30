//! `TestAlienAgent` -- helpers for running alien-agent containers in pull mode.
//!
//! In pull mode the alien-agent runs inside a Docker container (or as a Helm
//! release in Kubernetes), connects to the manager, and executes deployments
//! locally. This module provides helpers to start and stop such agents during
//! E2E tests.

use std::sync::Arc;

use alien_core::Platform;
use tracing::info;

use crate::config::TestConfig;
use crate::manager::TestManager;

/// Default Docker label applied to test alien-agent containers so they can be
/// cleaned up afterwards.
pub const TEST_AGENT_LABEL: &str = "alien-test-agent=true";

/// Generate a random 64-character hex string for use as an encryption key.
fn generate_encryption_key() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    hex::encode(bytes)
}

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
    ///
    /// For cloud platforms (AWS/GCP/Azure), pass a `TestConfig` to inject
    /// target account credentials into the container so the agent can deploy
    /// resources directly.
    pub async fn start_container(
        manager: &TestManager,
        image: &str,
        platform: Platform,
        config: Option<&TestConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        info!(
            manager_url = %manager.url,
            %image,
            platform = %platform.as_str(),
            "starting alien-agent container"
        );

        let encryption_key = generate_encryption_key();

        let mut args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--label".to_string(),
            TEST_AGENT_LABEL.to_string(),
            // The agent needs to reach the manager on the host network
            "--network".to_string(),
            "host".to_string(),
            // Core agent configuration (env var names match clap `env = "..."` annotations)
            "-e".to_string(), format!("SYNC_URL={}", manager.url),
            "-e".to_string(), format!("SYNC_TOKEN={}", manager.admin_token),
            "-e".to_string(), format!("PLATFORM={}", platform.as_str()),
            "-e".to_string(), format!("AGENT_ENCRYPTION_KEY={}", encryption_key),
        ];

        // Inject target account credentials for cloud platforms so the agent
        // can deploy resources directly (pull model = no cross-account IAM).
        if let Some(cfg) = config {
            match platform {
                Platform::Aws => {
                    if let Some(ref target) = cfg.aws_target {
                        args.extend([
                            "-e".to_string(), format!("AWS_ACCESS_KEY_ID={}", target.access_key_id),
                            "-e".to_string(), format!("AWS_SECRET_ACCESS_KEY={}", target.secret_access_key),
                            "-e".to_string(), format!("AWS_REGION={}", target.region),
                        ]);
                        if let Some(ref token) = target.session_token {
                            args.extend(["-e".to_string(), format!("AWS_SESSION_TOKEN={}", token)]);
                        }
                        if let Some(ref account_id) = target.account_id {
                            args.extend(["-e".to_string(), format!("AWS_ACCOUNT_ID={}", account_id)]);
                        }
                    }
                }
                Platform::Gcp => {
                    if let Some(ref target) = cfg.gcp_target {
                        args.extend([
                            "-e".to_string(), format!("GCP_PROJECT_ID={}", target.project_id),
                            "-e".to_string(), format!("GCP_REGION={}", target.region),
                        ]);
                        if let Some(ref creds) = target.credentials_json {
                            args.extend(["-e".to_string(), format!("GOOGLE_SERVICE_ACCOUNT_KEY={}", creds)]);
                        }
                    }
                }
                Platform::Azure => {
                    if let Some(ref target) = cfg.azure_target {
                        args.extend([
                            "-e".to_string(), format!("AZURE_SUBSCRIPTION_ID={}", target.subscription_id),
                            "-e".to_string(), format!("AZURE_TENANT_ID={}", target.tenant_id),
                            "-e".to_string(), format!("AZURE_CLIENT_ID={}", target.client_id),
                            "-e".to_string(), format!("AZURE_CLIENT_SECRET={}", target.client_secret),
                            "-e".to_string(), format!("AZURE_REGION={}", target.region),
                        ]);
                    }
                }
                _ => {}
            }
        }

        args.push(image.to_string());

        let output = tokio::process::Command::new("docker")
            .args(&args)
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

        let encryption_key = generate_encryption_key();

        let mut cmd = tokio::process::Command::new("helm");
        cmd.args([
            "install",
            release_name,
            chart,
            "--namespace",
            namespace,
            "--create-namespace",
            "--set",
            &format!("syncUrl={}", manager.url),
            "--set",
            &format!("syncToken={}", manager.admin_token),
            "--set",
            &format!("encryptionKey={}", encryption_key),
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

        crate::cleanup::cleanup_helm_release(release, namespace, self.kubeconfig.as_deref()).await
    }
}

impl TestAlienAgent {
    /// Best-effort cleanup: stop container or helm uninstall. Errors are logged
    /// but never propagated, making this safe for teardown paths.
    pub async fn cleanup(self) {
        if self.container_id.is_some() {
            if let Err(e) = self.stop().await {
                tracing::warn!(error = %e, "cleanup: failed to stop agent container");
            }
        } else if self.helm_release.is_some() {
            if let Err(e) = self.helm_uninstall().await {
                tracing::warn!(error = %e, "cleanup: failed to uninstall agent helm release");
            }
        }
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
        config: Option<&TestConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::start_container(manager.as_ref(), image, platform, config).await
    }
}
