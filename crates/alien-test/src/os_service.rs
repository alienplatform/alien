//! E2E harness for the os-service self-update flow: a real `alien-launcher`
//! supervising a real `alien-operator` (built with `test-hooks`) against an
//! in-process standalone manager, with release manifests served from a temp
//! dir and artifacts from a local HTTP server.
//!
//! Version identity trick: every "operator artifact" is a tiny wrapper script
//! that exports `ALIEN_OPERATOR_FAKE_VERSION=<v>` (honored by `test-hooks`
//! debug builds only) and `exec`s the ONE compiled operator binary — so a
//! single build impersonates any version, each artifact has a distinct
//! sha256, and per-version behavior (e.g. "never becomes ready") is a
//! one-line script change.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::manager::TestManager;

/// Served artifacts: URL path → (bytes, hit counter).
type ArtifactMap = Arc<Mutex<HashMap<String, (Vec<u8>, Arc<AtomicU32>)>>>;

pub struct OsServiceRig {
    pub manager: TestManager,
    pub deployment_id: String,
    pub data_dir: tempfile::TempDir,
    releases_dir: tempfile::TempDir,
    artifacts: ArtifactMap,
    artifact_addr: SocketAddr,
    operator_binary: PathBuf,
    launcher_binary: PathBuf,
    pub health_port: u16,
    launcher: Option<std::process::Child>,
    /// Env the launcher (and by inheritance the operator) runs with.
    launcher_env: Vec<(String, String)>,
}

