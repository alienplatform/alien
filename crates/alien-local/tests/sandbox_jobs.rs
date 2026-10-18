//! Real Docker job lifecycle through the public binding and authenticated manager route.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::providers::sandbox::local::LocalSandbox;
use alien_bindings::traits::{
    CommandOutput, CreateSandboxRequest, JobPoll, RunCommandRequest, Sandbox,
};
use alien_core::bindings::{BindingValue, LocalSandboxBinding};
use alien_local::{LocalSandboxManager, SandboxEgressMode, SandboxRoute, SandboxSessionConfig};

fn command(script: &str, timeout: Duration) -> RunCommandRequest {
    RunCommandRequest {
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        cwd: None,
        env: BTreeMap::new(),
        timeout,
    }
}

async fn finished(sandbox: &LocalSandbox, job: &str) -> JobPoll {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let poll = sandbox.poll_job("session", job, None).await.expect("poll");
            if !poll.running {
                return poll;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("job finishes within bound")
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn jobs_reconnect_replay_cancel_timeout_and_keep_the_sandbox_usable() {
    let dir = tempfile::TempDir::new().expect("state directory");
    let name = format!("alien1609-cap-{}", uuid::Uuid::new_v4().simple());
    let manager = Arc::new(LocalSandboxManager::new(dir.path().to_path_buf()).expect("Docker"));
    let route = SandboxRoute::ensure(
        Arc::clone(&manager),
        &name,
        SandboxSessionConfig {
            image: "alpine:3.20".to_string(),
            max_lifetime_ms: None,
            cpu_cores: 0.5,
            memory_bytes: 128 * 1024 * 1024,
            pids_limit: Some(64),
            scratch_bytes: 16 * 1024 * 1024,
            egress: SandboxEgressMode::Deny,
            preview_ports: Vec::new(),
            env: Default::default(),
        },
    )
    .await
    .expect("serve route");
    let binding = LocalSandboxBinding {
        sandbox_key: BindingValue::Value(name.clone()),
        manager_url: BindingValue::Value(route.base_url.clone()),
        token_path: BindingValue::Value(route.token_path.display().to_string()),
    };
    let sandbox = LocalSandbox::new("jobs", &binding).await.expect("binding");
    sandbox
        .create(CreateSandboxRequest {
            sandbox_id: Some("session".to_string()),
            ..Default::default()
        })
        .await
        .expect("create");

    // Keep teardown even if an assertion panics: cloud-style cleanup is part of the test.
    let result = tokio::spawn(async move {
        let job = sandbox.start_job("session", command("printf ready; read input || true; sleep 0.2; printf done; printf err >&2; exit 7", Duration::from_secs(10))).await.expect("start");
        drop(sandbox);
        let sandbox = LocalSandbox::new("jobs", &binding).await.expect("reconnect");
        let poll = finished(&sandbox, &job.job_id).await;
        assert_eq!(poll.exit.expect("exit").code, 7);
        assert_eq!(poll.error, None);
        let stdout: Vec<u8> = poll.frames.iter().filter_map(|frame| match frame { CommandOutput::Stdout { data, .. } => Some(data.clone()), _ => None }).flatten().collect();
        assert_eq!(stdout, b"readydone");
        assert_eq!(sandbox.poll_job("session", &job.job_id, None).await.expect("replay"), poll);
        let last = poll.frames.iter().filter_map(|frame| match frame { CommandOutput::Stdout { seq, .. } | CommandOutput::Stderr { seq, .. } => Some(*seq), _ => None }).max().expect("output");
        assert!(sandbox.poll_job("session", &job.job_id, Some(last)).await.expect("cursor").frames.is_empty());

        let job = sandbox.start_job("session", command("printf running; sleep 300", Duration::from_secs(600))).await.expect("start cancellable");
        tokio::time::timeout(Duration::from_secs(5), async {
            while sandbox.poll_job("session", &job.job_id, None).await.expect("live poll").frames.is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("output before completion");
        sandbox.cancel_job("session", &job.job_id).await.expect("cancel");
        assert_eq!(finished(&sandbox, &job.job_id).await.error.expect("cancel outcome").code, "cancelled");
        sandbox.cancel_job("session", &job.job_id).await.expect("repeat cancel");

        let job = sandbox.start_job("session", command("sleep 300", Duration::from_millis(100))).await.expect("bounded job");
        assert_eq!(finished(&sandbox, &job.job_id).await.error.expect("timeout outcome").code, "timeoutExceeded");
        let job = sandbox.start_job("session", command("printf alive", Duration::from_secs(2))).await.expect("sandbox survives");
        assert_eq!(finished(&sandbox, &job.job_id).await.exit.expect("exit").code, 0);
        sandbox.terminate("session").await.expect("terminate");
    }).await;
    SandboxRoute::remove(&name).await;
    manager.reap(&name).await.expect("cleanup");
    assert!(manager
        .list_sessions(&name)
        .await
        .expect("cleanup evidence")
        .is_empty());
    result.expect("job lifecycle assertions");
}

#[tokio::test]
#[ignore = "requires a real Docker daemon"]
async fn lifetime_reaps_without_client_activity_and_cannot_exceed_the_declaration() {
    let dir = tempfile::TempDir::new().expect("state directory");
    let name = format!("alien1609-cap-{}", uuid::Uuid::new_v4().simple());
    let manager = Arc::new(LocalSandboxManager::new(dir.path().to_path_buf()).expect("Docker"));
    let route = SandboxRoute::ensure(
        Arc::clone(&manager),
        &name,
        SandboxSessionConfig {
            image: "alpine:3.20".to_string(),
            max_lifetime_ms: Some(5000),
            cpu_cores: 0.5,
            memory_bytes: 128 * 1024 * 1024,
            pids_limit: Some(64),
            scratch_bytes: 16 * 1024 * 1024,
            egress: SandboxEgressMode::Deny,
            preview_ports: Vec::new(),
            env: Default::default(),
        },
    )
    .await
    .expect("serve route");
    let binding = LocalSandboxBinding {
        manager_url: BindingValue::Value(route.base_url),
        sandbox_key: BindingValue::Value(name.clone()),
        token_path: BindingValue::Value(route.token_path.display().to_string()),
    };
    let sandbox = LocalSandbox::new("lifetime", &binding)
        .await
        .expect("binding");
    let checked_manager = Arc::clone(&manager);
    let checked_name = name.clone();
    let result = tokio::spawn(async move {
        for (id, timeout_ms) in [
            ("default", None),
            ("wider", Some(60_000)),
            ("narrower", Some(1000)),
        ] {
            sandbox
                .create(CreateSandboxRequest {
                    sandbox_id: Some(id.to_string()),
                    timeout_ms,
                    ..Default::default()
                })
                .await
                .expect("create bounded session");
        }
        assert_eq!(
            checked_manager
                .list_sessions(&checked_name)
                .await
                .expect("sessions created")
                .len(),
            3
        );
        drop(sandbox);
        // No binding calls drive expiration: the host reaper must act independently.
        tokio::time::sleep(Duration::from_millis(1800)).await;
        let sessions = checked_manager
            .list_sessions(&checked_name)
            .await
            .expect("short deadline");
        assert_eq!(sessions.len(), 2);
        assert!(sessions
            .iter()
            .all(|session| session.session_id != "narrower"));
        tokio::time::sleep(Duration::from_millis(4000)).await;
        assert!(checked_manager
            .list_sessions(&checked_name)
            .await
            .expect("declaration deadline")
            .is_empty());
    })
    .await;
    SandboxRoute::remove(&name).await;
    manager.reap(&name).await.expect("cleanup");
    assert!(manager
        .list_sessions(&name)
        .await
        .expect("cleanup evidence")
        .is_empty());
    result.expect("lifetime assertions");
}
