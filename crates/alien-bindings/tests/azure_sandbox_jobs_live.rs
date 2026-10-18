//! Real Azure job lifecycle, including provider reconnection and retained-output bounds.
#![cfg(feature = "azure")]

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{AzureClientConfig, AzureCredentials, ClientConfig};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn provider() -> BindingsProvider {
    let region = env("AZURE_SANDBOX_REGION");
    BindingsProvider::new(
        ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
            tenant_id: env("AZURE_TARGET_TENANT_ID"),
            region: Some(region.clone()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: env("AZURE_TARGET_CLIENT_ID"),
                client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
            },
            service_overrides: None,
        })),
        HashMap::from([(
            "box".to_string(),
            serde_json::json!({
                "service": "sandbox-azure", "sandboxGroup": env("AZURE_SANDBOX_GROUP"),
                "dataPlaneEndpoint": format!("https://management.{region}.azuredevcompute.io"),
                "region": region, "resourceGroup": env("AZURE_RESOURCE_GROUP"),
                "diskImage": "ubuntu", "egress": {"mode": "allow"}
            }),
        )]),
    )
    .expect("provider config")
}

fn command(text: &str, seconds: u64) -> RunCommandRequest {
    RunCommandRequest {
        command: "sh".to_string(),
        args: vec!["-c".to_string(), text.to_string()],
        cwd: None,
        env: BTreeMap::new(),
        timeout: Duration::from_secs(seconds),
    }
}

async fn drain(
    sandbox: &dyn Sandbox,
    id: &str,
    job: &str,
) -> (Vec<u8>, Vec<u8>, alien_bindings::traits::JobPoll) {
    let mut cursor = None;
    let mut stdout = vec![];
    let mut stderr = vec![];
    for _ in 0..200 {
        let poll = sandbox.poll_job(id, job, cursor).await.expect("poll");
        for frame in &poll.frames {
            match frame {
                CommandOutput::Stdout { seq, data } => {
                    assert!(cursor.is_none_or(|old| *seq > old));
                    assert!(
                        *seq <= 9_007_199_254_740_991,
                        "cursor must survive JavaScript numbers exactly"
                    );
                    cursor = Some(*seq);
                    stdout.extend(data);
                }
                CommandOutput::Stderr { seq, data } => {
                    assert!(cursor.is_none_or(|old| *seq > old));
                    assert!(
                        *seq <= 9_007_199_254_740_991,
                        "cursor must survive JavaScript numbers exactly"
                    );
                    cursor = Some(*seq);
                    stderr.extend(data);
                }
                _ => panic!("terminal frame belongs outside frames"),
            }
        }
        if !poll.running {
            return (stdout, stderr, poll);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("job did not finish within poll budget")
}

#[tokio::test]
#[ignore = "requires a live test-target Azure sandbox group"]
async fn native_jobs_survive_reconnection_and_bound_work_and_output() {
    let _ = dotenvy::from_path(workspace_root::get_workspace_root().join(".env.test"));
    let sandbox = provider().load_sandbox("box").await.expect("binding");
    let id = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .expect("create")
        .sandbox_id;
    eprintln!("created test sandbox {id}");
    let test_id = id.clone();
    let assertions = tokio::spawn(async move {
        let id = test_id;
        let first = provider().load_sandbox("box").await.expect("reconnect");
        let job = first
            .start_job(
                &id,
                command(
                    "printf first; printf error >&2; sleep 10; printf last; exit 7",
                    20,
                ),
            )
            .await
            .expect("start");
        drop(first);
        let second = provider()
            .load_sandbox("box")
            .await
            .expect("fresh provider");
        let live = second
            .poll_job(&id, &job.job_id, None)
            .await
            .expect("live output");
        assert!(live.running, "the detached job is still active");
        assert!(
            live.frames.iter().any(
                |frame| matches!(frame, CommandOutput::Stdout { data, .. } if data == b"first")
            ),
            "short output is visible before command exit"
        );
        let (out, err, result) = drain(second.as_ref(), &id, &job.job_id).await;
        assert_eq!(out, b"firstlast");
        assert_eq!(err, b"error");
        assert_eq!(result.exit.expect("natural exit").code, 7);
        let replay = second
            .poll_job(&id, &job.job_id, None)
            .await
            .expect("replay");
        assert!(!replay.running);
        assert!(!replay.frames.is_empty());
        let job = second
            .start_job(&id, command("sleep 120 &", 30))
            .await
            .expect("cancel start");
        second
            .cancel_job(&id, &job.job_id)
            .await
            .expect("cancel confirms stop");
        second
            .cancel_job(&id, &job.job_id)
            .await
            .expect("repeat cancel");
        let (_, _, result) = drain(second.as_ref(), &id, &job.job_id).await;
        assert_eq!(result.error.expect("cancel error").code, "cancelled");
        let job = second
            .start_job(&id, command("sleep 120 &", 1))
            .await
            .expect("timeout start");
        let (_, _, result) = drain(second.as_ref(), &id, &job.job_id).await;
        assert_eq!(result.error.expect("timeout error").code, "timeoutExceeded");
        let job = second
            .start_job(
                &id,
                command(
                    "head -c 4300000 /dev/zero; head -c 4300000 /dev/zero >&2",
                    30,
                ),
            )
            .await
            .expect("output start");
        let (out, err, result) = drain(second.as_ref(), &id, &job.job_id).await;
        assert_eq!(out, vec![0; 4194304]);
        assert_eq!(err, vec![0; 4194304]);
        assert!(result.exit.expect("output exit").truncated);
        let mut jobs = Vec::new();
        for _ in 0..16 {
            jobs.push(
                second
                    .start_job(&id, command("sleep 120", 60))
                    .await
                    .expect("admitted slot"),
            );
        }
        second
            .start_job(&id, command("true", 5))
            .await
            .expect_err("seventeenth retained job is refused");
        for job in jobs {
            second
                .cancel_job(&id, &job.job_id)
                .await
                .expect("slot cleanup");
            drain(second.as_ref(), &id, &job.job_id).await;
        }
        let job = second
            .start_job(&id, command("echo usable", 5))
            .await
            .expect("sandbox survives");
        assert_eq!(
            drain(second.as_ref(), &id, &job.job_id).await.0,
            b"usable\n"
        );
    })
    .await;
    sandbox
        .terminate(&id)
        .await
        .expect("cleanup confirms sandbox gone");
    assertions.expect("job lifecycle assertions");
}
