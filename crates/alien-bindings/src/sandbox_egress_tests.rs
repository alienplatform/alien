//! Exercise policy ownership across real KV handles, a real vault and HTTP admin transport.

use super::*;
use crate::providers::{kv::local::LocalKv, vault::local::LocalVault};
use alien_core::bindings::SandboxEgressSigningKey;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use ed25519_compact::{KeyPair, Seed, Signature};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

struct AdminState {
    public: ed25519_compact::PublicKey,
    highest: Mutex<std::collections::BTreeMap<String, u64>>,
    refuse: AtomicBool,
}

async fn fence(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    let token = headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("Bearer ")
        .unwrap();
    let parts: Vec<_> = token.split('.').collect();
    let header: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    let claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(header["typ"], "alien-egress-admin");
    state
        .public
        .verify(
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap(),
        )
        .unwrap();
    let policy: SandboxEgress = serde_json::from_value(body["policy"].clone()).unwrap();
    assert_eq!(claims["policy"], policy.policy_digest().unwrap());
    for field in ["aud", "sub", "gen"] {
        assert_eq!(claims[field], body[field]);
    }
    assert!(claims["exp"].as_i64().unwrap() <= now_unix().unwrap() + 60);
    if state.refuse.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    let key = format!(
        "{}/{}",
        body["aud"].as_str().unwrap(),
        body["sub"].as_str().unwrap()
    );
    let mut highest = state.highest.lock().unwrap();
    let current = highest.entry(key).or_default();
    let generation = body["gen"].as_u64().unwrap();
    if generation < *current {
        return StatusCode::CONFLICT;
    }
    *current = generation;
    StatusCode::NO_CONTENT
}

