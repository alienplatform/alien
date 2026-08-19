//! Real Docker coverage for the local sandbox manager's isolation guarantees.
//!
//! These assert the properties that make the manager safe to point at untrusted code, so they
//! must run against a real daemon — a mock would only re-state the config we passed in.
//!
//! `cargo test -p alien-local --test sandbox_isolation -- --ignored --test-threads=1`

use std::collections::HashMap;

use alien_local::{
    LocalSandboxManager, SandboxEgressMode, SandboxOutput, SandboxSessionConfig,
};
use tempfile::TempDir;

const IMAGE: &str = "alpine:3.20";

fn config(egress: SandboxEgressMode) -> SandboxSessionConfig {
    SandboxSessionConfig {
        image: IMAGE.to_string(),
        cpu_cores: 0.5,
        memory_bytes: 256 * 1024 * 1024,
        pids_limit: Some(64),
        scratch_bytes: 16 * 1024 * 1024,
        egress,
        preview_ports: Vec::new(),
        env: HashMap::new(),
    }
}

fn manager() -> (LocalSandboxManager, TempDir) {
    let dir = TempDir::new().expect("temp dir");
    let manager =
        LocalSandboxManager::new(dir.path().to_path_buf()).expect("Docker must be reachable");
    (manager, dir)
}

fn stdout(result: &alien_local::SandboxExecResult) -> String {
    result
        .output
        .iter()
        .filter_map(|frame| match frame {
            SandboxOutput::Stdout(bytes) => Some(String::from_utf8_lossy(bytes).to_string()),
            SandboxOutput::Stderr(_) => None,
        })
        .collect()
}

