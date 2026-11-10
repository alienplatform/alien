//! Interactive commands on Local, through the same binding and conformance cases the cloud
//! backends run.
//!
//! `cargo test -p alien-local --test sandbox_interactive -- --ignored --nocapture`

use std::sync::Arc;

use alien_bindings::providers::sandbox::local::LocalSandbox;
use alien_bindings::traits::{
    CommandOutput, CreateSandboxRequest, InteractiveCommand, RunCommandRequest, Sandbox,
};
use alien_core::bindings::{BindingValue, LocalSandboxBinding};
use alien_local::{LocalSandboxManager, SandboxEgressMode, SandboxRoute, SandboxSessionConfig};
use alien_sandbox_conformance::interactive::{run_cases, CaseVerdict};
use futures::StreamExt;
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

/// One session on its own sandbox, through the same binding the cases use.
async fn local_session(sandbox_key: &str) -> (TempDir, Arc<dyn Sandbox>, String) {
    let dir = TempDir::new().expect("temp dir");
    let manager = Arc::new(
        LocalSandboxManager::new(dir.path().to_path_buf()).expect("Docker must be reachable"),
    );
    manager.reap(sandbox_key).await.expect("clean slate");
    SandboxRoute::remove(sandbox_key).await;
    let route = SandboxRoute::ensure(
        manager,
        sandbox_key,
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
    let sandbox: Arc<dyn Sandbox> = Arc::new(
        LocalSandbox::new(
            "code",
            &LocalSandboxBinding {
                default_policy: None,
                egress: None,
                manager_url: BindingValue::value(route.base_url.clone()),
                sandbox_key: BindingValue::value(sandbox_key.to_string()),
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
    (dir, sandbox, instance.sandbox_id)
}

fn sh(script: &str, timeout: std::time::Duration) -> RunCommandRequest {
    RunCommandRequest {
        command: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        cwd: None,
        env: Default::default(),
        timeout,
    }
}

/// Reads a command's stream to its end within `within`, returning its stdout and the terminal
/// item: the exit code, or the error's text.
async fn drain(
    command: &mut InteractiveCommand,
    within: std::time::Duration,
) -> (String, Result<i32, String>) {
    let deadline = tokio::time::Instant::now() + within;
    let mut stdout = Vec::new();
    loop {
        match tokio::time::timeout_at(deadline, command.output.next())
            .await
            .unwrap_or_else(|_| panic!("the stream did not end within {within:?}"))
        {
            None => panic!("the stream ended without a terminal item"),
            Some(Ok(CommandOutput::Stdout { data, .. })) => stdout.extend(data),
            Some(Ok(CommandOutput::Stderr { .. })) => {}
            Some(Ok(CommandOutput::Exit { code, .. })) => {
                return (String::from_utf8_lossy(&stdout).into_owned(), Ok(code))
            }
            Some(Err(error)) => {
                return (
                    String::from_utf8_lossy(&stdout).into_owned(),
                    Err(error.to_string()),
                )
            }
        }
    }
}

/// The command runs as the session's user and kills everything that user may signal. The shell
/// that enforces its deadline runs as root, so it survives, and the deadline holds on the first
/// stop: the stream ends in the timeout, not in a kill that could not be confirmed.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn a_command_cannot_kill_the_shell_that_enforces_its_deadline() {
    const KEY: &str = "interactive-sweep";
    let (_dir, sandbox, id) = local_session(KEY).await;
    let mut command = sandbox
        .run_interactive(
            &id,
            sh(
                "kill -KILL -1 2>/dev/null; echo swept; exec sleep 600",
                std::time::Duration::from_secs(3),
            ),
        )
        .await
        .expect("starts");
    let (stdout, terminal) = drain(&mut command, std::time::Duration::from_secs(30)).await;
    let _ = sandbox.terminate(&id).await;
    SandboxRoute::remove(KEY).await;

    assert_eq!(stdout, "swept\n");
    let error = terminal.expect_err("the deadline ends it");
    assert!(error.contains("timeoutExceeded"), "{error}");
}

/// A child the command moves to a session of its own escapes the group kill and holds the output
/// open. The deadline still holds: after the retried stops fail, the whole session is killed from
/// the host, and nothing runs in it afterwards.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn a_command_whose_child_escapes_its_group_ends_with_its_session() {
    const KEY: &str = "interactive-escape";
    let (_dir, sandbox, id) = local_session(KEY).await;
    let mut command = sandbox
        .run_interactive(
            &id,
            sh(
                "setsid sleep 600 & echo spawned; exec sleep 600",
                std::time::Duration::from_secs(2),
            ),
        )
        .await
        .expect("starts");
    let (stdout, terminal) = drain(&mut command, std::time::Duration::from_secs(30)).await;
    assert_eq!(stdout, "spawned\n");
    let error = terminal.expect_err("the deadline ends it");
    assert!(
        error.contains("killFailed"),
        "the escaped child holds the output, so the stop is not confirmed: {error}"
    );

    // Two retried stops of 10s each, 5s apart, then the session is killed.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        let ran = match sandbox
            .run_command(&id, sh("echo alive", std::time::Duration::from_secs(10)))
            .await
        {
            Err(_) => false,
            Ok(mut output) => {
                let mut ok = false;
                while let Some(item) = output.next().await {
                    if matches!(item, Ok(CommandOutput::Exit { code: 0, .. })) {
                        ok = true;
                    }
                }
                ok
            }
        };
        if !ran {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session still ran commands 120s after the escaped child's deadline"
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    let _ = sandbox.terminate(&id).await;
    SandboxRoute::remove(KEY).await;
}