impl OsServiceRig {
    /// Boot the full rig: manager + deployment + artifact server + version
    /// store seeded with `initial_version` + a running launcher.
    pub async fn start(initial_version: &str) -> anyhow::Result<Self> {
        let operator_binary = find_operator_binary()?;
        let launcher_binary = find_launcher_binary()?;

        // Artifact server.
        let artifacts: ArtifactMap = Arc::new(Mutex::new(HashMap::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let artifact_addr = listener.local_addr()?;
        tokio::spawn(serve_artifacts(listener, artifacts.clone()));

        // Manager with the manifest base pointing at our temp releases dir.
        let releases_dir = tempfile::tempdir()?;
        let manager = TestManager::start_with_releases_url(
            releases_dir.path().to_string_lossy().into_owned(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("manager start failed: {e}"))?;

        // A deployment group is required before a deployment (the admin token
        // is workspace-scoped). Create one, then a deployment in it — the
        // create endpoint returns a deployment-scoped sync token.
        let group: serde_json::Value = post_json(
            &manager,
            "/v1/deployment-groups",
            serde_json::json!({ "name": "e2e-os-service" }),
        )
        .await
        .context("create deployment group")?;
        let group_id = group["id"]
            .as_str()
            .context("deployment group id in response")?
            .to_string();

        let created = post_json(
            &manager,
            "/v1/deployments",
            serde_json::json!({
                "name": "e2e-os-service",
                "platform": "local",
                "deploymentGroupId": group_id,
            }),
        )
        .await
        .context("create deployment")?;
        let deployment_id = created["deployment"]["id"]
            .as_str()
            .context("deployment id in create response")?
            .to_string();
        let token = created["token"]
            .as_str()
            .context("token in create response")?
            .to_string();

        // Data dir: secrets + the version store seeded with the initial version.
        // The operator rejects secret files that are group/world-accessible
        // (argv/perms hardening), so both must be 0600.
        let data_dir = tempfile::tempdir()?;
        let sync_token_path = data_dir.path().join("sync-token");
        write_secret(&sync_token_path, token.as_bytes())?;
        let encryption_key_path = data_dir.path().join("encryption-key");
        write_secret(&encryption_key_path, "0123456789abcdef".repeat(4).as_bytes())?;

        let rig_partial = Self {
            manager,
            deployment_id,
            data_dir,
            releases_dir,
            artifacts,
            artifact_addr,
            operator_binary,
            launcher_binary,
            health_port: free_port(),
            launcher: None,
            launcher_env: Vec::new(),
        };

        let initial_script = rig_partial.wrapper_script(initial_version, &[]);
        let version_dir = rig_partial.data_dir.path().join("versions").join(initial_version);
        std::fs::create_dir_all(&version_dir)?;
        write_executable(&version_dir.join("alien-operator"), &initial_script)?;
        for name in ["current", "last-stable"] {
            std::os::unix::fs::symlink(
                Path::new("versions").join(initial_version),
                rig_partial.data_dir.path().join(name),
            )?;
        }

        let mut rig = rig_partial;
        rig.launcher_env = vec![
            ("PLATFORM".into(), "local".into()),
            ("SYNC_URL".into(), rig.manager.url.clone()),
            (
                "SYNC_TOKEN_FILE".into(),
                sync_token_path.to_string_lossy().into_owned(),
            ),
            (
                "DATA_DIR".into(),
                rig.data_dir.path().to_string_lossy().into_owned(),
            ),
            (
                "OPERATOR_ENCRYPTION_KEY_FILE".into(),
                encryption_key_path.to_string_lossy().into_owned(),
            ),
            ("DEPLOYMENT_ID".into(), rig.deployment_id.clone()),
            // Fast loops for the test.
            ("SYNC_INTERVAL".into(), "1".into()),
            ("RUST_LOG".into(), "info".into()),
        ];
        rig.spawn_launcher()?;
        Ok(rig)
    }

    /// The wrapper-script "artifact" for a version. `extra_env` lines let a
    /// version misbehave (e.g. a SYNC_URL blackhole = never becomes ready).
    /// `exec ... || exit 1` keeps a broken exec an ordinary crash.
    pub fn wrapper_script(&self, version: &str, extra_env: &[(&str, &str)]) -> Vec<u8> {
        let mut script = String::from("#!/bin/sh\n");
        script.push_str(&format!("export ALIEN_OPERATOR_FAKE_VERSION={version}\n"));
        for (key, value) in extra_env {
            script.push_str(&format!("export {key}={value}\n"));
        }
        script.push_str(&format!("exec {} \"$@\"\n", self.operator_binary.display()));
        script.into_bytes()
    }

    /// A script that exits immediately — a "broken build" artifact.
    pub fn broken_script(&self) -> Vec<u8> {
        b"#!/bin/sh\nexit 1\n".to_vec()
    }

    /// Publish a release: serve the artifact over HTTP and write the
    /// per-version `manifest.json` the manager resolves targets from.
    /// Returns the artifact's hit counter.
    pub fn publish_release(
        &self,
        version: &str,
        artifact: Vec<u8>,
        min_launcher_version: &str,
    ) -> anyhow::Result<Arc<AtomicU32>> {
        let sha256 = {
            use sha2::Digest;
            format!("{:x}", sha2::Sha256::digest(&artifact))
        };
        let path = format!("/artifacts/operator-{version}");
        let hits = Arc::new(AtomicU32::new(0));
        self.artifacts
            .lock()
            .expect("artifact map lock")
            .insert(path.clone(), (artifact, hits.clone()));

        let platform_key = format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH);
        let manifest = serde_json::json!({
            "version": version,
            "minLauncherVersion": min_launcher_version,
            "artifacts": {
                platform_key: {
                    "url": format!("http://{}{path}", self.artifact_addr),
                    "sha256": sha256,
                }
            }
        });
        let version_dir = self.releases_dir.path().join(version);
        std::fs::create_dir_all(&version_dir)?;
        std::fs::write(
            version_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Ok(hits)
    }

    /// Swap the SERVED bytes for a published version without touching its
    /// manifest — the downloaded artifact then fails the digest check.
    pub fn replace_artifact(
        &self,
        version: &str,
        artifact: Vec<u8>,
    ) -> anyhow::Result<Arc<AtomicU32>> {
        let path = format!("/artifacts/operator-{version}");
        let hits = Arc::new(AtomicU32::new(0));
        self.artifacts
            .lock()
            .expect("artifact map lock")
            .insert(path, (artifact, hits.clone()));
        Ok(hits)
    }

    /// Pin (or clear) the target operator version via the admin API.
    pub async fn pin(&self, version: Option<&str>) -> anyhow::Result<()> {
        let response = self
            .manager
            .http_client()
            .put(format!(
                "{}/v1/deployments/{}/target-operator-version",
                self.manager.url, self.deployment_id
            ))
            .json(&serde_json::json!({ "targetOperatorVersion": version }))
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "pin failed: {} — {}",
            response.status(),
            response.text().await.unwrap_or_default()
        );
        Ok(())
    }

    /// The operator version + launcher version currently on the deployment row.
    pub async fn reported_versions(&self) -> anyhow::Result<(Option<String>, Option<String>)> {
        let row: serde_json::Value = self
            .manager
            .http_client()
            .get(format!(
                "{}/v1/deployments/{}",
                self.manager.url, self.deployment_id
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let get = |key: &str| row[key].as_str().map(str::to_string);
        Ok((get("operatorVersion"), get("launcherVersion")))
    }

    /// Poll until the deployment row reports `version` (or fail loudly with
    /// the last observation).
    pub async fn wait_for_reported_version(
        &self,
        version: &str,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        while Instant::now() < deadline {
            last = self.reported_versions().await?.0;
            if last.as_deref() == Some(version) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        anyhow::bail!("deployment never reported {version}; last seen {last:?}")
    }

    /// Poll until the launcher has fully PROMOTED `version`: current +
    /// last-stable both point at it and the transient markers are cleared.
    /// The manager reports the new operator_version the instant the new
    /// operator syncs, which is up to one probe-interval BEFORE the launcher
    /// finishes promote cleanup — so store-state assertions must wait for
    /// this, not just for the reported version.
    pub async fn wait_for_promote(&self, version: &str, timeout: Duration) -> anyhow::Result<()> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.current_version().as_deref() == Some(version)
                && self.last_stable_version().as_deref() == Some(version)
                && !self.pending_exists()
                && !self.probation_exists()
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        anyhow::bail!(
            "promote to {version} never completed: current={:?} last_stable={:?} pending={} probation={}",
            self.current_version(),
            self.last_stable_version(),
            self.pending_exists(),
            self.probation_exists(),
        )
    }

    // -- workload + orphan inspection (for the self-update orphan guard) -----

    /// Poll until the deployment row reports the given `status`. Needed before
    /// `deploy_test_app_workload`: the manager only assigns a new release's
    /// `desired_release_id` to deployments already in `running` (see
    /// `set_desired_release`), and a freshly-synced operator reaches `running`
    /// a beat after it first reports its version.
    pub async fn wait_for_status(&self, status: &str, timeout: Duration) -> anyhow::Result<()> {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        while Instant::now() < deadline {
            let row: serde_json::Value = self
                .manager
                .http_client()
                .get(format!(
                    "{}/v1/deployments/{}",
                    self.manager.url, self.deployment_id
                ))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            last = row["status"].as_str().map(str::to_string);
            if last.as_deref() == Some(status) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        anyhow::bail!("deployment never reached status {status}; last seen {last:?}")
    }

    /// Deploy a real user workload so the operator spawns an observable app
    /// child process. Builds `alien-test-app` (a Rust SDK worker) into a local
    /// OCI via `alien-build` (cargo only — no bun/docker), then `POST
    /// /v1/releases` with a `local` stack running it; the manager auto-assigns
    /// it as the deployment's desired release, so the operator extracts + spawns
    /// the app on its next sync. Returns the OCI build dir — the caller MUST
    /// keep it alive (the operator reads the OCI from this path) for the test.
    pub async fn deploy_test_app_workload(&self) -> anyhow::Result<tempfile::TempDir> {
        use alien_build::settings::{BuildSettings, PlatformBuildSettings};
        use alien_core::permissions::{PermissionProfile, PermissionsConfig};
        use alien_core::{
            BinaryTarget, ResourceLifecycle, Stack, ToolchainConfig, Worker, WorkerCode,
        };

        let app_src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .context("crates dir")?
            .join("alien-test-app");
        anyhow::ensure!(
            app_src.is_dir(),
            "alien-test-app source not found at {}",
            app_src.display()
        );

        let worker = Worker::new("app".to_string())
            .code(WorkerCode::Source {
                src: app_src.to_string_lossy().into_owned(),
                toolchain: ToolchainConfig::Rust {
                    binary_name: "alien-test-app".to_string(),
                },
            })
            .memory_mb(512)
            .timeout_seconds(60)
            .environment(HashMap::new())
            .permissions("execution".to_string())
            .build();
        let permissions = PermissionsConfig {
            profiles: [("execution".to_string(), PermissionProfile::default())]
                .into_iter()
                .collect(),
            management: Default::default(),
        };
        let stack = Stack::new("orphan-guard".to_string())
            .add(worker, ResourceLifecycle::Live)
            .permissions(permissions)
            .build();

        let build_dir = tempfile::tempdir()?;
        let settings = BuildSettings {
            output_directory: build_dir.path().to_string_lossy().into_owned(),
            platform: PlatformBuildSettings::Local {},
            targets: Some(vec![BinaryTarget::current_os()]),
            cache_url: None,
            override_base_image: None,
            debug_mode: false,
        };
        let built = alien_build::build_stack(stack, &settings)
            .await
            .context("build alien-test-app into a local OCI")?;

        // The release's `local` stack is the built stack — its Worker.code is now
        // a local OCI Image path the operator extracts + runs.
        let stack_json = serde_json::to_value(&built).context("serialize built stack")?;
        let resp = post_json(
            &self.manager,
            "/v1/releases",
            serde_json::json!({ "stack": { "local": stack_json }, "projectId": "default" }),
        )
        .await?;
        anyhow::ensure!(
            resp.get("id").and_then(|v| v.as_str()).is_some(),
            "release response missing id: {resp}"
        );
        Ok(build_dir)
    }

    /// PIDs of the workload app processes — direct children of the operator (the
    /// operator runs the worker runtime in-process, which `cmd.spawn()`s the app,
    /// so the app is the operator's child / the launcher's grandchild).
    pub fn app_pids(&mut self) -> anyhow::Result<Vec<u32>> {
        let operator_pid = self.operator_pid()?;
        Ok(child_pids(operator_pid))
    }

    /// Poll until exactly one app child runs under the current operator.
    pub async fn wait_for_one_app(&mut self, timeout: Duration) -> anyhow::Result<u32> {
        let deadline = Instant::now() + timeout;
        let mut last = Vec::new();
        while Instant::now() < deadline {
            last = self.app_pids().unwrap_or_default();
            if last.len() == 1 {
                return Ok(last[0]);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        anyhow::bail!("never converged to exactly one app child; last seen {last:?}")
    }

    /// True if `pid` is still alive AND reparented to init (ppid == 1) — the
    /// orphaned-workload signature. A terminated process returns false (gone,
    /// not orphaned).
    pub fn is_orphaned(&self, pid: u32) -> bool {
        ppid_of(pid) == Some(1)
    }

    // -- launcher process control ------------------------------------------

    pub fn spawn_launcher(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(self.launcher.is_none(), "launcher already running");
        let child = std::process::Command::new(&self.launcher_binary)
            .args([
                "--data-dir",
                &self.data_dir.path().to_string_lossy(),
                "--probation-secs",
                "20",
                "--health-port",
                &self.health_port.to_string(),
            ])
            .envs(self.launcher_env.iter().map(|(k, v)| (k, v)))
            .spawn()
            .context("spawning alien-launcher")?;
        self.launcher = Some(child);
        Ok(())
    }

    /// SIGKILL the launcher (simulating a crash — no graceful anything).
    pub fn kill_launcher(&mut self) -> anyhow::Result<u32> {
        let mut child = self.launcher.take().context("no launcher running")?;
        let pid = child.id();
        child.kill().context("SIGKILL launcher")?;
        child.wait().context("reap launcher")?;
        Ok(pid)
    }

    /// Pid of the operator child the launcher spawned (via the process table).
    pub fn operator_pid(&mut self) -> anyhow::Result<u32> {
        let launcher_pid = self
            .launcher
            .as_ref()
            .context("no launcher running")?
            .id();
        let output = std::process::Command::new("pgrep")
            .args(["-P", &launcher_pid.to_string()])
            .output()
            .context("pgrep")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .split_whitespace()
            .next()
            .and_then(|pid| pid.parse().ok())
            .context("launcher has no child yet")
    }

    // -- store inspection ----------------------------------------------------

    pub fn current_version(&self) -> Option<String> {
        pointer_target(&self.data_dir.path().join("current"))
    }

    pub fn last_stable_version(&self) -> Option<String> {
        pointer_target(&self.data_dir.path().join("last-stable"))
    }

    pub fn pending_exists(&self) -> bool {
        self.data_dir.path().join("pending.json").exists()
    }

    pub fn probation_exists(&self) -> bool {
        self.data_dir.path().join("probation.json").exists()
    }

    pub fn failure_record(&self, version: &str) -> Option<alien_core::self_update::FailureRecord> {
        alien_core::self_update::read_json(
            &self
                .data_dir
                .path()
                .join("failed")
                .join(format!("{version}.json")),
        )
        .ok()
        .flatten()
    }

    /// Best-effort teardown: kill the launcher (its process group dies with
    /// it via the supervisor's own mechanisms + kill fallback).
    pub async fn shutdown(mut self) {
        if let Some(mut child) = self.launcher.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.manager.stop().await;
    }
}

/// POST JSON to the manager with the admin token; surface the body on error
/// (the SDK/`error_for_status` hides it, which makes 400s undebuggable).
async fn post_json(
    manager: &TestManager,
    path: &str,
    body: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let response = manager
        .http_client()
        .post(format!("{}{path}", manager.url))
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    anyhow::ensure!(status.is_success(), "POST {path} → {status}: {text}");
    Ok(serde_json::from_str(&text).unwrap_or(serde_json::Value::Null))
}

fn pointer_target(path: &Path) -> Option<String> {
    std::fs::read_link(path)
        .ok()?
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// Direct children of `pid` (via `pgrep -P`).
fn child_pids(pid: u32) -> Vec<u32> {
    std::process::Command::new("pgrep")
        .args(["-P", &pid.to_string()])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Parent PID of `pid`, or None if the process no longer exists.
fn ppid_of(pid: u32) -> Option<u32> {
    let out = std::process::Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind :0")
        .local_addr()
        .expect("local addr")
        .port()
}

fn write_secret(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn write_executable(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn find_operator_binary() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("ALIEN_OPERATOR_BINARY") {
        let path = PathBuf::from(path);
        anyhow::ensure!(path.is_file(), "ALIEN_OPERATOR_BINARY does not exist");
        return Ok(path);
    }
    workspace_binary("alien-operator")
}

fn find_launcher_binary() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("ALIEN_LAUNCHER_BINARY") {
        let path = PathBuf::from(path);
        anyhow::ensure!(path.is_file(), "ALIEN_LAUNCHER_BINARY does not exist");
        return Ok(path);
    }
    workspace_binary("alien-launcher")
}

fn workspace_binary(name: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug")
        .join(name);
    anyhow::ensure!(
        path.is_file(),
        "{name} not found at {} — build it first (the E2E CI job does) or set the \
         ALIEN_{}_BINARY env var",
        path.display(),
        name.trim_start_matches("alien-").to_uppercase()
    );
    Ok(path)
}

/// Minimal static-artifact HTTP server: GET <path> → mapped bytes + hit count.
async fn serve_artifacts(listener: tokio::net::TcpListener, artifacts: ArtifactMap) {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let artifacts = artifacts.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_string();
            let hit = artifacts.lock().expect("artifact map lock").get(&path).map(
                |(bytes, hits)| {
                    hits.fetch_add(1, Ordering::SeqCst);
                    bytes.clone()
                },
            );
            let response = match hit {
                Some(bytes) => {
                    let mut response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        bytes.len()
                    )
                    .into_bytes();
                    response.extend_from_slice(&bytes);
                    response
                }
                None => b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    .to_vec(),
            };
            let _ = stream.write_all(&response).await;
            let _ = stream.shutdown().await;
        });
    }
}