pub(crate) struct Harness {
    manager: SandboxEgressManager,
    state: Arc<AdminState>,
    config: SandboxEgressGateway,
    vault: Arc<LocalVault>,
    directory: tempfile::TempDir,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Harness {
    pub(crate) fn highest(&self, sub: &str) -> Option<u64> {
        self.state
            .highest
            .lock()
            .unwrap()
            .get(&format!("prefix/box/{sub}"))
            .copied()
    }

    pub(crate) async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let vault = Arc::new(LocalVault::new(
            "secrets".to_string(),
            directory.path().join("vault"),
        ));
        let key = KeyPair::from_seed(Seed::new([29; 32]));
        vault.set_secret("signing-key", &serde_json::json!({"kid":"key-1", "privateKey":base64::engine::general_purpose::STANDARD.encode(key.sk.to_der())}).to_string()).await.unwrap();
        let state = Arc::new(AdminState {
            public: key.pk,
            highest: Mutex::new(Default::default()),
            refuse: AtomicBool::new(false),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let admin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/v1/generations", post(fence))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let config = SandboxEgressGateway {
            gateway: "127.0.0.1:3128".to_string(),
            admin,
            audience: "prefix/box".to_string(),
            bound: SandboxEgress::Internet,
            signing_key: SandboxEgressSigningKey {
                vault: "secrets".to_string(),
                key: "signing-key".to_string(),
            },
            kid: "key-1".to_string(),
            admin_ca_pem: None,
        };
        let kv = Arc::new(LocalKv::new(directory.path().join("kv")).await.unwrap());
        let manager = SandboxEgressManager::new(config.clone(), vault.clone(), kv).unwrap();
        Self {
            manager,
            state,
            config,
            vault,
            directory,
            server,
        }
    }
    pub(crate) async fn reopen(&self) -> SandboxEgressManager {
        SandboxEgressManager::new(
            self.config.clone(),
            self.vault.clone(),
            Arc::new(
                LocalKv::new(self.directory.path().join("kv"))
                    .await
                    .unwrap(),
            ),
        )
        .unwrap()
    }
}

fn claims(delivery: &EgressDelivery) -> crate::sandbox_egress_token::EgressTokenClaims {
    serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(delivery.token.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn unused_policy_chunks_expire_without_resetting_generations_or_other_sandboxes() {
    let harness = Harness::new().await;
    let manager = &harness.manager;
    let deadline = now_unix().unwrap() + 3;
    manager
        .create("short", &SandboxEgress::Internet, deadline)
        .await
        .unwrap();
    manager.update("short", &SandboxEgress::None).await.unwrap();
    let lost_policy = SandboxEgress::Allow {
        allow: vec!["api.example.com".to_string()],
    };
    assert_eq!(
        manager
            .create("short", &lost_policy, deadline)
            .await
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_CONFLICT"
    );
    manager
        .create("long", &SandboxEgress::Internet, deadline + 1000)
        .await
        .unwrap();
    let prefix = format!("sandbox-policy-content:{deadline}:");
    let chunks = manager.kv.scan_prefix(&prefix, None, None).await.unwrap();
    assert_eq!(
        chunks.items.len(),
        3,
        "current, replaced and losing policies exist"
    );

    // Exercise the real store's expiry, allowing its millisecond TTL to pass the Unix deadline.
    tokio::time::sleep(Duration::from_secs(4)).await;
    for chunk in chunks.items {
        assert!(manager.kv.get(&chunk.key).await.unwrap().is_none());
    }
    assert!(manager
        .kv
        .get(&manager.key("short"))
        .await
        .unwrap()
        .is_some());
    assert!(manager.renew("short").await.is_err());
    assert_eq!(
        manager
            .create("short", &SandboxEgress::Internet, deadline + 1000)
            .await
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_CONFLICT"
    );
    let renewed = harness.reopen().await.renew("long").await.unwrap();
    assert_eq!(
        claims(&renewed).policy,
        SandboxEgress::Internet.policy_digest().unwrap()
    );
}

#[tokio::test]
async fn policy_survives_reconnect_and_concurrent_updates_without_extending_lifetime() {
    let harness = Harness::new().await;
    let deadline = now_unix().unwrap() + 1000;
    let first = harness
        .manager
        .create("session", &SandboxEgress::Internet, deadline)
        .await
        .unwrap();
    assert_eq!(claims(&first).exp, deadline);
    let other = harness.reopen().await;
    let policy = SandboxEgress::Allow {
        allow: vec!["api.example.com".to_string()],
    };
    let (a, b) = tokio::join!(
        harness.manager.update("session", &policy),
        other.update("session", &policy)
    );
    for result in [&a, &b] {
        if let Err(error) = result {
            assert_eq!(error.code, "SANDBOX_EGRESS_CONFLICT");
        }
    }
    let reopened = harness.reopen().await;
    let renewed = reopened.renew("session").await.unwrap();
    assert!(renewed.generation >= 1);
    assert_eq!(claims(&renewed).policy, policy.policy_digest().unwrap());
    assert_eq!(claims(&renewed).exp, deadline);
    assert_eq!(
        harness
            .manager
            .confirm("session", first.generation)
            .await
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_CONFLICT"
    );
    assert_eq!(
        harness
            .manager
            .create("session", &SandboxEgress::Internet, deadline + 999)
            .await
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_CONFLICT"
    );
}

#[tokio::test]
async fn failed_fence_returns_no_token_and_retry_renews_the_latest_policy() {
    let harness = Harness::new().await;
    harness
        .manager
        .create(
            "session",
            &SandboxEgress::Internet,
            now_unix().unwrap() + 1000,
        )
        .await
        .unwrap();
    harness.state.refuse.store(true, Ordering::SeqCst);
    assert!(harness
        .manager
        .update("session", &SandboxEgress::None)
        .await
        .is_err());
    harness.state.refuse.store(false, Ordering::SeqCst);
    let renewed = harness.reopen().await.renew("session").await.unwrap();
    assert_eq!(
        claims(&renewed).policy,
        SandboxEgress::None.policy_digest().unwrap()
    );
    assert_eq!(renewed.generation, 1);
}

#[tokio::test]
async fn all_256_long_targets_round_trip_through_real_kv_value_limits() {
    let harness = Harness::new().await;
    let policy = SandboxEgress::Allow {
        allow: (0..256)
            .map(|i| {
                format!(
                    "{i}.{}.{}.{}.example.com",
                    "a".repeat(63),
                    "b".repeat(63),
                    "c".repeat(63)
                )
            })
            .collect(),
    };
    assert!(serde_json::to_vec(&policy).unwrap().len() > 24 * 1024);
    harness
        .manager
        .create("long-policy", &policy, now_unix().unwrap() + 1000)
        .await
        .unwrap();
    let renewed = harness.reopen().await.renew("long-policy").await.unwrap();
    assert_eq!(claims(&renewed).policy, policy.policy_digest().unwrap());
}

#[tokio::test]
async fn maximum_target_count_uses_small_tokens_and_survives_reopen() {
    let harness = Harness::new().await;
    let policy = SandboxEgress::Allow {
        allow: (0..4096)
            .map(|i| format!("{i}.{}.example.com", "a".repeat(32)))
            .collect(),
    };
    let bytes = policy.canonical_json().unwrap();
    assert!(bytes.len() > 192 * 1024 && bytes.len() <= 256 * 1024);
    let initial = harness
        .manager
        .create("maximum-policy", &policy, now_unix().unwrap() + 1000)
        .await
        .unwrap();
    assert!(initial.token.len() < 4096);
    let renewed = harness
        .reopen()
        .await
        .renew("maximum-policy")
        .await
        .unwrap();
    assert_eq!(claims(&renewed).policy, policy.policy_digest().unwrap());
    assert_eq!(claims(&initial).policy, claims(&renewed).policy);
}

#[tokio::test]
#[ignore = "requires ALIEN_TEST_EGRESS_GATEWAY_BIN and public TCP access to one.one.one.one:443"]
async fn released_gateway_accepts_minted_tokens_and_revokes_old_generations() {
    released_gateway_contract(false).await;
}

#[tokio::test]
#[ignore = "requires ALIEN_TEST_EGRESS_GATEWAY_BIN and public TCP access to one.one.one.one:443"]
async fn released_gateway_accepts_all_4096_targets() {
    released_gateway_contract(true).await;
}

async fn released_gateway_contract(long_policy: bool) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let harness = Harness::new().await;
    let proxy_reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let admin_reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = proxy_reservation.local_addr().unwrap();
    let admin = admin_reservation.local_addr().unwrap();
    let pair = KeyPair::from_seed(Seed::new([29; 32]));
    let config = serde_json::json!({
        "listen":proxy.to_string(),"adminListen":admin.to_string(),"bounds":{"prefix/box":"internet"},
        "keys":[{"kid":"key-1","publicKey":base64::engine::general_purpose::STANDARD.encode(pair.pk.as_ref())}],
        "deniedRanges":[],"generationBinding":STATE_BINDING,"credentials":[]
    });
    let kv =
        serde_json::json!({"service":"local-kv","dataDir":harness.directory.path().join("kv")});
    drop((proxy_reservation, admin_reservation));
    let binary =
        std::env::var("ALIEN_TEST_EGRESS_GATEWAY_BIN").expect("set the gateway executable path");
    let mut process = tokio::process::Command::new(binary)
        .kill_on_drop(true)
        .env("ALIEN_DEPLOYMENT_TYPE", "local")
        .env("ALIEN_EGRESS_STATE_BINDING", kv.to_string())
        .env("ALIEN_EGRESS_GATEWAY_CONFIG", config.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap();
    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        assert!(
            process.try_wait().unwrap().is_none(),
            "gateway exited before becoming ready"
        );
        if client
            .get(format!("http://{admin}/health"))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "gateway did not start"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let mut binding = harness.config.clone();
    binding.admin = format!("http://{admin}");
    binding.gateway = proxy.to_string();
    let manager = SandboxEgressManager::new(
        binding,
        harness.vault.clone(),
        Arc::new(
            LocalKv::new(harness.directory.path().join("kv"))
                .await
                .unwrap(),
        ),
    )
    .unwrap();
    let policy = if long_policy {
        SandboxEgress::Allow {
            allow: std::iter::once("one.one.one.one".to_string())
                .chain((0..4095).map(|i| format!("{i}.{}.example.com", "a".repeat(32))))
                .collect(),
        }
    } else {
        SandboxEgress::Internet
    };
    let initial = manager
        .create("gateway-session", &policy, now_unix().unwrap() + 600)
        .await
        .unwrap();
    async fn connect_status(proxy: std::net::SocketAddr, token: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(proxy).await.unwrap();
        stream.write_all(format!("CONNECT one.one.one.one:443 HTTP/1.1\r\nHost: one.one.one.one:443\r\nProxy-Authorization: Bearer {token}\r\n\r\n").as_bytes()).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            reader.read_line(&mut status),
        )
        .await
        .unwrap()
        .unwrap();
        status
    }
    assert!(
        connect_status(proxy, &initial.token)
            .await
            .starts_with("HTTP/1.1 200"),
        "the real gateway must accept the bounded token before revocation"
    );
    let narrowed = manager
        .update("gateway-session", &SandboxEgress::None)
        .await
        .unwrap();
    assert!(
        connect_status(proxy, &initial.token)
            .await
            .starts_with("HTTP/1.1 403"),
        "an older token must be fenced"
    );
    assert!(
        connect_status(proxy, &narrowed.token)
            .await
            .starts_with("HTTP/1.1 403"),
        "none must deny the same destination"
    );
    process.kill().await.unwrap();
    process.wait().await.unwrap();
}
