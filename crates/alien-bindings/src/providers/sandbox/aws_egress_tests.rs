//! AWS lifecycle ordering with real TLS agent delivery and durable KV policy records.

use super::*;
use crate::sandbox_egress::tests::Harness;
use crate::sandbox_egress_token::EgressTokenClaims;
use alien_aws_clients::aws::lambda_microvms::{MicrovmAuthToken, MockLambdaMicrovmsApi};
use alien_core::SandboxEgress;
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
async fn aws_egress_create_update_paused_reconnect_and_renew_deliver_fenced_tokens() {
    exercise(false).await;
}

#[tokio::test]
async fn aws_egress_failed_initial_delivery_terminates_the_unreturned_microvm() {
    exercise(true).await;
}

async fn exercise(refuse_delivery: bool) {
    let harness = Arc::new(Harness::new().await);
    let policies = Arc::new(Mutex::new(Vec::<EgressTokenClaims>::new()));
    let seen = policies.clone();
    let fences = harness.clone();
    let agent = Router::new()
        .route(
            "/v1/egress",
            put(
                move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                    let seen = seen.clone();
                    let fences = fences.clone();
                    async move {
                        assert_eq!(headers["x-aws-proxy-auth"], "test-endpoint-token");
                        assert_eq!(headers[PROXY_PORT_HEADER], AGENT_PORT.to_string());
                        let token = body["token"].as_str().unwrap();
                        let claims: EgressTokenClaims = serde_json::from_slice(
                            &URL_SAFE_NO_PAD
                                .decode(token.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap();
                        assert_eq!(claims.sub, "mvm-policy");
                        assert_eq!(claims.aud, "prefix/box");
                        assert_eq!(
                            fences.highest(&claims.sub),
                            Some(claims.generation),
                            "fence must precede agent delivery"
                        );
                        seen.lock().unwrap().push(claims);
                        if refuse_delivery {
                            StatusCode::BAD_REQUEST
                        } else {
                            StatusCode::NO_CONTENT
                        }
                    }
                },
            ),
        )
        .route(HEALTH_PATH, get(|| async { StatusCode::OK }));
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
    .with_egress_manager(Arc::new(harness.reopen().await));
    provider.agent = reqwest::Client::builder()
        .add_root_certificate(
            reqwest::Certificate::from_pem(certificate.cert.pem().as_bytes()).unwrap(),
        )
        .build()
        .unwrap();
    let policy = SandboxEgress::Allow {
        allow: vec!["api.example.com".to_string()],
    };
    let created = provider
        .create(CreateSandboxRequest {
            egress: Some(policy.clone()),
            ..Default::default()
        })
        .await;
    if refuse_delivery {
        assert!(created.is_err());
        assert_eq!(terminated.load(Ordering::SeqCst), 1);
    } else {
        let created = created.unwrap();
        assert_eq!(
            policies.lock().unwrap()[0].policy,
            policy.policy_digest().unwrap()
        );
        paused.store(true, Ordering::SeqCst);
        provider
            .update_egress(&created.sandbox_id, SandboxEgress::None)
            .await
            .unwrap();
        assert!(
            paused.load(Ordering::SeqCst),
            "update must restore paused state"
        );
        assert_eq!(
            policies.lock().unwrap().last().unwrap().policy,
            SandboxEgress::None.policy_digest().unwrap()
        );
        provider.resume(&created.sandbox_id).await.unwrap();
        assert_eq!(
            policies.lock().unwrap().last().unwrap().generation,
            1,
            "renewal must retain generation"
        );
        let recovered = provider
            .get_or_create(CreateSandboxRequest {
                sandbox_id: Some(created.sandbox_id.clone()),
                egress: Some(policy),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(!recovered.created);
        assert_eq!(policies.lock().unwrap().last().unwrap().generation, 2);
        provider.terminate(&created.sandbox_id).await.unwrap();
        assert_eq!(terminated.load(Ordering::SeqCst), 1);
    }
    server.abort();
}

#[tokio::test]
async fn aws_egress_declared_none_needs_no_gateway_for_a_same_policy_update() {
    let mut api = MockLambdaMicrovmsApi::new();
    api.expect_get_microvm().times(1).returning(|id| {
        Ok(Microvm {
            microvm_id: Some(id.to_string()),
            endpoint: None,
            state: Some("SUSPENDED".to_string()),
            image_arn: Some("image".to_string()),
            image_version: Some("1".to_string()),
        })
    });
    let provider = AwsSandbox::new(
        Arc::new(api),
        "image",
        "1",
        vec!["loopback-only".to_string()],
        vec![],
        None,
        None,
    )
    .with_egress(Some(SandboxEgress::None));
    assert_eq!(
        provider
            .update_egress("mvm-policy", SandboxEgress::Internet)
            .await
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_WIDENS"
    );
    provider
        .update_egress("mvm-policy", SandboxEgress::None)
        .await
        .unwrap();
}
