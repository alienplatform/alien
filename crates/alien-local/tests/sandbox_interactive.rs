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
