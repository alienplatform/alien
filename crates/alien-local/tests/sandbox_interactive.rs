//! Interactive commands on Local, through the same binding and conformance cases the cloud
//! backends run.
//!
//! `cargo test -p alien-local --test sandbox_interactive -- --ignored --nocapture`

use std::sync::Arc;
use std::time::Duration;

use alien_bindings::providers::sandbox::local::LocalSandbox;
use alien_bindings::traits::{
    CommandOutput, CreateSandboxRequest, InteractiveCommand, RunCommandRequest, Sandbox,
    SandboxState,
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
async fn local_session(sandbox_key: &str, pids_limit: i64) -> (TempDir, Arc<dyn Sandbox>, String) {
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
            pids_limit: Some(pids_limit),
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

fn sh(script: &str, timeout: Duration) -> RunCommandRequest {
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
    within: Duration,
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

/// Runs a short command to its end and returns its stdout, or why it could not run.
async fn run(sandbox: &dyn Sandbox, id: &str, script: &str) -> Result<String, String> {
    let mut command = sandbox
        .run_interactive(id, sh(script, Duration::from_secs(20)))
        .await
        .map_err(|error| error.to_string())?;
    match drain(&mut command, Duration::from_secs(30)).await {
        (stdout, Ok(0)) => Ok(stdout),
        (stdout, other) => Err(format!("{other:?}, stdout {stdout:?}")),
    }
}

async fn state(sandbox: &dyn Sandbox, id: &str) -> SandboxState {
    sandbox
        .get(id)
        .await
        .expect("the session lists")
        .expect("the session is still listed")
        .state
}

/// The signal shell holds `CAP_KILL` and nothing else; the command holds nothing and so cannot
/// kill the shell meant to stop it.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn the_signal_shell_holds_only_cap_kill_and_the_command_cannot_kill_it() {
    const KEY: &str = "interactive-caps";
    let (_dir, sandbox, id) = local_session(KEY, 64).await;
    // Root runs the idle loop and the signal shell, which starts before the first interactive
    // command, this probe. Every root process should hold CAP_KILL and nothing else, and refuse
    // the command's kill.
    let probe = run(
        sandbox.as_ref(),
        &id,
        "echo \"command $(awk '/^CapEff/ {print $2}' /proc/self/status)\"; \
         for p in /proc/[0-9]*; do \
           [ \"$(awk '/^Uid/ {print $2}' $p/status 2>/dev/null)\" = 0 ] || continue; \
           cap=$(awk '/^CapEff/ {print $2}' $p/status); \
           if kill -KILL ${p#/proc/} 2>/dev/null; then echo \"root $cap killed\"; else echo \"root $cap refused\"; fi; \
         done | sort -u; \
         for p in /proc/[0-9]*; do \
           [ \"$(awk '/^Uid/ {print $2}' $p/status 2>/dev/null)\" = 0 ] && tr '\\0' ' ' < $p/cmdline | grep -q '^sh -c while read -r g n t' && echo 'signal shell'; \
         done",
    )
    .await;
    let _ = sandbox.terminate(&id).await;
    SandboxRoute::remove(KEY).await;

    let probe = probe.expect("the probe runs");
    assert_eq!(
        probe, "command 0000000000000000\nroot 0000000000000020 refused\nsignal shell\n",
        "the shell runs as root with CAP_KILL only; the command has no capability and is refused"
    );
}

/// The command kills everything its user may signal. The signal shell is not among them, so the
/// deadline holds from inside: only the command ends, and its session runs on.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn a_command_that_kills_all_it_can_still_meets_its_deadline() {
    const KEY: &str = "interactive-sweep";
    let (_dir, sandbox, id) = local_session(KEY, 64).await;
    let mut command = sandbox
        .run_interactive(
            &id,
            sh(
                "kill -KILL -1 2>/dev/null; echo swept; exec sleep 600",
                Duration::from_secs(3),
            ),
        )
        .await
        .expect("starts");
    let (stdout, terminal) = drain(&mut command, Duration::from_secs(30)).await;
    let after = run(sandbox.as_ref(), &id, "echo alive").await;
    let listed = state(sandbox.as_ref(), &id).await;
    let _ = sandbox.terminate(&id).await;
    SandboxRoute::remove(KEY).await;

    assert_eq!(stdout, "swept\n");
    let error = terminal.expect_err("the deadline ends it");
    assert!(error.contains("timeoutExceeded"), "{error}");
    assert!(
        !error.contains("whole session"),
        "only the command ended: {error}"
    );
    assert_eq!(after.expect("the session runs on"), "alive\n");
    assert_eq!(listed, SandboxState::Running);
}

/// The command takes every process slot in its session. The signal shell started before it and
/// starts nothing to kill it, so the deadline still holds from inside and frees the slots.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn a_command_that_takes_every_process_slot_still_meets_its_deadline() {
    const KEY: &str = "interactive-pids";
    let (_dir, sandbox, id) = local_session(KEY, 24).await;
    let mut command = sandbox
        .run_interactive(
            &id,
            RunCommandRequest {
                command: "python3".to_string(),
                // Forks until the session refuses another process, then waits.
                args: vec![
                    "-c".to_string(),
                    "import os, time\n\
                     while True:\n\
                     \x20   try:\n\
                     \x20       pid = os.fork()\n\
                     \x20   except OSError:\n\
                     \x20       break\n\
                     \x20   if pid == 0:\n\
                     \x20       time.sleep(600)\n\
                     \x20       os._exit(0)\n\
                     print('full', flush=True)\n\
                     time.sleep(600)\n"
                        .to_string(),
                ],
                cwd: None,
                env: Default::default(),
                timeout: Duration::from_secs(4),
            },
        )
        .expect("starts");
    let (stdout, terminal) = drain(&mut command, Duration::from_secs(30)).await;
    let after = run(sandbox.as_ref(), &id, "echo alive").await;
    let _ = sandbox.terminate(&id).await;
    SandboxRoute::remove(KEY).await;

    assert_eq!(stdout, "full\n");
    let error = terminal.expect_err("the deadline ends it");
    assert!(error.contains("timeoutExceeded"), "{error}");
    assert!(
        !error.contains("whole session"),
        "stopped from inside: {error}"
    );
    assert_eq!(after.expect("its slots are free again"), "alive\n");
}

