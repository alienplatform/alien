//! The credential write reaches every replica or fails, refuses an old gateway before any
//! write, and never puts a value in an error or a Debug line.
use super::*;
use crate::providers::vault::local::LocalVault;
use alien_core::{bindings::SandboxEgressSigningKey, SandboxCredentialRule};
use axum::{extract::State, http::StatusCode, routing::get, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_compact::{KeyPair, Seed, Signature};
use std::sync::{
    atomic::{AtomicU16, AtomicU32},
    Mutex,
};

const VALUE: &str = "binding-fixture-value-9d1e";

struct Replica {
    public: ed25519_compact::PublicKey,
    protocol: AtomicU32,
    /// 204 when zero; otherwise every write answers this status.
    status: AtomicU16,
    writes: Mutex<Vec<String>>,
}

async fn protocol(State(replica): State<Arc<Replica>>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"protocol": replica.protocol.load(Ordering::SeqCst)}))
}

async fn write(
    State(replica): State<Arc<Replica>>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> (StatusCode, String) {
    let grant = headers["authorization"]
        .to_str()
        .unwrap()
        .strip_prefix("Bearer ")
        .unwrap()
        .to_string();
    let claims = verified(&replica, &grant, "alien-egress-admin");
    assert_eq!(claims["method"], "PUT");
    let record = body["record"].as_str().unwrap().to_string();
    let record_claims = verified(&replica, &record, "alien-egress-credentials");
    assert_eq!(record_claims["sub"], claims["sub"]);
    replica.writes.lock().unwrap().push(record);
    match replica.status.load(Ordering::SeqCst) {
        0 => (StatusCode::NO_CONTENT, String::new()),
        503 => (
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({"code":"SANDBOX_EGRESS_GATEWAY_UNAVAILABLE","message":"peer away"})
                .to_string(),
        ),
        // A misbehaving gateway that echoes the value: the binding must not repeat it.
        status => (
            StatusCode::from_u16(status).unwrap(),
            serde_json::json!({"code":"EGRESS_INVALID","message":VALUE}).to_string(),
        ),
    }
}

fn verified(replica: &Replica, token: &str, typ: &str) -> serde_json::Value {
    let parts: Vec<_> = token.split('.').collect();
    let header: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    assert_eq!(header["typ"], typ);
    replica
        .public
        .verify(
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap(),
        )
        .unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap()
}

struct Setup {
    manager: SandboxEgressManager,
    replicas: Vec<Arc<Replica>>,
    _directory: tempfile::TempDir,
}

/// `count` replicas on 127.0.0.1, 127.0.0.2, … sharing one port, of which `listening` answer.
async fn setup(count: usize, listening: usize) -> Setup {
    let directory = tempfile::tempdir().unwrap();
    let vault = Arc::new(LocalVault::new(
        "secrets".into(),
        directory.path().join("vault"),
    ));
    let key = KeyPair::from_seed(Seed::new([29; 32]));
    vault
        .set_secret(
            "signing-key",
            &serde_json::json!({"kid":"key-1","privateKey":base64::engine::general_purpose::STANDARD.encode(key.sk.to_der())}).to_string(),
        )
        .await
        .unwrap();
    let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = first.local_addr().unwrap().port();
    let mut listeners = vec![first];
    for index in 1..listening {
        listeners.push(
            tokio::net::TcpListener::bind((format!("127.0.0.{}", index + 1).as_str(), port))
                .await
                .unwrap(),
        );
    }
    let mut replicas = Vec::new();
    for listener in listeners {
        let replica = Arc::new(Replica {
            public: key.pk,
            protocol: AtomicU32::new(2),
            status: AtomicU16::new(0),
            writes: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .route("/v1/protocol", get(protocol))
            .route("/v1/credentials", axum::routing::put(write))
            .with_state(replica.clone());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        replicas.push(replica);
    }
    let config = SandboxEgressGateway {
        links: Default::default(),
        container: "sandbox-egress-gateway".into(),
        audience: "prefix/box".into(),
        signing_key: SandboxEgressSigningKey {
            vault: "secrets".into(),
            key: "signing-key".into(),
        },
        kid: "key-1".into(),
        admin_ca_pem: None,
        replicas: Some(alien_core::bindings::SandboxEgressReplicas {
            dns: String::new(),
            addresses: (0..count).map(|i| format!("127.0.0.{}", i + 1)).collect(),
            port,
            count: count as u32,
            min_protocol: 1,
            proxy_port: 0,
        }),
    };
    let manager = SandboxEgressManager::with_endpoints(
        config,
        vault,
        "127.0.0.1:3128".into(),
        format!("http://127.0.0.1:{port}"),
    )
    .unwrap();
    Setup {
        manager,
        replicas,
        _directory: directory,
    }
}

fn rules() -> Vec<SandboxCredentialRule> {
    serde_json::from_value(serde_json::json!([
        {"host":"api.example.com","header":"Authorization","valuePrefix":"Bearer ","value":VALUE},
        {"link":"connector","header":"X-Turn","method":"POST","path":"/v1/","value":VALUE}
    ]))
    .unwrap()
}

#[tokio::test]
async fn every_replica_gets_the_same_signed_set_before_the_call_returns() {
    let setup = setup(2, 2).await;
    assert!(!format!("{:?}", rules()).contains(VALUE));
    setup
        .manager
        .update_credentials("box-1", &rules())
        .await
        .unwrap();
    let first = setup.replicas[0].writes.lock().unwrap().clone();
    let second = setup.replicas[1].writes.lock().unwrap().clone();
    assert_eq!(first.len(), 1);
    assert_eq!(first, second, "both replicas hold the same record");
    let claims = verified(&setup.replicas[0], &first[0], "alien-egress-credentials");
    assert_eq!(claims["aud"], "prefix/box");
    assert_eq!(claims["sub"], "box-1");
    assert_eq!(
        claims["rules"],
        serde_json::json!([
            {"host":"api.example.com","header":"Authorization","valuePrefix":"Bearer ","value":VALUE},
            {"link":"connector","header":"X-Turn","method":"POST","path":"/v1/","value":VALUE}
        ])
    );
    // An empty list is a write too: it removes every rule.
    setup
        .manager
        .update_credentials("box-1", &[])
        .await
        .unwrap();
    let writes = setup.replicas[1].writes.lock().unwrap().clone();
    let claims = verified(&setup.replicas[1], &writes[1], "alien-egress-credentials");
    assert_eq!(claims["rules"], serde_json::json!([]));
    assert!(
        claims["ver"][0].as_u64()
            > verified(&setup.replicas[1], &writes[0], "alien-egress-credentials")["ver"][0]
                .as_u64()
    );
}

#[tokio::test]
async fn a_replica_that_is_missing_or_keeps_answering_503_means_no_acknowledgement() {
    let missing = setup(2, 1).await;
    let error = missing
        .manager
        .update_credentials("box-1", &rules())
        .await
        .unwrap_err();
    assert!(error.retryable, "{error:?}");
    assert!(!format!("{error:?}").contains(VALUE));

    let away = setup(2, 2).await;
    away.replicas[1].status.store(503, Ordering::SeqCst);
    let error = away
        .manager
        .update_credentials("box-1", &rules())
        .await
        .unwrap_err();
    assert!(error.retryable, "{error:?}");
    // The same record on every attempt: a retry never allocates a newer version.
    let writes = away.replicas[1].writes.lock().unwrap().clone();
    assert!(writes.len() > 1);
    assert!(writes.iter().all(|record| record == &writes[0]));
}

#[tokio::test]
async fn a_gateway_without_credential_support_is_refused_before_any_write() {
    let setup = setup(2, 2).await;
    setup.replicas[1].protocol.store(1, Ordering::SeqCst);
    let error = setup
        .manager
        .update_credentials("box-1", &rules())
        .await
        .unwrap_err();
    assert!(error.message.contains("update the deployment"), "{error:?}");
    assert!(!error.retryable);
    for replica in &setup.replicas {
        assert!(replica.writes.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn refusals_and_invalid_rules_never_repeat_the_value() {
    let setup = setup(1, 1).await;
    setup.replicas[0].status.store(400, Ordering::SeqCst);
    let error = setup
        .manager
        .update_credentials("box-1", &rules())
        .await
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains(VALUE), "{error:?}");
    assert!(error.message.contains("HTTP 400"), "{error:?}");

    let invalid: Vec<SandboxCredentialRule> = serde_json::from_value(serde_json::json!([
        {"host":"api.example.com","header":"Proxy-Authorization","value":VALUE}
    ]))
    .unwrap();
    let error = setup
        .manager
        .update_credentials("box-1", &invalid)
        .await
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains(VALUE), "{error:?}");
}
