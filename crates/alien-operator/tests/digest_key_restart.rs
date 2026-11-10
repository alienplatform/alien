//! An Operator restart must not change the checksums saved in the deployment's state.
//! Each "restart" here is a new process (this test binary run again) on the same data directory.

use std::process::Command;

use alien_core::{
    EnvironmentVariable, EnvironmentVariableType, OtlpConfig, Platform, ResourceLifecycle, Stack,
    Worker, WorkerCode,
};
use alien_deployment::{environment_snapshot_hash, inject_monitoring_environment_variables};
use alien_infra::{environment_secret_plan, EnvSecretRotationTracker};
use alien_operator::db::OperatorDb;

const DB_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const CHILD_DIR: &str = "DIGEST_RESTART_CHILD_DIR";
const CHILD_TRACKER: &str = "DIGEST_RESTART_CHILD_TRACKER";
const CHILD_EPHEMERAL: &str = "DIGEST_RESTART_CHILD_EPHEMERAL";
const PREFIX: &str = "FINGERPRINT:";

fn secret(value: &str) -> Vec<EnvironmentVariable> {
    vec![EnvironmentVariable {
        name: "DB_PASSWORD".to_string(),
        value: value.to_string(),
        var_type: EnvironmentVariableType::Secret,
        target_resources: None,
    }]
}

fn runtime_secrets_hash() -> String {
    let worker = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .build();
    let mut stack = Stack::new("restart".to_string())
        .add(worker, ResourceLifecycle::Live)
        .build();
    let monitoring = OtlpConfig {
        logs_endpoint: "https://manager.test/v1/logs".to_string(),
        credentials: None,
        logs_auth_header: "authorization=Bearer logs-token".to_string(),
        metrics_endpoint: None,
        metrics_auth_header: None,
        resource_attributes: Default::default(),
    };
    inject_monitoring_environment_variables(&mut stack, &monitoring, Platform::Kubernetes).unwrap();
    let entry = stack.resources.get("api").unwrap();
    let worker = entry.config.downcast_ref::<Worker>().unwrap();
    worker.environment["ALIEN_RUNTIME_SECRETS"].clone()
}

/// What this process computes, as the saved state would hold it.
fn fingerprint(value: &str) -> serde_json::Value {
    let variables = secret(value);
    let plan = environment_secret_plan("web", "web", &variables)
        .unwrap()
        .unwrap();
    let mut tracker = EnvSecretRotationTracker::default();
    tracker.record(Some(&plan));
    serde_json::json!({
        "k8sChecksum": plan.checksum,
        "tracker": tracker,
        "snapshotHash": environment_snapshot_hash(&variables).unwrap(),
        "runtimeSecrets": runtime_secrets_hash(),
    })
}

async fn start_operator_process(data_dir: &str) {
    if std::env::var(CHILD_EPHEMERAL).is_ok() {
        alien_deployment::set_ephemeral_secrets_digest_key();
    } else {
        let db = OperatorDb::new(data_dir, DB_KEY).await.unwrap();
        db.install_digest_key().await.unwrap();
    }
}

/// Runs only as the child process of the tests below.
#[tokio::test]
async fn child_prints_fingerprint() {
    let Ok(dir) = std::env::var(CHILD_DIR) else {
        return;
    };
    start_operator_process(&dir).await;
    let mut out = fingerprint("hunter2-canary");
    if let Ok(tracker) = std::env::var(CHILD_TRACKER) {
        let tracker: EnvSecretRotationTracker = serde_json::from_str(&tracker).unwrap();
        out["driftedFromParentTracker"] = serde_json::json!([
            tracker
                .drifted("web", "web", &secret("hunter2-canary"))
                .unwrap(),
            tracker
                .drifted("web", "web", &secret("rotated-canary"))
                .unwrap(),
        ]);
    }
    println!("{PREFIX}{out}");
}

fn restart(dir: &str, tracker: Option<&serde_json::Value>, ephemeral: bool) -> serde_json::Value {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["child_prints_fingerprint", "--exact", "--nocapture"])
        .env(CHILD_DIR, dir);
    if let Some(tracker) = tracker {
        command.env(CHILD_TRACKER, tracker.to_string());
    }
    if ephemeral {
        command.env(CHILD_EPHEMERAL, "1");
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix(PREFIX))
        .unwrap_or_else(|| panic!("the child printed no fingerprint: {stdout}"));
    serde_json::from_str(line).unwrap()
}

#[tokio::test]
async fn a_restart_changes_no_checksum_and_a_changed_value_still_rolls() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().to_str().unwrap();

    // The first process saves its checksums, the way the deployment's state does.
    start_operator_process(dir).await;
    let saved = fingerprint("hunter2-canary");

    let restarted = restart(dir, Some(&saved["tracker"]), false);
    for field in ["k8sChecksum", "snapshotHash", "runtimeSecrets"] {
        assert_eq!(saved[field], restarted[field], "{field} changed on restart");
    }
    assert_eq!(
        restarted["driftedFromParentTracker"],
        serde_json::json!([false, true]),
        "unchanged values must not need an update and a changed value must"
    );

    // A process with a random key, which is what the Operator used to do, changes all of them.
    let random = restart(dir, None, true);
    for field in ["k8sChecksum", "snapshotHash", "runtimeSecrets"] {
        assert_ne!(
            saved[field], random[field],
            "{field} did not depend on the key"
        );
    }
}
