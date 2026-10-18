//! `capabilities().jobs` against what `start_job` actually does, on every provider.
//!
//! The capability set exists so a caller branches before calling. A flag that disagrees with the
//! method is worse than no flag: it sends a caller down a path the backend refuses, or hides a
//! path it supports. So each provider here is built over a transport that can be observed, and
//! the flag is checked against whether `start_job` reached that transport or refused before it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alien_core::bindings::{BindingValue, KubernetesSandboxBinding, LocalSandboxBinding};
use alien_core::SandboxEgress;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use std::collections::BTreeMap;
use std::net::SocketAddr;

use crate::traits::{RunCommandRequest, Sandbox};

use super::aws::AwsSandbox;
use super::azure::AzureSandbox;
use super::gcp_agent_platform::GcpAgentPlatformSandbox;
use super::kubernetes::KubernetesSandbox;
use super::local::LocalSandbox;

fn command() -> RunCommandRequest {
    RunCommandRequest {
        command: "/bin/sleep".to_string(),
        args: vec!["600".to_string()],
        cwd: None,
        env: BTreeMap::new(),
        timeout: Duration::from_secs(600),
    }
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, router).await.expect("serve") });
    format!("http://{address}")
}

/// An agent that accepts a job start, and counts what reached it.
async fn job_agent(hits: Arc<AtomicUsize>) -> String {
    let handler = move || {
        let hits = Arc::clone(&hits);
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            Json(json!({ "jobId": "j1" }))
        }
    };
    serve(Router::new().route("/v1/jobs/start", post(handler))).await
}

/// AWS reaches its agent over TLS, so what is observable in-process is the call that authorizes
/// the request: reaching it at all is what tells a capability refusal apart from a backend that
/// tried. The sandbox read is what fails here, which is a long way past a refusal.
#[tokio::test]
async fn aws_declares_jobs_and_starting_one_reaches_the_backend() {
    let mut microvms = alien_aws_clients::aws::lambda_microvms::MockLambdaMicrovmsApi::new();
    microvms
        .expect_get_microvm()
        .times(1)
        // A record with no image is nobody's sandbox, so the call fails on ownership — well past
        // any refusal a capability would have made.
        .returning(|_| {
            Ok(alien_aws_clients::aws::lambda_microvms::Microvm {
                started_at: None,
                maximum_duration_in_seconds: None,
                microvm_id: Some("s1".to_string()),
                endpoint: None,
                state: Some("RUNNING".to_string()),
                image_arn: None,
                image_version: None,
            })
        });

    let sandbox = AwsSandbox::new(
        Arc::new(microvms),
        "arn:aws:lambda:us-west-2:123456789012:microvm-image:sbx",
        "1",
        Vec::new(),
        Vec::new(),
        None,
        None,
    );

    assert!(sandbox.capabilities().jobs);
    let error = sandbox
        .start_job("s1", command())
        .await
        .expect_err("the fixture's sandbox belongs to nobody");
    assert_ne!(
        error.code, "OPERATION_NOT_SUPPORTED",
        "the failure has to be the sandbox's, not a capability's: {error}"
    );
}

/// Kubernetes reaches its agent over the pod IP the broker hands back, so the whole path runs
/// here: claim a sandbox, start a job on it, and the agent answers with the id.
#[tokio::test]
async fn kubernetes_declares_jobs_and_starts_one_end_to_end() {
    let hits = Arc::new(AtomicUsize::new(0));
    let agent = job_agent(Arc::clone(&hits)).await;

    let expires_at = chrono::Utc::now().timestamp() + 300;
    let broker = serve(Router::new().route(
        "/v1/sandbox/sessions",
        post(move || {
            let agent = agent.clone();
            async move {
                Json(json!({
                    "sessionId": "s1",
                    "endpoint": agent,
                    "capability": "cap",
                    "expiresAt": expires_at,
                }))
            }
        }),
    ))
    .await;

    let token = tempfile::NamedTempFile::new().expect("a token file");
    std::fs::write(token.path(), "service-account-token").expect("the token is written");

    let sandbox = KubernetesSandbox::new(
        "sbx",
        &KubernetesSandboxBinding {
            default_policy: None,
            egress: None,
            namespace: BindingValue::Value("alien".to_string()),
            runtime_class: BindingValue::Value("gvisor".to_string()),
            selector: BindingValue::Value("alien/sandbox=sbx".to_string()),
            broker_url: BindingValue::Value(broker),
            key_name: BindingValue::Value("sandbox-key".to_string()),
            token_path: BindingValue::Value(token.path().display().to_string()),
        },
        "sbx",
    )
    .expect("the binding is complete");

    assert!(sandbox.capabilities().jobs);
    sandbox
        .create(crate::traits::CreateSandboxRequest {
            sandbox_id: Some("s1".to_string()),
            tenant_key: None,
            env: BTreeMap::new(),
            ..Default::default()
        })
        .await
        .expect("the broker claims a pod");

    let started = sandbox
        .start_job("s1", command())
        .await
        .expect("the agent starts the job");
    assert_eq!(started.job_id, "j1");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "one start, one request");
}

