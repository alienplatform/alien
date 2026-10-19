//! AWS lifecycle ordering with real TLS agent delivery and gateway policy writes.

use super::*;
use crate::sandbox_egress::tests::Harness;
use crate::sandbox_egress_token::EgressTokenClaims;
use alien_aws_clients::aws::lambda_microvms::{MicrovmAuthToken, MockLambdaMicrovmsApi};
use alien_core::{EgressPolicy, SandboxEgress};
use axum::{
    http::{HeaderMap, StatusCode},
    routing::{get, put},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};

#[tokio::test]
async fn aws_egress_updates_paused_policy_without_redelivering_identity() {
    exercise(StatusCode::NO_CONTENT, "").await;
}

#[tokio::test]
async fn aws_egress_failed_initial_delivery_terminates_the_unreturned_microvm() {
    exercise(
        StatusCode::INTERNAL_SERVER_ERROR,
        "AGENT_OPERATION_FAILED: Agent operation 'sandbox gateway egress' failed: token audience or sandbox ID mismatch",
    ).await;
}

#[tokio::test]
async fn aws_egress_self_check_refusal_is_typed_and_never_retried() {
    exercise(
        StatusCode::SERVICE_UNAVAILABLE,
        "EGRESS_PATH_UNEXPECTED: EGRESS_PATH_UNEXPECTED: sandbox egress path is not verified",
    )
    .await;
}

#[tokio::test]
async fn aws_egress_transport_refusal_preserves_the_agent_reason() {
    exercise(
        StatusCode::FORBIDDEN,
        "the agent does not serve the code it is running",
    )
    .await;
}

#[tokio::test]
async fn aws_egress_refusal_redacts_an_echoed_identity_token() {
    exercise(StatusCode::BAD_REQUEST, "invalid identity: $TOKEN").await;
}

#[tokio::test]
async fn aws_egress_refusal_body_is_bounded() {
    exercise(StatusCode::BAD_REQUEST, &"x".repeat(8192)).await;
}

