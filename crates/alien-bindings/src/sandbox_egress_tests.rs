//! Real vault and signed HTTP policy lifecycle, without a caller-side policy database.
use super::*;
use crate::providers::vault::local::LocalVault;
use alien_core::bindings::SandboxEgressSigningKey;
use axum::{
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    routing::put,
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_compact::{KeyPair, Seed, Signature};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex as StdMutex,
};

struct AdminState {
    public: ed25519_compact::PublicKey,
    policies: StdMutex<std::collections::BTreeMap<String, EgressPolicy>>,
    writes: StdMutex<Vec<String>>,
    refuse: AtomicBool,
}
async fn policy_request(
    State(state): State<Arc<AdminState>>,
    method: Method,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    let token = headers["authorization"]
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
    assert_eq!(claims["method"], method.as_str());
    for field in ["aud", "sub"] {
        assert_eq!(claims[field], body[field]);
    }
    assert!(claims["exp"].as_i64().unwrap() <= now_unix().unwrap() + 60);
    let policy = if method == Method::PUT {
        let policy: EgressPolicy = serde_json::from_value(body["policy"].clone()).unwrap();
        assert_eq!(claims["policy"], policy.policy_digest().unwrap());
        Some(policy)
    } else {
        assert!(claims.get("policy").is_none());
        None
    };
    if state.refuse.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    let key = body["sub"].as_str().unwrap().to_string();
    state.writes.lock().unwrap().push(method.to_string());
    let mut policies = state.policies.lock().unwrap();
    if let Some(policy) = policy {
        policies.insert(key, policy);
    } else {
        policies.remove(&key);
    }
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
    pub(crate) fn refuse_writes(&self, refuse: bool) {
        self.state.refuse.store(refuse, Ordering::SeqCst);
    }
    pub(crate) fn policy(&self, id: &str) -> Option<EgressPolicy> {
        self.state.policies.lock().unwrap().get(id).cloned()
    }
    pub(crate) async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let vault = Arc::new(LocalVault::new(
            "secrets".into(),
            directory.path().join("vault"),
        ));
        let key = KeyPair::from_seed(Seed::new([29; 32]));
        vault.set_secret("signing-key",&serde_json::json!({"kid":"key-1","privateKey":base64::engine::general_purpose::STANDARD.encode(key.sk.to_der())}).to_string()).await.unwrap();
        let state = Arc::new(AdminState {
            public: key.pk,
            policies: StdMutex::new(Default::default()),
            writes: StdMutex::new(Vec::new()),
            refuse: AtomicBool::new(false),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let admin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/v1/policies", put(policy_request).delete(policy_request))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let config = SandboxEgressGateway {
            container: "sandbox-egress-gateway".into(),
            audience: "prefix/box".into(),
            signing_key: SandboxEgressSigningKey {
                vault: "secrets".into(),
                key: "signing-key".into(),
            },
            kid: "key-1".into(),
            admin_ca_pem: None,
        };
        let manager = SandboxEgressManager::with_endpoints(
            config.clone(),
            vault.clone(),
            "127.0.0.1:3128".into(),
            admin,
        )
        .unwrap();
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
        SandboxEgressManager::with_endpoints(
            self.config.clone(),
            self.vault.clone(),
            self.manager.gateway.clone(),
            self.manager.admin_url.clone(),
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
async fn policy_updates_and_renewal_use_one_gateway_store() {
    let h = Harness::new().await;
    let expires = now_unix().unwrap() + 1000;
    let first = h
        .manager
        .create("s1", &EgressPolicy::Internet, expires)
        .await
        .unwrap();
    assert_eq!(h.policy("s1"), Some(EgressPolicy::Internet));
    let policy = EgressPolicy::Allow {
        allow: vec!["*.example.com".into()],
    };
    h.manager.update("s1", &policy).await.unwrap();
    assert_eq!(h.policy("s1"), Some(policy));
    let renewed = h.reopen().await.renew("s1", expires).await.unwrap();
    assert_eq!(claims(&first).exp, claims(&renewed).exp);
    assert_eq!(*h.state.writes.lock().unwrap(), vec!["PUT", "PUT"]);
    h.manager.delete("s1").await.unwrap();
    assert_eq!(h.policy("s1"), None);
    h.manager.delete("s1").await.unwrap();
}
#[tokio::test]
async fn full_size_policy_uses_a_small_identity_token_and_failures_propagate() {
    let h = Harness::new().await;
    let policy = EgressPolicy::Allow {
        allow: (0..4096)
            .map(|i| format!("{i}.{}.example.com", "a".repeat(32)))
            .collect(),
    };
    assert!(policy.canonical_json().unwrap().len() > 192 * 1024);
    let token = h
        .manager
        .create("large", &policy, now_unix().unwrap() + 1000)
        .await
        .unwrap();
    assert!(token.token.len() < 4096);
    assert_eq!(
        h.policy("large").unwrap().canonical_json().unwrap(),
        policy.canonical_json().unwrap()
    );
    h.state.refuse.store(true, Ordering::SeqCst);
    assert!(h
        .manager
        .update("large", &EgressPolicy::Internet)
        .await
        .is_err());
    assert!(h.manager.delete("large").await.is_err());
    assert!(h
        .manager
        .create("expired", &EgressPolicy::Internet, now_unix().unwrap())
        .await
        .is_err());
    assert_eq!(h.policy("expired"), None);
}

#[tokio::test]
#[ignore = "requires ALIEN_TEST_EGRESS_GATEWAY_BIN and public TCP access to one.one.one.one:443"]
async fn released_gateway_updates_policy_without_changing_the_identity_token() {
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
        "listen":proxy.to_string(),"admin":admin.to_string(),"audiences":["prefix/box"],"defaultPolicies":{},
        "keys":[{"kid":"key-1","publicKey":base64::engine::general_purpose::STANDARD.encode(pair.pk.as_ref())}],
        "deniedRanges":[],"generationStore":{"binding":"egress-state"},"credentials":[]
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
    let manager = SandboxEgressManager::with_endpoints(
        harness.config.clone(),
        harness.vault.clone(),
        proxy.to_string(),
        format!("http://{admin}"),
    )
    .unwrap();
    let policy = if long_policy {
        EgressPolicy::Allow {
            allow: std::iter::once("one.one.one.one".to_string())
                .chain((0..4095).map(|i| format!("{i}.{}.example.com", "a".repeat(32))))
                .collect(),
        }
    } else {
        EgressPolicy::Internet
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
        "the real gateway must accept the identity under its current policy"
    );
    manager
        .update("gateway-session", &EgressPolicy::Allow { allow: vec![] })
        .await
        .unwrap();
    assert!(
        connect_status(proxy, &initial.token)
            .await
            .starts_with("HTTP/1.1 403"),
        "the same identity token must be denied by the empty policy"
    );
    manager.update("gateway-session", &policy).await.unwrap();
    assert!(
        connect_status(proxy, &initial.token)
            .await
            .starts_with("HTTP/1.1 200"),
        "same identity token must use the updated policy"
    );
    manager.delete("gateway-session").await.unwrap();
    assert!(connect_status(proxy, &initial.token)
        .await
        .starts_with("HTTP/1.1 403"));
    process.kill().await.unwrap();
    process.wait().await.unwrap();
}

#[tokio::test]
async fn linked_container_resolves_both_gateway_ports_without_storing_an_address_in_the_sandbox_binding(
) {
    use crate::traits::BindingsProviderApi;
    let h = Harness::new().await;
    let provider = crate::BindingsProvider::new(
        alien_core::ClientConfig::Local {
            state_directory: h.directory.path().display().to_string(),
        },
        [(
            "sandbox-egress-gateway".into(),
            serde_json::to_value(alien_core::bindings::ContainerBinding::local(
                "gateway",
                "http://gateway.internal:8080",
            ))
            .unwrap(),
        )]
        .into(),
    )
    .unwrap();
    let container = provider.load_container(&h.config.container).await.unwrap();
    let manager = SandboxEgressManager::new(
        h.config.clone(),
        h.vault.clone(),
        container.get_internal_url(),
    )
    .unwrap();
    assert_eq!(manager.gateway, "gateway.internal:3128");
    assert_eq!(manager.admin_url, "http://gateway.internal:3129/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&manager.bootstrap_payload()).unwrap()["gateway"],
        "gateway.internal:3128"
    );
    for invalid in [
        "http://user:secret@gateway",
        "http://gateway/path",
        "file:///tmp/gateway",
        "http://gateway?token=secret",
    ] {
        assert!(SandboxEgressManager::new(h.config.clone(), h.vault.clone(), invalid).is_err());
    }
}