/// GCP reaches its agent through the `:execute` proxy, so the client double stands in for the
/// whole transport and a start that works is observable directly.
#[tokio::test]
async fn gcp_declares_jobs_and_starts_one_through_its_proxy() {
    let mut client = alien_gcp_clients::gcp::agent_platform::MockAgentPlatformApi::new();
    client
        .expect_execute()
        .times(1)
        .returning(|_, _, _| Ok(serde_json::to_vec(&json!({ "jobId": "j1" })).unwrap()));

    let sandbox = GcpAgentPlatformSandbox::new(
        Arc::new(client),
        "projects/p/locations/us-central1/reasoningEngines/eng1".to_string(),
        "projects/p/locations/us-central1/sandboxTemplates/agent".to_string(),
        None,
    );

    assert!(sandbox.capabilities().jobs);
    let started = sandbox
        .start_job("s1", command())
        .await
        .expect("the proxy starts the job");
    assert_eq!(started.job_id, "j1");
}

/// Azure reads the sandbox policy before starting a native shell job.
#[tokio::test]
async fn azure_declares_jobs_and_reaches_the_backend() {
    let mut client = alien_azure_clients::azure::sandbox_data_plane::MockSandboxDataPlaneApi::new();
    client.expect_get_sandbox().times(1).returning(|_, id| {
        Ok(alien_azure_clients::azure::sandbox_data_plane::Sandbox {
            id: id.to_string(),
            state: Some("Running".to_string()),
            egress_policy: None,
        })
    });
    client
        .expect_execute_shell_command()
        .times(1)
        .returning(|_, _, _, _| {
            Ok(alien_azure_clients::azure::sandbox_data_plane::ExecResult {
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        });
    let sandbox = AzureSandbox::new(
        Arc::new(client),
        "group".to_string(),
        "ubuntu".to_string(),
        SandboxEgress::Internet,
        None,
        "1".to_string(),
        "2Gi".to_string(),
        None,
    );
    assert!(sandbox.capabilities().jobs);
    let error = sandbox
        .start_job("s1", command())
        .await
        .expect_err("fixture supplies no start receipt");
    assert_eq!(error.code, "SANDBOX_OUTCOME_UNKNOWN");
}

/// Local starts jobs through the authenticated manager route.
#[tokio::test]
async fn local_declares_jobs_and_reaches_its_manager() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let manager = serve(Router::new().fallback(move || {
        let counted = Arc::clone(&counted);
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            Json(json!({ "jobId": "j1" }))
        }
    }))
    .await;

    let token = tempfile::NamedTempFile::new().expect("a token file");
    std::fs::write(token.path(), "route-token").expect("the token is written");

    let sandbox = LocalSandbox::new(
        "sbx",
        &LocalSandboxBinding {
            default_policy: None,
            egress: None,
            manager_url: BindingValue::Value(manager),
            sandbox_key: BindingValue::Value("sbx".to_string()),
            token_path: BindingValue::Value(token.path().display().to_string()),
        },
    )
    .await
    .expect("the binding is complete");

    assert!(sandbox.capabilities().jobs);
    assert_eq!(
        sandbox
            .start_job("s1", command())
            .await
            .expect("start")
            .job_id,
        "j1"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}
