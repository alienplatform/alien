//! `capabilities().sandbox_lifetime` against what `create` does with a `timeoutMs`, on every
//! provider.
//!
//! A lifetime is a security control, so the flag and the method disagreeing is worse here than
//! anywhere else: a caller reading `true` and receiving a refusal has lost a call, while one
//! reading `true` whose ceiling was quietly dropped runs untrusted code with no deadline at all.
//! Each provider is built over a transport that can be observed, so a refusal that turned into a
//! request shows up as a hit rather than passing quietly.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alien_core::bindings::{BindingValue, KubernetesSandboxBinding, LocalSandboxBinding};
use alien_core::SandboxEgress;
use alien_error::ContextError as _;
use axum::routing::post;
use axum::{Json, Router};
use futures::StreamExt as _;
use serde_json::json;
use std::net::SocketAddr;

use crate::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest, Sandbox};

use super::aws::AwsSandbox;
use super::azure::AzureSandbox;
use super::gcp_agent_platform::GcpAgentPlatformSandbox;
use super::kubernetes::KubernetesSandbox;
use super::local::LocalSandbox;

fn with_lifetime() -> CreateSandboxRequest {
    CreateSandboxRequest {
        timeout_ms: Some(600_000),
        ..Default::default()
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

/// Both create verbs refuse with the code a caller branches on. `get_or_create` is checked too
/// because it is the one that reaches an existing sandbox, which honours a lifetime no more than
/// a fresh one does.
async fn refuses_a_created_lifetime(sandbox: &dyn Sandbox) {
    assert!(
        !sandbox.capabilities().sandbox_lifetime,
        "this helper is for the backends that declare no lifetime"
    );

    for (what, error) in [
        (
            "create",
            sandbox
                .create(with_lifetime())
                .await
                .expect_err("a backend with no lifetime cannot take one"),
        ),
        (
            "get_or_create",
            sandbox
                .get_or_create(with_lifetime())
                .await
                .expect_err("nor on the reconnect path"),
        ),
    ] {
        assert_eq!(
            error.code, "OPERATION_NOT_SUPPORTED",
            "{what} must refuse as unsupported so a caller reading `sandboxLifetime: false` and \
             a caller that asked anyway are told the same thing: {error}"
        );
        assert!(
            error.to_string().contains("sandboxLifetime"),
            "{what} has to name the capability that is missing: {error}"
        );
    }
}

/// AWS declares the capability, and `RunMicrovm` is where the deadline is set. What is observable
/// in-process is that the create got past any refusal and on to the readiness wait.
#[tokio::test]
async fn aws_declares_a_lifetime_and_a_create_carrying_one_reaches_the_backend() {
    let mut microvms = alien_aws_clients::aws::lambda_microvms::MockLambdaMicrovmsApi::new();
    microvms
        .expect_run_microvm()
        .times(1)
        .withf(|_, _, _, _, _, _, max_lifetime| *max_lifetime == Some(600))
        .returning(|_, _, _, _, _, _, _| {
            Ok(alien_aws_clients::aws::lambda_microvms::Microvm {
                microvm_id: Some("s1".to_string()),
                endpoint: None,
                state: Some("PENDING".to_string()),
                image_arn: Some("sbx".to_string()),
                image_version: Some("1".to_string()),
            })
        });
    microvms.expect_get_microvm().returning(|id| {
        Ok(alien_aws_clients::aws::lambda_microvms::Microvm {
            microvm_id: Some(id.to_string()),
            endpoint: None,
            state: Some("RUNNING".to_string()),
            image_arn: Some("sbx".to_string()),
            image_version: Some("1".to_string()),
        })
    });
    microvms.expect_terminate_microvm().returning(|_| Ok(()));

    let sandbox = AwsSandbox::new(
        Arc::new(microvms),
        "sbx",
        "1",
        Vec::new(),
        Vec::new(),
        None,
        None,
    );

    assert!(sandbox.capabilities().sandbox_lifetime);
    let error = sandbox
        .create(with_lifetime())
        .await
        .expect_err("no agent answers in a unit test");
    assert_ne!(
        error.code, "OPERATION_NOT_SUPPORTED",
        "the failure has to be the readiness wait's, not a capability's: {error}"
    );
}

/// GCP declares the capability, and the create body carries `ttl`. The client double stands in
/// for the whole transport, so a create that reaches it is observable directly.
#[tokio::test]
async fn gcp_declares_a_lifetime_and_sends_it_as_a_ttl() {
    let mut client = alien_gcp_clients::gcp::agent_platform::MockAgentPlatformApi::new();
    client
        .expect_create_sandbox()
        .times(1)
        .withf(|_, request| request.ttl.as_deref() == Some("600s"))
        .returning(|_, _| {
            Err(alien_error::AlienError::new(
                alien_gcp_clients::gcp::agent_platform::AgentPlatformErrorData::RequestFailed {
                    operation: "create sandbox".to_string(),
                    message: "the double answers no operation".to_string(),
                },
            ))
        });

    let sandbox = GcpAgentPlatformSandbox::new(
        Arc::new(client),
        "projects/p/locations/us-central1/reasoningEngines/eng1".to_string(),
        "projects/p/locations/us-central1/sandboxTemplates/agent".to_string(),
        None,
    );

    assert!(sandbox.capabilities().sandbox_lifetime);
    let error = sandbox
        .create(with_lifetime())
        .await
        .expect_err("the double refuses the operation");
    assert_ne!(
        error.code, "OPERATION_NOT_SUPPORTED",
        "the failure has to be the double's, not a capability's: {error}"
    );
}

/// The sandbox's lifetime and one command's timeout are separate fields that never feed each
/// other.
///
/// `CreateSandboxRequest::timeout_ms` bounds the sandbox and `RunCommandRequest::timeout` bounds
/// one command inside it. They are near-homonyms sitting on the same binding, so a swap compiles;
/// only a provider that observes both calls catches it. Both values are asserted, so a leak in
/// either direction fails: the create would carry `5s` and the command `600000`.
#[tokio::test]
async fn a_sandbox_lifetime_and_a_command_timeout_never_reach_each_other() {
    let mut client = alien_gcp_clients::gcp::agent_platform::MockAgentPlatformApi::new();
    client
        .expect_create_sandbox()
        .times(1)
        .withf(|_, request| request.ttl.as_deref() == Some("600s"))
        .returning(|_, _| {
            Err(alien_error::AlienError::new(
                alien_gcp_clients::gcp::agent_platform::AgentPlatformErrorData::RequestFailed {
                    operation: "create sandbox".to_string(),
                    message: "the double answers no operation".to_string(),
                },
            ))
        });
    client
        .expect_execute()
        .times(1)
        .withf(|_, _, input| {
            let body: serde_json::Value =
                serde_json::from_slice(input).expect("the envelope is json");
            body["timeoutMs"] == json!(5_000)
        })
        .returning(|_, _, _| Ok(b"{\"t\":\"exit\",\"code\":0,\"truncated\":false}\n".to_vec()));

    let sandbox = GcpAgentPlatformSandbox::new(
        Arc::new(client),
        "projects/p/locations/us-central1/reasoningEngines/eng1".to_string(),
        "projects/p/locations/us-central1/sandboxTemplates/agent".to_string(),
        None,
    );

    sandbox
        .create(with_lifetime())
        .await
        .expect_err("the double refuses the operation");

    let frames: Vec<_> = sandbox
        .run_command(
            "s1",
            RunCommandRequest {
                command: "/bin/true".to_string(),
                args: Vec::new(),
                cwd: None,
                env: BTreeMap::new(),
                timeout: Duration::from_secs(5),
            },
        )
        .await
        .expect("the command runs")
        .collect()
        .await;

    assert!(
        matches!(frames.last(), Some(Ok(CommandOutput::Exit { code, .. })) if *code == 0),
        "the command has to reach its exit: {frames:?}"
    );
}

/// Azure declares none, and the client double proves it: a call on it would panic, so a refusal
/// that leaked through to the data plane fails this test rather than passing quietly.
#[tokio::test]
async fn azure_declares_no_lifetime_and_sends_nothing() {
    let sandbox = AzureSandbox::new(
        Arc::new(alien_azure_clients::azure::sandbox_data_plane::MockSandboxDataPlaneApi::new()),
        "group".to_string(),
        "ubuntu".to_string(),
        SandboxEgress::Allow,
        None,
        "1".to_string(),
        "2Gi".to_string(),
        None,
    );

    refuses_a_created_lifetime(&sandbox).await;
}

/// Kubernetes has `activeDeadlineSeconds`, but the pool's pods are already running under the
/// deadline the declaration fixed by the time a claim reaches them. So this binding narrows the
/// platform's flag to false, and the claim route counts what a refusal must not cost.
#[tokio::test]
async fn kubernetes_narrows_the_platform_flag_and_claims_nothing() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let broker = serve(Router::new().route(
        "/v1/sandbox/sessions",
        post(move || {
            let counted = Arc::clone(&counted);
            async move {
                counted.fetch_add(1, Ordering::SeqCst);
                Json(json!({
                    "sessionId": "s1",
                    "endpoint": "http://127.0.0.1:1",
                    "capability": "cap",
                    "expiresAt": chrono::Utc::now().timestamp() + 300,
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

    assert!(
        alien_core::SandboxCapabilities::for_platform(alien_core::Platform::Kubernetes)
            .expect("Kubernetes has a sandbox backend")
            .sandbox_lifetime,
        "the platform row stays true, which is what gates a declared maxLifetimeSeconds at plan \
         time; only this binding narrows it"
    );
    refuses_a_created_lifetime(&sandbox).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "a refusal must cost no claim: a pod taken from the pool and then discarded is a pod the \
         next caller has to wait for"
    );
}

/// Local declares none, and its manager is a live route here rather than a dead address: a
/// refusal that turned into a request would show up as a hit, and a dead address would not tell
/// the two apart because an unreachable local route reports the same code as a refusal.
#[tokio::test]
async fn local_declares_no_lifetime_and_asks_its_manager_for_nothing() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let manager = serve(Router::new().fallback(move || {
        let counted = Arc::clone(&counted);
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            Json(json!({ "sessionId": "s1", "containerId": "c1" }))
        }
    }))
    .await;

    let token = tempfile::NamedTempFile::new().expect("a token file");
    std::fs::write(token.path(), "route-token").expect("the token is written");

    let sandbox = LocalSandbox::new(
        "sbx",
        &LocalSandboxBinding {
            manager_url: BindingValue::Value(manager),
            sandbox_key: BindingValue::Value("sbx".to_string()),
            token_path: BindingValue::Value(token.path().display().to_string()),
        },
    )
    .await
    .expect("the binding is complete");

    refuses_a_created_lifetime(&sandbox).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "a refusal must cost no request: a container started and then left unbounded is exactly \
         what the refusal exists to prevent"
    );
}

/// A lifetime the request names but no sandbox can be built to, through both doors a caller
/// reaches: `create`, and the `get_or_create` fall-through that creates for an id naming nothing.
///
/// Zero is the one that matters. Raised to one second it buys a sandbox the backend reaps while
/// `create` is still inside its readiness wait, and that wait ends as `SANDBOX_UNREACHABLE` —
/// retryable, HTTP 503 — so a caller obeying it mints another one-second sandbox and is told
/// "transient" forever. The far end is checked beside it because saturating a lifetime wider than
/// whole seconds hold applies a deadline the caller never asked for.
async fn refuses_a_lifetime_no_sandbox_can_be_built_to(sandbox: &dyn Sandbox) {
    for timeout_ms in [0_u64, u64::MAX] {
        for (door, error) in [
            (
                "create",
                sandbox
                    .create(CreateSandboxRequest {
                        timeout_ms: Some(timeout_ms),
                        ..Default::default()
                    })
                    .await
                    .expect_err("a lifetime no sandbox can be built to"),
            ),
            (
                "get_or_create",
                sandbox
                    .get_or_create(CreateSandboxRequest {
                        sandbox_id: Some("names-nothing".to_string()),
                        timeout_ms: Some(timeout_ms),
                        ..Default::default()
                    })
                    .await
                    .expect_err("nor through the create the fall-through reaches"),
            ),
        ] {
            assert_eq!(
                error.code, "INVALID_INPUT",
                "{door} with timeoutMs {timeout_ms} has to refuse the request, not report a \
                 backend that could not be reached: {error}"
            );
            assert!(
                !error.retryable,
                "{door} with timeoutMs {timeout_ms} can never succeed, so a caller told to try \
                 again would build one doomed sandbox per attempt: {error}"
            );
            assert!(
                error.to_string().contains("sandbox lifetime"),
                "{door} with timeoutMs {timeout_ms} has to name what the caller must change: \
                 {error}"
            );
        }
    }
}

/// The client double carries one expectation — the absent read the `get_or_create` door makes —
/// so a `RunMicrovm` that got past the refusal panics on an unexpected call rather than matching
/// a `max_lifetime` assertion and passing quietly.
#[tokio::test]
async fn aws_refuses_a_lifetime_no_microvm_can_be_built_to() {
    let mut microvms = alien_aws_clients::aws::lambda_microvms::MockLambdaMicrovmsApi::new();
    microvms.expect_get_microvm().returning(|id| {
        Err(alien_error::AlienError::new(
            alien_client_core::ErrorData::RemoteResourceNotFound {
                resource_type: "Microvm".to_string(),
                resource_name: id.to_string(),
            },
        ))
    });

    let sandbox = AwsSandbox::new(
        Arc::new(microvms),
        "sbx",
        "1",
        Vec::new(),
        Vec::new(),
        None,
        None,
    );

    refuses_a_lifetime_no_sandbox_can_be_built_to(&sandbox).await;
}

/// The declared ceiling does not stand in for the refusal: it bounds a lifetime that is too long,
/// and neither end here is a lifetime the deployment's own limit can repair.
#[tokio::test]
async fn aws_refuses_it_under_a_declared_ceiling_too() {
    let mut microvms = alien_aws_clients::aws::lambda_microvms::MockLambdaMicrovmsApi::new();
    microvms.expect_get_microvm().returning(|id| {
        Err(alien_error::AlienError::new(
            alien_client_core::ErrorData::RemoteResourceNotFound {
                resource_type: "Microvm".to_string(),
                resource_name: id.to_string(),
            },
        ))
    });

    let sandbox = AwsSandbox::new(
        Arc::new(microvms),
        "sbx",
        "1",
        Vec::new(),
        Vec::new(),
        None,
        Some(1_800),
    );

    refuses_a_lifetime_no_sandbox_can_be_built_to(&sandbox).await;
}

/// GCP reaches the same refusal for the same reason, so the two backends that honour a lifetime
/// answer one bad request identically. Its own `Terminated` mapping is untouched — this never
/// gets far enough to build a sandbox that could terminate.
#[tokio::test]
async fn gcp_refuses_a_lifetime_no_sandbox_can_be_built_to() {
    let mut client = alien_gcp_clients::gcp::agent_platform::MockAgentPlatformApi::new();
    client.expect_get_sandbox().returning(|_, id| {
        Err(
            alien_error::AlienError::new(alien_client_core::ErrorData::RemoteResourceNotFound {
                resource_type: "SandboxEnvironment".to_string(),
                resource_name: id.to_string(),
            })
            .context(
                alien_gcp_clients::gcp::agent_platform::AgentPlatformErrorData::RequestFailed {
                    operation: "get sandbox".to_string(),
                    message: id.to_string(),
                },
            ),
        )
    });

    let sandbox = GcpAgentPlatformSandbox::new(
        Arc::new(client),
        "projects/p/locations/us-central1/reasoningEngines/eng1".to_string(),
        "projects/p/locations/us-central1/sandboxTemplates/agent".to_string(),
        None,
    );

    refuses_a_lifetime_no_sandbox_can_be_built_to(&sandbox).await;
}
