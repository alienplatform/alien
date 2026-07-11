//! Integration tests for runtime-less local Daemon supervision.
//!
//! These prove the ALIEN-226 end state for local Daemons *without* needing Docker or a real
//! OCI build: the app binary is spawned as a DIRECT child of the supervisor (no runtime
//! wrapper), the supervisor captures the app's output, and the child's environment is the
//! resolved one — plain bindings and receiver config present, the vault-load marker
//! (`ALIEN_SECRETS`) and worker-runtime signals (`ALIEN_TRANSPORT`,
//! `ALIEN_WORKER_GRPC_ADDRESS`, `ALIEN_BINDINGS_GRPC_ADDRESS`) absent.
//!
//! The "app" is a tiny `/bin/sh` script that dumps its parent pid and environment to a file,
//! then blocks so the supervisor sees a long-running process. This is a faithful stand-in for
//! the real app process: the manager launches it exactly as it launches a compiled entrypoint.
//!
//! Unix-only: the harness relies on `$PPID` and a POSIX shell.
#![cfg(unix)]

use alien_local::LocalBindingsProvider;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;

/// Seeds a daemon "extracted image" directory with a shell-script entrypoint and the metadata the
/// manager reads at start. The script records its parent pid and full environment to `out_file`,
/// then sleeps so the supervisor observes a running process.
fn seed_script_daemon(state_dir: &Path, daemon_id: &str, out_file: &Path) -> PathBuf {
    let daemon_dir = state_dir.join("daemons").join(daemon_id);
    std::fs::create_dir_all(&daemon_dir).unwrap();

    let script_path = daemon_dir.join("run.sh");
    let script = format!(
        "#!/bin/sh\n\
         {{\n\
           echo \"PPID=$PPID\"\n\
           env\n\
         }} > '{out}'\n\
         sleep 60\n",
        out = out_file.display()
    );
    std::fs::write(&script_path, script).unwrap();

    // WorkerMetadata is (de)serialized with its Rust field names (no rename). Only the entrypoint
    // command and paths matter here; env/port are set live at start_daemon.
    let metadata = serde_json::json!({
        "worker_id": daemon_id,
        "extracted_path": daemon_dir.to_str().unwrap(),
        "env_vars": {},
        "runtime_command": ["/bin/sh", script_path.to_str().unwrap()],
        "working_dir": null,
        "transport_port": null,
        "runtime_only_binding_names": [],
    });
    std::fs::write(
        daemon_dir.join("metadata.json"),
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();

    script_path
}

/// Waits until `path` exists and is non-empty, then returns its contents.
async fn read_when_ready(path: &Path, timeout: Duration) -> String {
    let start = std::time::Instant::now();
    loop {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if !contents.is_empty() {
                return contents;
            }
        }
        if start.elapsed() > timeout {
            panic!("daemon output file never appeared at {}", path.display());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Gives the manager's spawned monitor task time to run its one-time startup recovery scan (over
/// still-empty state) and park on its polling interval, so it won't later re-adopt a daemon from
/// preserved metadata mid-test. The scan over empty/absent dirs is near-instant once scheduled.
async fn settle_initial_recovery() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

fn parse_env(dump: &str) -> HashMap<String, String> {
    dump.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The daemon app runs as a direct child of the supervisor and its environment is the resolved
/// one — receiver config and bindings present, the vault-load marker and worker-runtime transport
/// signals absent. This single test covers the ALIEN-226 done-when checks a Daemon can prove
/// without Docker: process-tree parentage and the local Daemon env audit.
#[tokio::test]
async fn daemon_runs_as_direct_child_with_resolved_env() {
    let temp = TempDir::new().unwrap();
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    // Create the provider (and its recovery monitor) BEFORE seeding metadata, mirroring the real
    // flow where extraction writes metadata after the manager exists. Seeding first would let the
    // one-time startup recovery scan auto-start the daemon out from under the test.
    let provider = LocalBindingsProvider::new(&state_dir).unwrap();
    let manager = provider.worker_manager();
    // Let the monitor's initial (empty) recovery scan complete before any metadata exists, so it
    // can't later re-adopt this daemon from the metadata `stop` intentionally preserves.
    settle_initial_recovery().await;

    let out_file = temp.path().join("daemon-env.txt");
    seed_script_daemon(&state_dir, "gateway", &out_file);

    // The controller resolves bindings + receiver config + secrets into plain env vars. We pass a
    // representative set, including an `ALIEN_SECRETS` marker the supervisor must strip.
    let mut env_vars = HashMap::new();
    env_vars.insert(
        "ALIEN_DATA_BINDING".to_string(),
        r#"{"service":"local","path":"/mnt/storage/data"}"#.to_string(),
    );
    env_vars.insert(
        "ALIEN_COMMANDS_URL".to_string(),
        "http://localhost:9999/v1".to_string(),
    );
    env_vars.insert("ALIEN_DEPLOYMENT_ID".to_string(), "dep-1".to_string());
    env_vars.insert("MY_SECRET".to_string(), "s3cr3t-value".to_string());
    // Markers that must never reach the app on the local platform:
    env_vars.insert("ALIEN_SECRETS".to_string(), "vault://ignored".to_string());

    manager
        .start_daemon("gateway", env_vars, Vec::new(), None)
        .await
        .expect("daemon should start under direct supervision");

    assert!(manager.is_daemon_running("gateway").await);

    let child_pid = manager
        .daemon_pid("gateway")
        .await
        .expect("supervisor must know the app process pid");

    let dump = read_when_ready(&out_file, Duration::from_secs(10)).await;
    let env = parse_env(&dump);

    // Process tree: the app's parent is THIS supervisor process, i.e. the app is a direct child
    // with no wrapper process in between.
    let reported_ppid: u32 = dump
        .lines()
        .find_map(|line| line.strip_prefix("PPID="))
        .and_then(|value| value.trim().parse().ok())
        .expect("script should report its parent pid");
    assert_eq!(
        reported_ppid,
        std::process::id(),
        "daemon app must be a direct child of the supervisor process"
    );
    assert!(child_pid > 0, "supervisor should track a real pid");

    // Env audit: no vault-load marker, no worker-runtime transport/gRPC signals.
    for forbidden in [
        "ALIEN_SECRETS",
        "ALIEN_TRANSPORT",
        "ALIEN_WORKER_GRPC_ADDRESS",
        "ALIEN_BINDINGS_GRPC_ADDRESS",
    ] {
        assert!(
            !env.contains_key(forbidden),
            "local Daemon env must not contain {forbidden}; got keys: {:?}",
            env.keys().collect::<Vec<_>>()
        );
    }

    // Resolved values the app is meant to see are present and plain.
    assert_eq!(
        env.get("ALIEN_DATA_BINDING").map(String::as_str),
        Some(r#"{"service":"local","path":"/mnt/storage/data"}"#)
    );
    assert_eq!(
        env.get("ALIEN_COMMANDS_URL").map(String::as_str),
        Some("http://localhost:9999/v1"),
        "command receiver config must flow to a local Daemon"
    );
    assert_eq!(
        env.get("MY_SECRET").map(String::as_str),
        Some("s3cr3t-value"),
        "resolved secret must reach the app as a plain env var"
    );

    manager
        .stop_daemon("gateway")
        .await
        .expect("daemon should stop");
    assert!(!manager.is_daemon_running("gateway").await);
}

/// Stopping a daemon terminates its app process; a subsequent health check fails. Guards the
/// supervisor's shutdown path (kill child on shutdown signal) end to end.
#[tokio::test]
async fn stopping_daemon_kills_the_app_process() {
    let temp = TempDir::new().unwrap();
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    let provider = LocalBindingsProvider::new(&state_dir).unwrap();
    let manager = provider.worker_manager();
    settle_initial_recovery().await;

    let out_file = temp.path().join("stop-env.txt");
    seed_script_daemon(&state_dir, "worker-d", &out_file);

    manager
        .start_daemon("worker-d", HashMap::new(), Vec::new(), None)
        .await
        .expect("daemon should start");

    // Confirm it actually reached "running app" before we stop it.
    read_when_ready(&out_file, Duration::from_secs(10)).await;
    manager
        .check_daemon_health("worker-d")
        .await
        .expect("running daemon should be healthy");

    manager.stop_daemon("worker-d").await.expect("stop daemon");

    assert!(!manager.is_daemon_running("worker-d").await);
    assert!(
        manager.check_daemon_health("worker-d").await.is_err(),
        "health check must fail once the daemon is stopped"
    );
}
