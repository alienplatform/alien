//! Interactive commands on Local, through the same binding and conformance cases the cloud
//! backends run.
//!
//! `cargo test -p alien-local --test sandbox_interactive -- --ignored --nocapture`

use std::sync::Arc;

use alien_bindings::providers::sandbox::local::LocalSandbox;
use alien_bindings::traits::{CreateSandboxRequest, Sandbox};
use alien_core::bindings::{BindingValue, LocalSandboxBinding};
use alien_local::{LocalSandboxManager, SandboxEgressMode, SandboxRoute, SandboxSessionConfig};
use alien_sandbox_conformance::interactive::{run_cases, CaseVerdict};
use tempfile::TempDir;

/// Has `python3`, so the REPL case runs rather than being skipped.
const IMAGE: &str = "python:3.12-slim";
const SANDBOX: &str = "interactive-a";

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn every_interactive_conformance_case_passes_on_local() {
    let dir = TempDir::new().expect("temp dir");
    let manager = Arc::new(
        LocalSandboxManager::new(dir.path().to_path_buf()).expect("Docker must be reachable"),
    );
    manager.reap(SANDBOX).await.expect("clean slate");
    SandboxRoute::remove(SANDBOX).await;

    let route = SandboxRoute::ensure(
        manager,
        SANDBOX,
        SandboxSessionConfig {
            image: IMAGE.to_string(),
            cpu_cores: 1.0,
            memory_bytes: 536_870_912,
            pids_limit: Some(128),
            scratch_bytes: 67_108_864,
            egress: SandboxEgressMode::Deny,
            preview_ports: Vec::new(),
            env: Default::default(),
        },
    )
    .await
    .expect("route binds on loopback");

    let sandbox: Arc<dyn Sandbox> = Arc::new(
        LocalSandbox::new(
            "code",
            &LocalSandboxBinding {
                default_policy: None,
                egress: None,
                manager_url: BindingValue::value(route.base_url.clone()),
                sandbox_key: BindingValue::value(SANDBOX.to_string()),
                token_path: BindingValue::value(route.token_path.display().to_string()),
            },
        )
        .await
        .expect("the binding loads"),
    );
    let instance = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .expect("a session starts");

    let results = run_cases(Arc::clone(&sandbox), &instance.sandbox_id).await;
    let _ = sandbox.terminate(&instance.sandbox_id).await;
    SandboxRoute::remove(SANDBOX).await;

    println!(
        "{}",
        serde_json::to_string_pretty(&results).expect("results serialize")
    );
    let failed: Vec<_> = results
        .iter()
        .filter(|case| case.verdict == CaseVerdict::Fail)
        .collect();
    assert!(failed.is_empty(), "failed cases: {failed:#?}");
    assert!(
        results.iter().all(|case| case.verdict == CaseVerdict::Pass),
        "every case runs on this image, none skipped: {results:#?}"
    );
}

/// The Node and Python packages drive an interactive command through their native extensions
/// against this real daemon, so each language's thin wrapper is proven against the Rust one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker, built Node addon and Python wheel installed at ALIEN_TEST_PYTHON"]
async fn language_bindings_drive_interactive_commands_against_local() {
    const LANGUAGE_SANDBOX: &str = "interactive-lang";
    let dir = TempDir::new().expect("temp dir");
    let manager = Arc::new(
        LocalSandboxManager::new(dir.path().to_path_buf()).expect("Docker must be reachable"),
    );
    manager.reap(LANGUAGE_SANDBOX).await.expect("clean slate");
    SandboxRoute::remove(LANGUAGE_SANDBOX).await;
    let route = SandboxRoute::ensure(
        Arc::clone(&manager),
        LANGUAGE_SANDBOX,
        SandboxSessionConfig {
            image: IMAGE.to_string(),
            cpu_cores: 0.5,
            memory_bytes: 268_435_456,
            pids_limit: Some(64),
            scratch_bytes: 16_777_216,
            egress: SandboxEgressMode::Deny,
            preview_ports: Vec::new(),
            env: Default::default(),
        },
    )
    .await
    .expect("route binds on loopback");
    let binding = serde_json::json!({
        "service": "sandbox-local", "managerUrl": route.base_url,
        "sandboxKey": LANGUAGE_SANDBOX, "tokenPath": route.token_path,
        "egress": {"mode":"deny"},
    })
    .to_string();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let python = std::env::var("ALIEN_TEST_PYTHON")
        .expect("set ALIEN_TEST_PYTHON to the wheel's interpreter");
    for (program, script) in [
        (
            "node".to_string(),
            "crates/alien-local/tests/fixtures/interactive-bindings.mjs",
        ),
        (
            python,
            "crates/alien-local/tests/fixtures/interactive_bindings.py",
        ),
    ] {
        let output = tokio::process::Command::new(&program)
            .arg(root.join(script))
            .env("ALIEN_DEPLOYMENT_TYPE", "local")
            .env("ALIEN_INTERACTIVE_TEST_BINDING", &binding)
            .output()
            .await
            .expect("language process starts");
        assert!(
            output.status.success(),
            "{program}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(
        manager
            .list_sessions(LANGUAGE_SANDBOX)
            .await
            .expect("lists")
            .is_empty(),
        "language tests must terminate their real containers"
    );
    SandboxRoute::remove(LANGUAGE_SANDBOX).await;
}