async fn exercise(delivery_status: StatusCode, delivery_body: &str) {
    let refuse_delivery = !delivery_status.is_success();
    let harness = Arc::new(Harness::new().await);
    let policies = Arc::new(Mutex::new(Vec::<EgressTokenClaims>::new()));
    let seen = policies.clone();
    let installed = Arc::new(Mutex::new((String::new(), String::new())));
    let agent_copy = installed.clone();
    let record_only = Arc::new(AtomicUsize::new(0));
    let record_only_seen = record_only.clone();
    let fences = harness.clone();
    let response_body = delivery_body.to_string();
    let agent = Router::new()
        .route(
            "/v1/egress",
            put(
                move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                    let seen = seen.clone();
                    let installed = agent_copy.clone();
                    let record_only_seen = record_only_seen.clone();
                    let fences = fences.clone();
                    let response_body = response_body.clone();
                    async move {
                        assert_eq!(headers["x-aws-proxy-auth"], "test-endpoint-token");
                        assert_eq!(headers[PROXY_PORT_HEADER], AGENT_PORT.to_string());
                        let mut installed = installed.lock().unwrap();
                        if let Some(token) = body["token"].as_str() {
                            installed.0 = token.to_owned();
                        } else {
                            record_only_seen.fetch_add(1, Ordering::SeqCst);
                        }
                        assert!(
                            !installed.0.is_empty(),
                            "record-only updates require an installed identity"
                        );
                        let token = &installed.0;
                        let claims: EgressTokenClaims = serde_json::from_slice(
                            &URL_SAFE_NO_PAD
                                .decode(token.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap();
                        assert_eq!(claims.sub, "mvm-policy");
                        assert_eq!(claims.aud, "prefix/box");
                        assert!(
                            fences.policy(&claims.sub).is_some(),
                            "policy must precede identity delivery"
                        );
                        let record = body["policy"]
                            .as_str()
                            .expect("agent must receive its recovery record");
                        let record_claims: serde_json::Value = serde_json::from_slice(
                            &URL_SAFE_NO_PAD
                                .decode(record.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap();
                        assert_eq!(record_claims["sub"], claims.sub);
                        let policy: EgressPolicy =
                            serde_json::from_value(record_claims["policy"].clone()).unwrap();
                        assert_eq!(
                            Some(policy),
                            fences.policy(&claims.sub),
                            "resume must deliver the latest policy"
                        );
                        seen.lock().unwrap().push(claims);
                        let body = response_body.replace("$TOKEN", token);
                        installed.1 = record.to_owned();
                        (delivery_status, body)
                    }
                },
            ),
        )
        .route(HEALTH_PATH, get(|| async { StatusCode::OK }));
    let (endpoint, agent_client, server) = serve_agent(agent);
    let paused = Arc::new(AtomicBool::new(false));
    let terminated = Arc::new(AtomicUsize::new(0));
    let mut api = MockLambdaMicrovmsApi::new();
    api.expect_run_microvm()
        .times(1)
        .withf(|_, _, _, role, connectors, _, _, payload| {
            let value: serde_json::Value = serde_json::from_str(payload.as_ref().unwrap()).unwrap();
            role.is_none()
                && !connectors.is_empty()
                && value == serde_json::json!({"gateway":"127.0.0.1:3128","audience":"prefix/box"})
        })
        .returning(|_, _, _, _, _, _, _, _| {
            Ok(Microvm {
                started_at: Some(crate::sandbox_egress::now_unix().unwrap().into()),
                maximum_duration_in_seconds: Some(3600),
                microvm_id: Some("mvm-policy".to_string()),
                endpoint: None,
                state: Some("PENDING".to_string()),
                image_arn: Some("image".to_string()),
                image_version: Some("1".to_string()),
            })
        });
    let pause_state = paused.clone();
    api.expect_get_microvm().returning(move |id| {
        Ok(Microvm {
            started_at: Some(crate::sandbox_egress::now_unix().unwrap().into()),
            maximum_duration_in_seconds: Some(3600),
            microvm_id: Some(id.to_string()),
            endpoint: Some(endpoint.clone()),
            image_arn: Some("image".to_string()),
            state: Some(
                if pause_state.load(Ordering::SeqCst) {
                    "SUSPENDED"
                } else {
                    "RUNNING"
                }
                .to_string(),
            ),
            image_version: Some("1".to_string()),
        })
    });
    api.expect_create_microvm_auth_token()
        .returning(|_, ports, _| {
            assert_eq!(ports, vec![AGENT_PORT]);
            Ok(MicrovmAuthToken {
                auth_token: [(
                    "X-aws-proxy-auth".to_string(),
                    "test-endpoint-token".to_string(),
                )]
                .into(),
            })
        });
    let pause_state = paused.clone();
    api.expect_resume_microvm().returning(move |_| {
        pause_state.store(false, Ordering::SeqCst);
        Ok(())
    });
    let pause_state = paused.clone();
    api.expect_suspend_microvm().returning(move |_| {
        pause_state.store(true, Ordering::SeqCst);
        Ok(())
    });
    let terminated_count = terminated.clone();
    api.expect_terminate_microvm().returning(move |_| {
        terminated_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let mut provider = AwsSandbox::new(
        Arc::new(api),
        "image",
        "1",
        vec!["connector".to_string()],
        vec![],
        None,
        Some(3600),
    )
    .with_egress(Some(SandboxEgress::Internet))
    .with_egress_manager(Arc::new(harness.reopen().await));
    provider.agent = agent_client;
    let policy = EgressPolicy::Allow {
        allow: vec!["api.example.com".to_string()],
    };
    provider = provider.with_default_policy(Some(policy.clone()));
    let created = provider.create(CreateSandboxRequest::default()).await;
    if refuse_delivery {
        let error = created.unwrap_err();
        if delivery_status == StatusCode::SERVICE_UNAVAILABLE {
            assert_eq!(error.code, "EGRESS_PATH_UNEXPECTED");
            assert!(!error.retryable);
            assert_eq!(
                policies.lock().unwrap().len(),
                1,
                "self-check refusal must not redeliver identity"
            );
        }
        if delivery_status != StatusCode::SERVICE_UNAVAILABLE {
            let expected = if delivery_body.len() > 4096 {
                "agent refusal body exceeded 4096 bytes".to_string()
            } else {
                delivery_body.replace("$TOKEN", "[redacted]")
            };
            assert!(error.to_string().contains(&expected), "{error}");
            assert!(
                !error.to_string().contains("eyJ"),
                "identity must not appear in errors"
            );
            assert_eq!(policies.lock().unwrap().len(), 1);
        }
        assert_eq!(terminated.load(Ordering::SeqCst), 1);
        assert_eq!(harness.policy("mvm-policy"), None);
    } else {
        let created = created.unwrap();
        assert_eq!(harness.policy(&created.sandbox_id), Some(policy.clone()));
        let delivered = policies.lock().unwrap().len();
        let _request = provider
            .request(&created.sandbox_id, reqwest::Method::GET, "/health")
            .await
            .unwrap();
        assert_eq!(
            policies.lock().unwrap().len(),
            delivered,
            "ordinary operations reuse the identity token"
        );
        let original_token = installed.lock().unwrap().0.clone();
        provider
            .update_egress(&created.sandbox_id, EgressPolicy::Allow { allow: vec![] })
            .await
            .unwrap();
        assert_eq!(
            policies.lock().unwrap().len(),
            delivered + 1,
            "running updates must refresh the agent recovery record"
        );
        assert_eq!(
            installed.lock().unwrap().0,
            original_token,
            "record-only delivery must preserve the actual identity token"
        );
        assert_eq!(
            record_only.load(Ordering::SeqCst),
            1,
            "running update must omit token, not mint a replacement in the same second"
        );
        let recovery_record = installed.lock().unwrap().1.clone();
        harness.recover_after_total_loss(&recovery_record).await;
        assert_eq!(
            harness.policy(&created.sandbox_id),
            Some(EgressPolicy::Allow { allow: vec![] }),
            "the agent's delivered record must recover the narrowed policy after total loss"
        );
        let delivered = policies.lock().unwrap().len();
        paused.store(true, Ordering::SeqCst);
        provider
            .update_egress(&created.sandbox_id, EgressPolicy::Allow { allow: vec![] })
            .await
            .unwrap();
        assert!(paused.load(Ordering::SeqCst));
        assert_eq!(
            policies.lock().unwrap().len(),
            delivered,
            "update must not PUT the agent"
        );
        assert_eq!(
            harness.policy(&created.sandbox_id),
            Some(EgressPolicy::Allow { allow: vec![] })
        );
        provider.resume(&created.sandbox_id).await.unwrap();
        assert_eq!(
            policies.lock().unwrap().len(),
            delivered + 1,
            "resume may renew identity"
        );
        let found = provider
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some(created.sandbox_id.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(!found.created);
        assert_eq!(
            harness.policy(&created.sandbox_id),
            Some(EgressPolicy::Allow { allow: vec![] }),
            "reconnect without egress must preserve the narrowed policy"
        );
        let recovered = provider
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some(created.sandbox_id.clone()),
                egress: Some(EgressPolicy::Internet),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(!recovered.created);
        assert_eq!(
            harness.policy(&created.sandbox_id),
            Some(EgressPolicy::Internet)
        );
        provider.terminate(&created.sandbox_id).await.unwrap();
        assert_eq!(terminated.load(Ordering::SeqCst), 1);
        assert_eq!(harness.policy("mvm-policy"), None);
    }
    server.abort();
}

/// Serves an agent over TLS on loopback, since `authorized_request` always builds an `https://`
/// URL. Returns the endpoint, a client that trusts it, and the server task.
fn serve_agent(agent: Router) -> (String, reqwest::Client, tokio::task::JoinHandle<()>) {
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![certificate.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der()).into(),
    )
    .unwrap();
    let tls = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let server = tokio::spawn(async move {
        axum_server::from_tcp_rustls(listener, tls)
            .unwrap()
            .serve(agent.into_make_service())
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(certificate.cert.pem().as_bytes()).unwrap(),
        )
        .build()
        .unwrap();
    (endpoint, client, server)
}

fn microvm(id: &str, state: &str, endpoint: Option<String>) -> Microvm {
    Microvm {
        started_at: Some(crate::sandbox_egress::now_unix().unwrap().into()),
        maximum_duration_in_seconds: Some(120),
        microvm_id: Some(id.to_string()),
        endpoint,
        state: Some(state.to_string()),
        image_arn: Some("image".to_string()),
        image_version: Some("1".to_string()),
    }
}

/// A MicroVM Lambda ended at its maximum lifetime (or is ending) never serves again. Handing its
/// id back made the caller's first command fail with the proxy's 502; `getOrCreate` must answer
/// it with a fresh sandbox under the requested policy, as GCP and Azure do, and must not leave
/// the dead id's policy live on the gateway — nothing called `terminate` for it.
///
/// The `RUNNING` case is a sandbox that ends between `getOrCreate`'s read and the one its policy
/// update makes: the same answer, reached a moment later.
#[tokio::test]
async fn aws_get_or_create_replaces_a_terminated_sandbox_and_removes_its_policy() {
    for dead_state in ["TERMINATED", "TERMINATING", "RUNNING"] {
        let harness = Harness::new().await;
        // The policy the dead sandbox's own create installed, still on the gateway.
        harness
            .reopen()
            .await
            .update("mvm-dead", &EgressPolicy::Internet)
            .await
            .unwrap();
        let delivered_to = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = delivered_to.clone();
        let agent = Router::new()
            .route(
                "/v1/egress",
                put(move |Json(body): Json<serde_json::Value>| {
                    let seen = seen.clone();
                    async move {
                        let token = body["token"].as_str().unwrap();
                        let claims: EgressTokenClaims = serde_json::from_slice(
                            &URL_SAFE_NO_PAD
                                .decode(token.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap();
                        seen.lock().unwrap().push(claims.sub);
                        StatusCode::NO_CONTENT
                    }
                }),
            )
            .route(HEALTH_PATH, get(|| async { StatusCode::OK }));
        let (endpoint, agent_client, server) = serve_agent(agent);

        let mut api = MockLambdaMicrovmsApi::new();
        let reads = Arc::new(AtomicUsize::new(0));
        api.expect_get_microvm().returning(move |id| {
            Ok(if id == "mvm-dead" {
                // The first read sees `dead_state`; any later one sees the MicroVM gone.
                let state = if reads.fetch_add(1, Ordering::SeqCst) == 0 {
                    dead_state
                } else {
                    "TERMINATED"
                };
                microvm(id, state, None)
            } else {
                microvm(id, "RUNNING", Some(endpoint.clone()))
            })
        });
        api.expect_run_microvm()
            .times(1)
            .returning(|_, _, _, _, _, _, _, _| Ok(microvm("mvm-fresh", "PENDING", None)));
        api.expect_create_microvm_auth_token()
            .withf(|id, _, _| id == "mvm-fresh")
            .returning(|_, _, _| {
                Ok(MicrovmAuthToken {
                    auth_token: [("X-aws-proxy-auth".to_string(), "token".to_string())].into(),
                })
            });
        // The dead MicroVM is left to Lambda: it is already ending, and nothing here created it.
        api.expect_terminate_microvm().never();
        api.expect_suspend_microvm().never();
        api.expect_resume_microvm().never();

        let mut provider = AwsSandbox::new(
            Arc::new(api),
            "image",
            "1",
            vec!["connector".to_string()],
            vec![],
            None,
            Some(120),
        )
        .with_egress(Some(SandboxEgress::Internet))
        .with_egress_manager(Arc::new(harness.reopen().await));
        provider.agent = agent_client;

        let requested = EgressPolicy::Allow {
            allow: vec!["api.example.com".to_string()],
        };
        let resolved = provider
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some("mvm-dead".to_string()),
                egress: Some(requested.clone()),
                ..Default::default()
            })
            .await
            .unwrap();

        assert!(
            resolved.created,
            "a {dead_state} sandbox that ends must be replaced, not handed back"
        );
        assert_eq!(resolved.sandbox.sandbox_id, "mvm-fresh");
        assert_eq!(resolved.sandbox.state, SandboxState::Running);
        assert_eq!(harness.policy("mvm-fresh"), Some(requested));
        assert_eq!(
            *delivered_to.lock().unwrap(),
            vec!["mvm-fresh".to_string()],
            "identity goes to the fresh sandbox only"
        );
        assert!(
            harness.tombstoned("mvm-dead"),
            "the {dead_state} sandbox's policy must be removed from the gateway"
        );
        server.abort();
    }
}

/// Updating a terminated sandbox's policy would write a live gateway record for an id that
/// never runs again, re-creating what its end should have removed.
#[tokio::test]
async fn aws_update_egress_refuses_a_terminated_sandbox() {
    let harness = Harness::new().await;
    let mut api = MockLambdaMicrovmsApi::new();
    api.expect_get_microvm()
        .returning(|id| Ok(microvm(id, "TERMINATED", None)));
    let provider = AwsSandbox::new(
        Arc::new(api),
        "image",
        "1",
        vec!["connector".to_string()],
        vec![],
        None,
        Some(120),
    )
    .with_egress(Some(SandboxEgress::Internet))
    .with_egress_manager(Arc::new(harness.reopen().await));

    let error = provider
        .update_egress("mvm-dead", EgressPolicy::Internet)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("sandboxTerminated"), "{error}");
    assert_eq!(harness.policy("mvm-dead"), None, "no policy is written");
}

#[tokio::test]
async fn aws_egress_declared_none_refuses_every_explicit_policy_before_io() {
    let provider = AwsSandbox::new(
        Arc::new(MockLambdaMicrovmsApi::new()),
        "image",
        "1",
        vec!["loopback-only".into()],
        vec![],
        None,
        None,
    )
    .with_egress(Some(SandboxEgress::None));
    for policy in [
        EgressPolicy::Internet,
        EgressPolicy::Allow { allow: vec![] },
    ] {
        assert_eq!(
            provider
                .update_egress("mvm-policy", policy.clone())
                .await
                .unwrap_err()
                .code,
            "SANDBOX_EGRESS_REQUIRES_INTERNET"
        );
        assert_eq!(
            provider
                .create(CreateSandboxRequest {
                    egress: Some(EgressPolicy::Internet),
                    ..Default::default()
                })
                .await
                .unwrap_err()
                .code,
            "SANDBOX_EGRESS_REQUIRES_INTERNET"
        );
    }
}

#[tokio::test]
async fn aws_egress_health_self_check_refusal_is_not_a_restore_response() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    let router = Router::new().route(
        HEALTH_PATH,
        get(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            async {
                let chunks = futures::stream::iter([
                    Ok::<_, std::io::Error>("EGRESS_PATH_"),
                    Ok("UNEXPECTED: sandbox egress path is not verified"),
                ]);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    axum::body::Body::from_stream(chunks),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let response = AwsSandbox::poll_until_healthy(
        reqwest::Client::new().get(format!("http://{address}{HEALTH_PATH}")),
        std::time::Instant::now() + std::time::Duration::from_secs(5),
        "mvm-self-check",
    )
    .await
    .unwrap_err();
    assert_eq!(response.code, "EGRESS_PATH_UNEXPECTED");
    assert!(!response.retryable);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    server.abort();
}