/// A child the command moves to a session of its own escapes the group kill and holds the output
/// open. The deadline still holds: the sandbox session is killed from the host, the caller is
/// told the whole session stopped, the session reports itself stopped, and later calls fail
/// rather than hang.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn a_command_whose_child_escapes_its_group_ends_with_its_session() {
    const KEY: &str = "interactive-escape";
    let (_dir, sandbox, id) = local_session(KEY, 64).await;
    let mut command = sandbox
        .run_interactive(
            &id,
            sh(
                "setsid sleep 600 & echo spawned; exec sleep 600",
                Duration::from_secs(2),
            ),
        )
        .await
        .expect("starts");
    let (stdout, terminal) = drain(&mut command, Duration::from_secs(30)).await;
    let listed = state(sandbox.as_ref(), &id).await;
    let after = tokio::time::timeout(
        Duration::from_secs(30),
        run(sandbox.as_ref(), &id, "echo alive"),
    )
    .await
    .expect("a call on a stopped session fails rather than hangs");
    let _ = sandbox.terminate(&id).await;
    SandboxRoute::remove(KEY).await;

    assert_eq!(stdout, "spawned\n");
    let error = terminal.expect_err("the deadline ends it");
    assert!(error.contains("timeoutExceeded"), "{error}");
    assert!(
        error.contains("the whole session was stopped"),
        "the caller is told the session, not only the command, ended: {error}"
    );
    assert_eq!(
        listed,
        SandboxState::Terminated,
        "a stopped session never reports running"
    );
    assert!(
        after.is_err(),
        "nothing runs in a stopped session: {after:?}"
    );
}