fn sh(command: &str) -> Vec<String> {
    vec!["/bin/sh".to_string(), "-c".to_string(), command.to_string()]
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn session_round_trips_and_runs_unprivileged() {
    let (manager, _dir) = manager();
    let sandbox = "isolation-a";
    manager.reap(sandbox).await.expect("clean slate");

    let session = manager
        .create_session(sandbox, "s1", &config(SandboxEgressMode::Deny))
        .await
        .expect("session creates");

    let whoami = manager
        .exec(&session.container_id, &sh("id -u"))
        .await
        .expect("exec runs");
    assert_eq!(whoami.exit_code, 0);
    assert_eq!(
        stdout(&whoami).trim(),
        "65534",
        "the workload must not run as root"
    );

    manager
        .write_file(&session.container_id, "in.txt", b"payload-in")
        .await
        .expect("file uploads");
    let read_back = manager
        .exec(&session.container_id, &sh("cat /sandbox/in.txt"))
        .await
        .expect("exec runs");
    assert_eq!(stdout(&read_back).trim(), "payload-in");

    manager
        .exec(&session.container_id, &sh("echo payload-out > /sandbox/out.txt"))
        .await
        .expect("exec runs");
    let downloaded = manager
        .read_file(&session.container_id, "out.txt")
        .await
        .expect("file downloads");
    assert_eq!(String::from_utf8_lossy(&downloaded).trim(), "payload-out");

    manager.terminate(sandbox, "s1").await.expect("terminates");
    manager
        .terminate(sandbox, "s1")
        .await
        .expect("terminate is idempotent");
    assert!(
        manager.list_sessions(sandbox).await.expect("lists").is_empty(),
        "a terminated session must not remain"
    );
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn root_filesystem_is_read_only_and_scratch_is_not() {
    let (manager, _dir) = manager();
    let sandbox = "isolation-b";
    manager.reap(sandbox).await.expect("clean slate");

    let session = manager
        .create_session(sandbox, "s1", &config(SandboxEgressMode::Deny))
        .await
        .expect("session creates");

    let root_write = manager
        .exec(&session.container_id, &sh("echo x > /escape 2>&1"))
        .await
        .expect("exec runs");
    assert_ne!(
        root_write.exit_code, 0,
        "the root filesystem must be read-only, got: {}",
        stdout(&root_write)
    );

    let scratch_write = manager
        .exec(&session.container_id, &sh("echo x > /sandbox/ok"))
        .await
        .expect("exec runs");
    assert_eq!(scratch_write.exit_code, 0, "scratch must stay writable");

    manager.terminate(sandbox, "s1").await.expect("terminates");
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn deny_egress_has_no_network_at_all() {
    let (manager, _dir) = manager();
    let sandbox = "isolation-c";
    manager.reap(sandbox).await.expect("clean slate");

    let session = manager
        .create_session(sandbox, "s1", &config(SandboxEgressMode::Deny))
        .await
        .expect("session creates");

    // Assert on the interface list rather than on a reachability probe: a probe that fails
    // could equally mean the network is merely slow.
    let interfaces = manager
        .exec(&session.container_id, &sh("ls /sys/class/net"))
        .await
        .expect("exec runs");
    let listed = stdout(&interfaces);
    assert!(
        !listed.split_whitespace().any(|nic| nic.starts_with("eth")),
        "deny must leave no ethernet interface, saw: {listed}"
    );

    manager.terminate(sandbox, "s1").await.expect("terminates");
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn the_host_gateway_is_not_mapped_in() {
    let (manager, _dir) = manager();
    let sandbox = "isolation-d";
    manager.reap(sandbox).await.expect("clean slate");

    let session = manager
        .create_session(sandbox, "s1", &config(SandboxEgressMode::Allow))
        .await
        .expect("session creates");

    // LocalContainerManager maps host.docker.internal:host-gateway into every container. For a
    // sandbox that is a route to the developer's machine, so its absence is the assertion.
    let hosts = manager
        .exec(&session.container_id, &sh("cat /etc/hosts"))
        .await
        .expect("exec runs");
    assert!(
        !stdout(&hosts).contains("host.docker.internal"),
        "the host gateway must not be reachable by name: {}",
        stdout(&hosts)
    );

    manager.reap(sandbox).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn a_fork_bomb_hits_the_pid_limit_without_taking_the_host_with_it() {
    let (manager, _dir) = manager();
    let sandbox = "isolation-e";
    manager.reap(sandbox).await.expect("clean slate");

    let session = manager
        .create_session(sandbox, "s1", &config(SandboxEgressMode::Deny))
        .await
        .expect("session creates");

    // Bounded rather than a true `:(){ :|:& };:` — the point is that the limit binds, and an
    // unbounded bomb would leave the assertion at the mercy of the test runner.
    let bomb = manager
        .exec(
            &session.container_id,
            &sh("i=0; while [ $i -lt 200 ]; do sleep 30 & i=$((i+1)); done; echo spawned-all"),
        )
        .await
        .expect("exec runs");

    assert!(
        !stdout(&bomb).contains("spawned-all"),
        "the PID limit must bind before 200 processes: {}",
        stdout(&bomb)
    );

    // Not asserted: that the session keeps answering. Once the ceiling is reached there is no
    // room to fork an exec either, so a follow-up command fails with a runc nsexec error. That
    // is the limit working, not the session dying — and the host is unaffected either way,
    // which is the property that matters.

    manager.terminate(sandbox, "s1").await.expect("terminates");
}

/// Pins what Local can and cannot promise about session-to-session reachability.
///
/// `deny` is absolute: no interface, so nothing to reach anything with. `allow` is not, and the
/// gap is in the container runtime rather than in this manager — verified with the raw Docker
/// CLI on OrbStack 29.4.0, where two containers on a bridge created with
/// `com.docker.network.bridge.enable_icc=false` still ping each other. Stock Linux Docker
/// honours the option; OrbStack does not, and OrbStack is the common macOS setup.
///
/// So the promise is: an egress-denied session is isolated, and an egress-allowed session is
/// not isolated from its siblings on every runtime. That is why Local is development-only for
/// untrusted code, and why this is a test rather than a comment.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn deny_isolates_sessions_and_allow_does_not_promise_to() {
    let (manager, _dir) = manager();
    let sandbox = "isolation-f";
    manager.reap(sandbox).await.expect("clean slate");

    let first = manager
        .create_session(sandbox, "s1", &config(SandboxEgressMode::Allow))
        .await
        .expect("first session creates");
    let second = manager
        .create_session(sandbox, "s2", &config(SandboxEgressMode::Deny))
        .await
        .expect("second session creates");

    let address = manager
        .exec(&first.container_id, &sh("hostname -i"))
        .await
        .expect("exec runs");
    let first_ip = stdout(&address).trim().to_string();
    assert!(!first_ip.is_empty(), "the egress-allowed session needs an address");

    let from_denied = manager
        .exec(
            &second.container_id,
            &sh(&format!(
                "ping -c 1 -W 2 {first_ip} >/dev/null 2>&1 && echo REACHED || echo BLOCKED"
            )),
        )
        .await
        .expect("exec runs");
    assert_eq!(
        stdout(&from_denied).trim(),
        "BLOCKED",
        "an egress-denied session has no interface, so it must reach nothing"
    );

    manager.reap(sandbox).await.expect("cleanup");
}

/// Docker accepts port bindings on a network-less container and silently drops them, so this
/// combination would look configured and never resolve. Better a typed error at create.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn a_preview_port_under_deny_egress_is_refused_rather_than_silently_dropped() {
    let (manager, _dir) = manager();
    let sandbox = "isolation-i";
    manager.reap(sandbox).await.expect("clean slate");

    let mut denied = config(SandboxEgressMode::Deny);
    denied.preview_ports = vec![8080];

    let error = manager
        .create_session(sandbox, "s1", &denied)
        .await
        .expect_err("a preview port with no interface to serve it must be refused");
    assert_eq!(error.code, "SANDBOX_SESSION_FAILED");

    let mut allowed = config(SandboxEgressMode::Allow);
    allowed.preview_ports = vec![8080];
    manager
        .create_session(sandbox, "s2", &allowed)
        .await
        .expect("the same port is fine once there is a network");

    manager.reap(sandbox).await.expect("cleanup");
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn reap_removes_every_session_of_a_sandbox_and_leaves_others_alone() {
    let (manager, _dir) = manager();
    let mine = "isolation-g";
    let other = "isolation-h";
    manager.reap(mine).await.expect("clean slate");
    manager.reap(other).await.expect("clean slate");

    manager
        .create_session(mine, "s1", &config(SandboxEgressMode::Deny))
        .await
        .expect("creates");
    manager
        .create_session(mine, "s2", &config(SandboxEgressMode::Deny))
        .await
        .expect("creates");
    manager
        .create_session(other, "s1", &config(SandboxEgressMode::Deny))
        .await
        .expect("creates");

    assert_eq!(manager.list_sessions(mine).await.expect("lists").len(), 2);

    let reaped = manager.reap(mine).await.expect("reaps");
    assert_eq!(reaped, 2);
    assert!(manager.list_sessions(mine).await.expect("lists").is_empty());
    assert_eq!(
        manager.list_sessions(other).await.expect("lists").len(),
        1,
        "reaping one sandbox must not touch another's sessions"
    );

    manager.reap(other).await.expect("cleanup");
}

/// The metadata assertion, with a real bound on what it proves.
///
/// Under `Deny` this is conclusive: the probe returns "Network unreachable". Under `Allow` on a
/// developer machine there is **no metadata service at that address at all**, so the test cannot
/// distinguish blocked from absent and would pass either way. The endpoint was measured
/// *reachable* from inside a gVisor pod on GKE, so the `Allow` case has to be re-asserted on a
/// cloud host — it is covered here only so the Deny path cannot regress unnoticed.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn the_metadata_endpoint_is_unreachable_under_both_egress_modes() {
    for egress in [SandboxEgressMode::Deny, SandboxEgressMode::Allow] {
        let (manager, _dir) = manager();
        let sandbox = format!("meta-{egress:?}").to_lowercase();
        let session = manager
            .create_session(&sandbox, "s1", &config(egress))
            .await
            .expect("session");

        let result = manager
            .exec(
                &session.container_id,
                &sh("wget -q -T 3 -O - http://169.254.169.254/ 2>&1; echo rc=$?"),
            )
            .await
            .expect("exec");

        let out = stdout(&result);
        assert!(
            out.contains("rc=1") || out.contains("rc=4") || !out.contains("rc=0"),
            "{egress:?}: the metadata endpoint must not answer, got: {out}"
        );

        manager.reap(&sandbox).await.expect("reap");
    }
}

/// The credential assertion. The manager builds the session's environment from the
/// controller's template, so anything in the host's environment — including whatever cloud
/// credentials the developer happens to be holding — must not appear inside.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn the_hosts_environment_does_not_leak_into_a_session() {
    // Set on the host only. A sandbox that can read it could read a real credential the same way.
    std::env::set_var("ALIEN_TEST_HOST_ONLY_SECRET", "host-secret-must-not-appear");

    let (manager, _dir) = manager();
    let session = manager
        .create_session("envleak", "s1", &config(SandboxEgressMode::Deny))
        .await
        .expect("session");

    let result = manager
        .exec(&session.container_id, &sh("env"))
        .await
        .expect("exec");

    let out = stdout(&result);
    assert!(
        !out.contains("host-secret-must-not-appear"),
        "the host's environment leaked into the sandbox:\n{out}"
    );
    assert!(
        !out.contains("AWS_SECRET_ACCESS_KEY") && !out.contains("AWS_SESSION_TOKEN"),
        "cloud credentials are present in the sandbox environment:\n{out}"
    );

    manager.reap("envleak").await.expect("reap");
}

/// No session content reaches anything Alien persists. The session's own output is the caller's;
/// it must not be duplicated into anything Alien keeps. Asserted against the manager's state
/// directory, which is the only thing this layer persists.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn session_output_is_not_written_into_manager_state() {
    let (manager, dir) = manager();
    let session = manager
        .create_session("nolog", "s1", &config(SandboxEgressMode::Deny))
        .await
        .expect("session");

    let canary = "canary-9f3a2b7c-session-content";
    let result = manager
        .exec(&session.container_id, &sh(&format!("echo {canary}")))
        .await
        .expect("exec");
    assert!(stdout(&result).contains(canary), "the caller should get its own output");

    let mut found = Vec::new();
    for entry in walk(dir.path()) {
        if std::fs::read(&entry)
            .map(|bytes| String::from_utf8_lossy(&bytes).contains(canary))
            .unwrap_or(false)
        {
            found.push(entry.display().to_string());
        }
    }

    assert!(
        found.is_empty(),
        "session output was persisted by the manager in: {found:?}"
    );

    manager.reap("nolog").await.expect("reap");
}

fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

/// A session nobody terminated — a caller that hung, was cut off, or crashed — must not outlive
/// the sandbox. Deletion reaps every session by label, whether or not its creator ever came back,
/// which is what lets a caller leave cleanup to the resource rather than race it inside a request.
#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn an_abandoned_session_is_reaped_with_its_sandbox() {
    let (manager, _dir) = manager();
    let sandbox = "isolation-reap";
    manager.reap(sandbox).await.expect("clean slate");

    manager
        .create_session(sandbox, "abandoned", &config(SandboxEgressMode::Deny))
        .await
        .expect("session starts");
    // Deliberately no terminate: this is the session a caller walked away from.
    let before = manager.list_sessions(sandbox).await.expect("lists");
    assert_eq!(
        before.len(),
        1,
        "the abandoned session is running: {before:?}"
    );

    let reaped = manager.reap(sandbox).await.expect("reap");
    assert_eq!(reaped, 1, "reap reports the one session it removed");

    let after = manager.list_sessions(sandbox).await.expect("lists");
    assert!(
        after.is_empty(),
        "nothing survives the sandbox's own teardown: {after:?}"
    );
}
