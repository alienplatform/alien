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

/// Find the alien-agent binary.
///
/// Resolution order:
/// 1. `ALIEN_AGENT_BINARY` environment variable
/// 2. `target/debug/alien-agent` (or `.exe` on Windows) walking up from CWD
/// 3. `alien-agent` from PATH
fn find_agent_binary() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    // 1. Explicit env var (resolve relative paths against workspace root)
    if let Ok(path) = std::env::var("ALIEN_AGENT_BINARY") {
        let p = std::path::PathBuf::from(&path);
        if p.exists() {
            return Ok(p.canonicalize().unwrap_or(p));
        }
        // Try resolving relative to workspace root (CARGO_MANIFEST_DIR → ../../)
        if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
            let workspace_root = std::path::PathBuf::from(&manifest)
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf());
            if let Some(root) = workspace_root {
                let resolved = root.join(&path);
                if resolved.exists() {
                    return Ok(resolved);
                }
            }
        }
        return Err(format!(
            "ALIEN_AGENT_BINARY set to '{}' but file does not exist",
            path
        )
        .into());
    }

    // 2. Search upward for target/debug/alien-agent
    let binary_name = if cfg!(windows) {
        "alien-agent.exe"
    } else {
        "alien-agent"
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join("target").join("debug").join(binary_name);
        if candidate.exists() {
            return Ok(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    // 3. Fallback to PATH
    Ok(std::path::PathBuf::from(binary_name))
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
    /// Native process child handle (if started via `start_local_process`).
    pub child_process: Option<tokio::process::Child>,
    /// Temp data directory for native agent (cleaned up on drop).
    pub data_dir: Option<tempfile::TempDir>,
    /// Temp file holding the sync token (`--sync-token-file`); deleted on drop.
    /// The agent rejects plaintext token args because argv is visible in `ps`.
    pub sync_token_file: Option<tempfile::NamedTempFile>,
    /// Temp file holding the encryption key (`--encryption-key-file`); deleted on drop.
    pub encryption_key_file: Option<tempfile::NamedTempFile>,
    /// The platform this agent targets.
    pub platform: Platform,
    /// Whether the agent was installed as an OS service via `alien-deploy up`.
    pub installed_as_service: bool,
    /// Path to the alien-deploy binary (for service uninstall during cleanup).
    pub deploy_binary: Option<std::path::PathBuf>,
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
            "-e".to_string(),
            format!("SYNC_URL={}", manager.url),
            "-e".to_string(),
            format!("SYNC_TOKEN={}", manager.admin_token),
            "-e".to_string(),
            format!("PLATFORM={}", platform.as_str()),
            "-e".to_string(),
            format!("AGENT_ENCRYPTION_KEY={}", encryption_key),
            // Set ALIEN_API_KEY so the agent's preflight checks
            // (DnsTlsRequiredCheck, HorizonRequiredCheck) skip themselves.
            // Without this, cloud platform deployments with public ingress
            // are blocked because the agent thinks it's running standalone.
            "-e".to_string(),
            format!("ALIEN_API_KEY={}", manager.admin_token),
        ];

        // Inject target account credentials for cloud platforms so the agent
        // can deploy resources directly (pull model = no cross-account IAM).
        if let Some(cfg) = config {
            match platform {
                Platform::Aws => {
                    if let Some(ref target) = cfg.aws_target {
                        args.extend([
                            "-e".to_string(),
                            format!("AWS_ACCESS_KEY_ID={}", target.access_key_id),
                            "-e".to_string(),
                            format!("AWS_SECRET_ACCESS_KEY={}", target.secret_access_key),
                            "-e".to_string(),
                            format!("AWS_REGION={}", target.region),
                        ]);
                        if let Some(ref token) = target.session_token {
                            args.extend(["-e".to_string(), format!("AWS_SESSION_TOKEN={}", token)]);
                        }
                        if let Some(ref account_id) = target.account_id {
                            args.extend([
                                "-e".to_string(),
                                format!("AWS_ACCOUNT_ID={}", account_id),
                            ]);
                        }
                    }
                }
                Platform::Gcp => {
                    if let Some(ref target) = cfg.gcp_target {
                        args.extend([
                            "-e".to_string(),
                            format!("GCP_PROJECT_ID={}", target.project_id),
                            "-e".to_string(),
                            format!("GCP_REGION={}", target.region),
                        ]);
                        if let Some(ref creds) = target.credentials_json {
                            args.extend([
                                "-e".to_string(),
                                format!("GOOGLE_SERVICE_ACCOUNT_KEY={}", creds),
                            ]);
                        }
                    }
                }
                Platform::Azure => {
                    if let Some(ref target) = cfg.azure_target {
                        args.extend([
                            "-e".to_string(),
                            format!("AZURE_SUBSCRIPTION_ID={}", target.subscription_id),
                            "-e".to_string(),
                            format!("AZURE_TENANT_ID={}", target.tenant_id),
                            "-e".to_string(),
                            format!("AZURE_CLIENT_ID={}", target.client_id),
                            "-e".to_string(),
                            format!("AZURE_CLIENT_SECRET={}", target.client_secret),
                            "-e".to_string(),
                            format!("AZURE_REGION={}", target.region),
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
            child_process: None,
            data_dir: None,
            sync_token_file: None,
            encryption_key_file: None,
            platform,
            installed_as_service: false,
            deploy_binary: None,
        })
    }

    /// Start the alien-agent as a native foreground process for Local platform.
    ///
    /// Finds the alien-agent binary (via `ALIEN_AGENT_BINARY` env, or searches
    /// `target/debug/alien-agent`), then spawns it with the manager URL and token.
    pub async fn start_local_process(
        manager: &TestManager,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        info!(
            manager_url = %manager.url,
            "starting alien-agent as native process (local platform)"
        );

        let binary_path = find_agent_binary()?;
        info!(binary = %binary_path.display(), "found alien-agent binary");

        let encryption_key = generate_encryption_key();
        let data_dir = tempfile::tempdir()?;

        // The agent rejects `--sync-token`/`--encryption-key` (argv leak via
        // `ps` / `/proc/<pid>/cmdline`). Write each secret to a NamedTempFile
        // (deleted on drop — held on `TestAlienAgent` for child lifetime) and
        // pass `--*-file` paths.
        use std::io::Write;
        let mut sync_token_file = tempfile::NamedTempFile::new()?;
        sync_token_file.write_all(manager.admin_token.as_bytes())?;
        let mut encryption_key_file = tempfile::NamedTempFile::new()?;
        encryption_key_file.write_all(encryption_key.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                sync_token_file.path(),
                std::fs::Permissions::from_mode(0o600),
            );
            let _ = std::fs::set_permissions(
                encryption_key_file.path(),
                std::fs::Permissions::from_mode(0o600),
            );
        }

        let mut cmd = tokio::process::Command::new(&binary_path);
        cmd.arg("--platform")
            .arg("local")
            .arg("--sync-url")
            .arg(&manager.url)
            .arg("--sync-token-file")
            .arg(sync_token_file.path())
            .arg("--data-dir")
            .arg(data_dir.path())
            .arg("--encryption-key-file")
            .arg(encryption_key_file.path())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        // Set ALIEN_API_KEY so preflight checks skip themselves
        cmd.env("ALIEN_API_KEY", &manager.admin_token);

        // No credential injection needed — the agent pulls images through
        // the manager's /v2/ registry proxy, which handles upstream auth.

        let child = cmd.spawn().map_err(|e| {
            format!(
                "Failed to spawn alien-agent at {}: {}",
                binary_path.display(),
                e
            )
        })?;

        info!("alien-agent native process started");

        Ok(Self {
            container_id: None,
            helm_release: None,
            helm_namespace: None,
            kubeconfig: None,
            child_process: Some(child),
            data_dir: Some(data_dir),
            sync_token_file: Some(sync_token_file),
            encryption_key_file: Some(encryption_key_file),
            platform: Platform::Local,
            installed_as_service: false,
            deploy_binary: None,
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
            child_process: None,
            data_dir: None,
            sync_token_file: None,
            encryption_key_file: None,
            platform: Platform::Kubernetes,
            installed_as_service: false,
            deploy_binary: None,
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
    /// Create a `TestAlienAgent` representing an OS service installed by `alien-deploy up`.
    pub fn from_service(deploy_binary: Option<std::path::PathBuf>) -> Self {
        Self {
            container_id: None,
            helm_release: None,
            helm_namespace: None,
            kubeconfig: None,
            child_process: None,
            data_dir: None,
            sync_token_file: None,
            encryption_key_file: None,
            platform: Platform::Local,
            installed_as_service: true,
            deploy_binary,
        }
    }

    /// Best-effort cleanup: stop container, helm uninstall, kill native process,
    /// or uninstall OS service.
    /// Errors are logged but never propagated, making this safe for teardown paths.
    pub async fn cleanup(mut self) {
        if self.installed_as_service {
            info!("uninstalling alien-agent OS service");
            if let Some(ref deploy_bin) = self.deploy_binary {
                // On Linux/macOS, system service management requires root
                let output = if !cfg!(target_os = "windows") {
                    tokio::process::Command::new("sudo")
                        .arg(deploy_bin.as_os_str())
                        .args(["agent", "uninstall"])
                        .output()
                        .await
                } else {
                    tokio::process::Command::new(deploy_bin)
                        .args(["agent", "uninstall"])
                        .output()
                        .await
                };
                match output {
                    Ok(o) if !o.status.success() => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        tracing::warn!("cleanup: alien-deploy agent uninstall failed: {}", stderr);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "cleanup: failed to run alien-deploy agent uninstall");
                    }
                    _ => {
                        info!("alien-agent OS service uninstalled");
                    }
                }
            } else {
                tracing::warn!("cleanup: no deploy binary path, cannot uninstall service");
            }
        } else if let Some(ref mut child) = self.child_process {
            info!("stopping alien-agent native process");
            if let Err(e) = child.kill().await {
                tracing::warn!(error = %e, "cleanup: failed to kill agent native process");
            }
            // data_dir is cleaned up when TempDir is dropped
        } else if self.container_id.is_some() {
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

/// Collect logs from the alien-agent OS service for debugging.
///
/// Checks multiple sources: systemd journal / macOS log, service status,
/// panic.log, and agent.db existence in the data directory.
pub async fn collect_service_logs() -> String {
    let mut logs = String::new();

    // Default data dir used by alien-deploy agent install
    let data_dir = if cfg!(windows) {
        r"C:\ProgramData\alien-agent".to_string()
    } else {
        "/var/lib/alien-agent".to_string()
    };

    // 1. Check panic.log
    let panic_log = std::path::Path::new(&data_dir).join("panic.log");
    if panic_log.exists() {
        if let Ok(content) = std::fs::read_to_string(&panic_log) {
            logs.push_str(&format!("=== panic.log ===\n{}\n\n", content));
        }
    }

    // 2. List data directory contents
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        let files: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| {
                format!(
                    "  {} ({} bytes)",
                    e.file_name().to_string_lossy(),
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                )
            })
            .collect();
        logs.push_str(&format!(
            "=== data dir ({}) ===\n{}\n\n",
            data_dir,
            files.join("\n")
        ));
    } else {
        logs.push_str(&format!(
            "=== data dir ({}) does not exist ===\n\n",
            data_dir
        ));
    }

    // 3. OS-specific service logs
    //
    // service-manager 0.10 converts the label "dev.alien.agent" into the script
    // name "{organization}-{application}" = "alien-agent". Systemd uses this as
    // the unit name, while launchd uses the full reverse-DNS label.
    #[cfg(target_os = "linux")]
    {
        // Try both the systemd script name and the full label, since the
        // service-manager maps "dev.alien.agent" → "alien-agent.service".
        for unit_name in ["alien-agent", "dev.alien.agent"] {
            if let Ok(o) = tokio::process::Command::new("sudo")
                .args(["journalctl", "-u", unit_name, "--no-pager", "-n", "500"])
                .output()
                .await
            {
                let stdout = String::from_utf8_lossy(&o.stdout);
                if !stdout.contains("-- No entries --") {
                    logs.push_str(&format!(
                        "=== journalctl -u {} ===\n{}\n\n",
                        unit_name, stdout
                    ));
                }
            }

            if let Ok(o) = tokio::process::Command::new("sudo")
                .args(["systemctl", "status", unit_name])
                .output()
                .await
            {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                if !stderr.contains("could not be found") {
                    logs.push_str(&format!(
                        "=== systemctl status {} ===\n{}{}\n\n",
                        unit_name, stdout, stderr
                    ));
                }
            }
        }

        // Check the unit file (service-manager writes "alien-agent.service")
        for filename in ["alien-agent.service", "dev.alien.agent.service"] {
            let path = format!("/etc/systemd/system/{}", filename);
            if let Ok(o) = tokio::process::Command::new("sudo")
                .args(["cat", &path])
                .output()
                .await
            {
                if o.status.success() {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    logs.push_str(&format!("=== {} ===\n{}\n\n", path, stdout));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = tokio::process::Command::new("log")
            .args([
                "show",
                "--predicate",
                "process == \"alien-agent\"",
                "--last",
                "10m",
                "--style",
                "compact",
            ])
            .output()
            .await
        {
            let stdout = String::from_utf8_lossy(&o.stdout);
            logs.push_str(&format!("=== macOS log ===\n{}\n\n", stdout));
        }

        // Check launchctl status
        if let Ok(o) = tokio::process::Command::new("sudo")
            .args(["launchctl", "print", "system/dev.alien.agent"])
            .output()
            .await
        {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            logs.push_str(&format!(
                "=== launchctl print ===\n{}{}\n\n",
                stdout, stderr
            ));
        }

        // Check the plist file
        if let Ok(o) = tokio::process::Command::new("cat")
            .arg("/Library/LaunchDaemons/dev.alien.agent.plist")
            .output()
            .await
        {
            let stdout = String::from_utf8_lossy(&o.stdout);
            logs.push_str(&format!("=== plist file ===\n{}\n\n", stdout));
        }
    }

    #[cfg(target_os = "windows")]
    {
        logs.push_str("Windows service log collection not yet implemented\n");
    }

    if logs.is_empty() {
        "No service logs found".to_string()
    } else {
        logs
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

/// Get the status of a Docker container (e.g. "running", "exited").
pub async fn docker_container_status(container_id: &str) -> String {
    tokio::process::Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", container_id])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Get the combined stdout+stderr logs of a Docker container.
pub async fn docker_container_logs(container_id: &str) -> String {
    tokio::process::Command::new("docker")
        .args(["logs", "--tail", "200", container_id])
        .output()
        .await
        .map(|o| {
            format!(
                "stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            )
        })
        .unwrap_or_else(|e| format!("failed to get logs: {}", e))
}
