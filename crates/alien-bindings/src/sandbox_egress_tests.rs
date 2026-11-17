//! Real vault and signed HTTP policy lifecycle, without a caller-side policy database.
use super::*;
use crate::providers::vault::local::LocalVault;
use alien_core::bindings::SandboxEgressSigningKey;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, Method, StatusCode},
    routing::put,
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_compact::{KeyPair, Seed, Signature};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex as StdMutex,
};

struct AdminState {
    public: ed25519_compact::PublicKey,
    policies:
        StdMutex<std::collections::BTreeMap<String, ((u64, String), Option<EgressPolicy>, String)>>,
    writes: StdMutex<Vec<String>>,
    refuse: AtomicBool,
    store_full: AtomicBool,
    transient_failures: AtomicUsize,
    records: StdMutex<Vec<String>>,
    record_response: StdMutex<Option<String>>,
}
fn verify_admin(state: &AdminState, headers: &HeaderMap, method: &str) -> serde_json::Value {
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
    assert_eq!(claims["method"], method);
    let now = now_unix().unwrap();
    assert!((now - 2..=now).contains(&claims["iat"].as_i64().unwrap()));
    assert!(claims["exp"].as_i64().unwrap() <= now + 60);
    claims
}
fn verify_record(state: &AdminState, record: &str) -> serde_json::Value {
    let parts: Vec<_> = record.split('.').collect();
    let header: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    assert_eq!(header["typ"], "alien-egress-record");
    state
        .public
        .verify(
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap(),
        )
        .unwrap();
    let claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(claims["v"], 1);
    assert!(claims.get("exp").is_none());
    claims
}
async fn policy_record(
    State(state): State<Arc<AdminState>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> (StatusCode, String) {
    let claims = verify_admin(&state, &headers, "GET");
    for field in ["aud", "sub"] {
        assert_eq!(claims[field], query[field]);
    }
    if let Some(body) = state.record_response.lock().unwrap().clone() {
        return (StatusCode::OK, body);
    }
    match state.policies.lock().unwrap().get(&query["sub"]) {
        Some(row) => (
            StatusCode::OK,
            serde_json::json!({"record":row.2}).to_string(),
        ),
        None => (StatusCode::NOT_FOUND, String::new()),
    }
}
async fn policy_request(
    State(state): State<Arc<AdminState>>,
    method: Method,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let grant = verify_admin(&state, &headers, method.as_str());
    let record = body["record"].as_str().unwrap();
    let claims = verify_record(&state, record);
    for field in ["aud", "sub"] {
        assert_eq!(grant[field], claims[field]);
    }
    let version: (u64, String) = serde_json::from_value(claims["ver"].clone()).unwrap();
    assert_eq!(version.1.len(), 32);
    assert!(version
        .1
        .bytes()
        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    let policy = if method == Method::PUT {
        Some(serde_json::from_value(claims["policy"].clone()).unwrap())
    } else {
        assert_eq!(claims["deleted"], true);
        None
    };
    state.records.lock().unwrap().push(record.into());
    if state.store_full.load(Ordering::SeqCst) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"code":"SANDBOX_EGRESS_POLICY_STORE_FULL"})),
        )
            .into_response();
    }
    if state.refuse.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let key = claims["sub"].as_str().unwrap().to_string();
    state.writes.lock().unwrap().push(method.to_string());
    let mut policies = state.policies.lock().unwrap();
    if policies.get(&key).is_none_or(|row| version > row.0) {
        policies.insert(key, (version, policy, record.into()));
    }
    // Simulate an acknowledgement lost after a successful local commit.
    if state
        .transient_failures
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
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
    pub(crate) fn lose_policy_state(&self) {
        self.state.policies.lock().unwrap().clear();
    }
    pub(crate) fn refuse_writes(&self, refuse: bool) {
        self.state.refuse.store(refuse, Ordering::SeqCst);
    }
    pub(crate) fn policy(&self, id: &str) -> Option<EgressPolicy> {
        self.state
            .policies
            .lock()
            .unwrap()
            .get(id)
            .and_then(|row| row.1.clone())
    }
    /// True once the gateway holds a deletion tombstone for `id`.
    pub(crate) fn tombstoned(&self, id: &str) -> bool {
        self.state
            .policies
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|row| row.1.is_none())
    }
    pub(crate) async fn recover_after_total_loss(&self, record: &str) {
        self.state.policies.lock().unwrap().clear();
        let claims = verify_record(&self.state, record);
        let issuer = self.manager.issuer().await.unwrap();
        let grant = issuer
            .mint_admin(
                "prefix/box",
                claims["sub"].as_str().unwrap(),
                EgressAdminOperation::Put,
                now_unix().unwrap(),
            )
            .unwrap();
        let response = reqwest::Client::new()
            .put(self.manager.admin_url.join("v1/policies").unwrap())
            .bearer_auth(grant)
            .json(&serde_json::json!({"record":record}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
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
            store_full: AtomicBool::new(false),
            transient_failures: AtomicUsize::new(0),
            records: StdMutex::new(Vec::new()),
            record_response: StdMutex::new(None),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let admin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/v1/policies", put(policy_request).delete(policy_request))
            .route("/v1/policies/record", axum::routing::get(policy_record))
            .route(
                "/v1/protocol",
                axum::routing::get(|| async { axum::Json(serde_json::json!({"protocol": 1})) }),
            )
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
            replicas: None,
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
            self.manager.admin_url.to_string(),
        )
        .unwrap()
    }
    /// A manager whose binding lists fixed replica addresses (admin port 3129), as on Azure.
    pub(crate) async fn reopen_with_replicas(&self, addresses: &[&str]) -> SandboxEgressManager {
        let mut config = self.config.clone();
        config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
            dns: String::new(),
            addresses: addresses.iter().map(|a| a.to_string()).collect(),
            port: 3129,
            count: addresses.len() as u32,
            min_protocol: 0,
        });
        SandboxEgressManager::with_endpoints(
            config,
            self.vault.clone(),
            format!("{}:3128", addresses[0]),
            format!("http://{}:3129", addresses[0]),
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
/// A linked workload's binding records the signing key id at render time, and the workload is
/// not re-rendered when the deployment's key changes. Tokens must carry the key the vault (and
/// so the gateway configuration) holds now, not fail on the stale copy.
#[tokio::test]
async fn a_binding_rendered_before_the_key_changed_signs_with_the_current_key() {
    let h = Harness::new().await;
    let rotated = KeyPair::from_seed(Seed::new([31; 32]));
    h.vault
        .set_secret(
            "signing-key",
            &serde_json::json!({"kid":"key-2","privateKey":base64::engine::general_purpose::STANDARD.encode(rotated.sk.to_der())}).to_string(),
        )
        .await
        .unwrap();
    assert_eq!(h.config.kid, "key-1", "the binding still names the old key");

    let token = h
        .manager
        .mint_identity("s1", now_unix().unwrap() + 1000)
        .await
        .expect("the workload signs with the vault's current key");

    let mut parts = token.split('.');
    let header: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts.next().unwrap()).unwrap()).unwrap();
    assert_eq!(header["kid"], "key-2");
    let payload = parts.next().unwrap();
    let signature =
        Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts.next().unwrap()).unwrap()).unwrap();
    let signed = &token[..token.rfind('.').unwrap()];
    assert!(payload.len() > 0);
    rotated
        .pk
        .verify(signed.as_bytes(), &signature)
        .expect("signed by the rotated key");
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
async fn reopened_clients_share_a_monotonic_process_writer_and_retries_preserve_records() {
    let h = Harness::new().await;
    h.state.transient_failures.store(2, Ordering::SeqCst);
    h.manager
        .update("s1", &EgressPolicy::Internet)
        .await
        .unwrap();
    let first = h.state.policies.lock().unwrap()["s1"].0.clone();
    let records = h.state.records.lock().unwrap().clone();
    assert_eq!(records.len(), 3);
    assert!(records.iter().all(|record| record == &records[0]));
    h.reopen().await.delete("s1").await.unwrap();
    let deleted = h.state.policies.lock().unwrap()["s1"].0.clone();
    assert!(deleted.0 > first.0);
    assert_eq!(deleted.1, first.1);
    assert_eq!(h.policy("s1"), None);
    h.reopen()
        .await
        .update("s1", &EgressPolicy::Allow { allow: vec![] })
        .await
        .unwrap();
    let latest = h.state.policies.lock().unwrap()["s1"].clone();
    assert!(latest.0 > deleted);
    let delivery = h
        .reopen()
        .await
        .renew("s1", now_unix().unwrap() + 600)
        .await
        .unwrap();
    assert_eq!(delivery.policy, latest.2);
}

#[tokio::test]
async fn concurrent_bindings_use_distinct_versions_with_one_writer_identity() {
    let h = Harness::new().await;
    let mut updates = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let manager = h.reopen().await;
        updates.spawn(async move { manager.update("s1", &EgressPolicy::Internet).await });
    }
    while let Some(result) = updates.join_next().await {
        result.unwrap().unwrap();
    }
    let versions: std::collections::BTreeSet<(u64, String)> = h
        .state
        .records
        .lock()
        .unwrap()
        .iter()
        .map(|record| {
            serde_json::from_value(verify_record(&h.state, record)["ver"].clone()).unwrap()
        })
        .collect();
    assert_eq!(versions.len(), 16);
    assert!(versions.iter().all(|v| v.1 == versions.first().unwrap().1));
    assert_eq!(
        &h.state.policies.lock().unwrap()["s1"].0,
        versions.last().unwrap()
    );
}

#[tokio::test]
async fn missing_or_oversized_resume_record_fails_without_republishing_a_default() {
    let h = Harness::new().await;
    assert!(h
        .manager
        .renew("missing", now_unix().unwrap() + 600)
        .await
        .is_err());
    for response in ["{}".into(), " ".repeat(360 * 1024 + 1)] {
        *h.state.record_response.lock().unwrap() = Some(response);
        assert!(h
            .manager
            .renew("s1", now_unix().unwrap() + 600)
            .await
            .is_err());
    }
    assert!(h.state.writes.lock().unwrap().is_empty());
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
        "deniedRanges":[],"credentials":[],"policyMemoryBytes":67108864
    });
    drop((proxy_reservation, admin_reservation));
    let binary =
        std::env::var("ALIEN_TEST_EGRESS_GATEWAY_BIN").expect("set the gateway executable path");
    let mut process = tokio::process::Command::new(binary)
        .kill_on_drop(true)
        .env("ALIEN_DEPLOYMENT_TYPE", "local")
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
    let manager = Arc::new(
        SandboxEgressManager::with_endpoints(
            harness.config.clone(),
            harness.vault.clone(),
            proxy.to_string(),
            format!("http://{admin}"),
        )
        .unwrap(),
    );
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
    let issuer = manager.issuer().await.unwrap();
    let original_authorization = issuer
        .mint_admin(
            "prefix/box",
            "gateway-session",
            EgressAdminOperation::Put,
            now_unix().unwrap(),
        )
        .unwrap();
    let original_body = serde_json::json!({
        "record":initial.policy,
    });
    let replay = || {
        client
            .put(format!("http://{admin}/v1/policies"))
            .bearer_auth(&original_authorization)
            .json(&original_body)
    };
    assert_eq!(
        replay().send().await.unwrap().status(),
        reqwest::StatusCode::NO_CONTENT
    );
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
    assert_eq!(
        replay().send().await.unwrap().status(),
        reqwest::StatusCode::NO_CONTENT,
        "a delayed replay must not undo the newer restriction"
    );
    assert!(
        connect_status(proxy, &initial.token)
            .await
            .starts_with("HTTP/1.1 403"),
        "the same identity token must be denied by the empty policy"
    );
    #[cfg(feature = "aws")]
    {
        let mut api = alien_aws_clients::aws::lambda_microvms::MockLambdaMicrovmsApi::new();
        api.expect_get_microvm().returning(|id| {
            Ok(alien_aws_clients::aws::lambda_microvms::Microvm {
                microvm_id: Some(id.into()),
                image_arn: Some("image".into()),
                state: Some("RUNNING".into()),
                started_at: None,
                maximum_duration_in_seconds: None,
                endpoint: None,
                image_version: Some("1".into()),
            })
        });
        let provider = crate::providers::sandbox::aws::AwsSandbox::new(
            Arc::new(api),
            "image",
            "1",
            vec!["connector".into()],
            vec![],
            None,
            Some(3600),
        )
        .with_egress(Some(alien_core::SandboxEgress::Internet))
        .with_egress_manager(manager.clone());
        let found = crate::traits::Sandbox::get_or_create(
            &provider,
            crate::traits::CreateSandboxRequest {
                sandbox_id: Some("gateway-session".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!found.created);
        assert!(
            connect_status(proxy, &initial.token)
                .await
                .starts_with("HTTP/1.1 403"),
            "getOrCreate without policy must not reopen an unlisted destination"
        );
    }
    manager.update("gateway-session", &policy).await.unwrap();
    assert!(
        connect_status(proxy, &initial.token)
            .await
            .starts_with("HTTP/1.1 200"),
        "same identity token must use the updated policy"
    );
    manager.delete("gateway-session").await.unwrap();
    assert_eq!(
        replay().send().await.unwrap().status(),
        reqwest::StatusCode::NO_CONTENT,
        "a delayed replay must not resurrect a deleted policy"
    );
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
            serde_json::to_value(
                alien_core::bindings::ContainerBinding::local(
                    "gateway",
                    "http://gateway.internal:8080",
                )
                .with_internal_urls(std::collections::HashMap::from([
                    (3128, "http://proxy.internal:3128".to_string().into()),
                    (3129, "http://admin.internal:3129".to_string().into()),
                ])),
            )
            .unwrap(),
        )]
        .into(),
    )
    .unwrap();
    let container = provider.load_container(&h.config.container).await.unwrap();
    let urls = container.get_internal_urls();
    let manager = SandboxEgressManager::new(
        h.config.clone(),
        h.vault.clone(),
        &urls[&3128],
        &urls[&3129],
    )
    .unwrap();
    assert_eq!(manager.gateway, "proxy.internal:3128");
    assert_eq!(manager.admin_url.as_str(), "http://admin.internal:3129/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&manager.bootstrap_payload()).unwrap()["gateway"],
        "proxy.internal:3128"
    );
    for invalid in [
        "http://user:secret@gateway",
        "http://gateway/path",
        "file:///tmp/gateway",
        "http://gateway?token=secret",
    ] {
        assert!(SandboxEgressManager::new(
            h.config.clone(),
            h.vault.clone(),
            invalid,
            &urls[&3129]
        )
        .is_err());
    }
}

/// A replicated gateway counts a write only once every ready replica holds it, so a
/// narrowed policy can never stay wide on a replica the writer did not reach.
#[tokio::test]
async fn replicated_writes_reach_every_ready_replica_or_fail() {
    let h = Harness::new().await;
    let port = h.manager.admin_url.port().unwrap();
    let second = Arc::new(AdminState {
        public: h.state.public,
        policies: StdMutex::new(Default::default()),
        writes: StdMutex::new(Vec::new()),
        refuse: AtomicBool::new(false),
        store_full: AtomicBool::new(false),
        transient_failures: AtomicUsize::new(0),
        records: StdMutex::new(Vec::new()),
        record_response: StdMutex::new(None),
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.2", port))
        .await
        .unwrap();
    let router = Router::new()
        .route("/v1/policies", put(policy_request).delete(policy_request))
        .with_state(second.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut config = h.config.clone();
    config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
        dns: "replicas.invalid".into(),
        addresses: vec![],
        port,
        count: 2,
        min_protocol: 0,
    });
    let mut manager = SandboxEgressManager::with_endpoints(
        config,
        h.vault.clone(),
        h.manager.gateway.clone(),
        h.manager.admin_url.to_string(),
    )
    .unwrap();
    let first: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let other: std::net::SocketAddr = format!("127.0.0.2:{port}").parse().unwrap();
    let held = |state: &AdminState| {
        state
            .policies
            .lock()
            .unwrap()
            .get("box-1")
            .and_then(|row| row.1.clone())
    };
    let wide: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com","www.example.com"]}))
            .unwrap();
    let narrow: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();

    // One of two replicas is discoverable: nothing is written and the caller is told.
    manager.replica_addresses = Some(vec![first]);
    let error = manager.update("box-1", &wide).await.unwrap_err();
    assert_eq!(error.code, "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE");
    assert!(held(&h.state).is_none() && held(&second).is_none());

    manager.replica_addresses = Some(vec![first, other]);
    manager.update("box-1", &wide).await.unwrap();
    manager.update("box-1", &narrow).await.unwrap();
    assert_eq!(held(&h.state), Some(narrow.clone()));
    assert_eq!(held(&second), Some(narrow.clone()));

    // A replica that does not acknowledge fails the write.
    second.refuse.store(true, Ordering::SeqCst);
    assert!(manager.update("box-1", &wide).await.is_err());
    server.abort();
}

#[test]
fn static_replica_addresses_are_the_write_targets_without_dns() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let h = Harness::new().await;
        let mut config = h.config.clone();
        config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
            dns: String::new(),
            addresses: vec!["10.0.3.4".into(), "10.0.3.5".into()],
            port: 3129,
            count: 2,
            min_protocol: 0,
        });
        let manager = SandboxEgressManager::with_endpoints(
            config,
            h.vault.clone(),
            h.manager.gateway.clone(),
            h.manager.admin_url.to_string(),
        )
        .unwrap();
        let targets = manager.write_targets().await.unwrap();
        assert_eq!(targets.len(), 2);
        assert!(targets[0].ends_with("10.0.3.4:3129") && targets[1].ends_with("10.0.3.5:3129"));
    });
}

#[tokio::test]
async fn a_gateway_older_than_the_binding_needs_is_refused_with_a_rerun_setup_message() {
    let h = Harness::new().await;
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();
    let with = |min_protocol: u32| {
        let mut config = h.config.clone();
        config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
            dns: String::new(),
            addresses: vec!["127.0.0.1".into()],
            port: h.manager.admin_url.port().unwrap(),
            count: 1,
            min_protocol,
        });
        SandboxEgressManager::with_endpoints(
            config,
            h.vault.clone(),
            h.manager.gateway.clone(),
            h.manager.admin_url.to_string(),
        )
        .unwrap()
    };
    with(1)
        .update("box-1", &policy)
        .await
        .expect("a gateway at protocol 1 serves protocol 1");
    let error = with(2).update("box-1", &policy).await.unwrap_err();
    assert_eq!(error.code, "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE");
    assert!(
        error
            .to_string()
            .contains("re-run setup to update the egress gateway"),
        "{error}"
    );
    // Retrying cannot fix an old gateway.
    assert!(!error.retryable, "{error}");
}

/// A two-replica manager whose second replica listens on `127.0.0.2` only after `delay`
/// (never when `delay` is `None`). Both replicas hold the grant for `box-1` first.
async fn two_replicas(
    h: &Harness,
    delay: Option<std::time::Duration>,
) -> (
    SandboxEgressManager,
    Arc<AdminState>,
    tokio::task::JoinHandle<()>,
) {
    let port = h.manager.admin_url.port().unwrap();
    let second = Arc::new(AdminState {
        public: h.state.public,
        policies: StdMutex::new(Default::default()),
        writes: StdMutex::new(Vec::new()),
        refuse: AtomicBool::new(false),
        store_full: AtomicBool::new(false),
        transient_failures: AtomicUsize::new(0),
        records: StdMutex::new(Vec::new()),
        record_response: StdMutex::new(None),
    });
    let mut config = h.config.clone();
    config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
        dns: String::new(),
        addresses: vec!["127.0.0.1".into(), "127.0.0.2".into()],
        port,
        count: 2,
        min_protocol: 1,
    });
    let manager = SandboxEgressManager::with_endpoints(
        config,
        h.vault.clone(),
        h.manager.gateway.clone(),
        h.manager.admin_url.to_string(),
    )
    .unwrap();
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();
    for state in [&h.state, &second] {
        state.policies.lock().unwrap().insert(
            "box-1".into(),
            ((0, String::new()), Some(policy.clone()), String::new()),
        );
    }
    let state = second.clone();
    let restart = tokio::spawn(async move {
        let Some(delay) = delay else {
            return std::future::pending().await;
        };
        tokio::time::sleep(delay).await;
        let listener = tokio::net::TcpListener::bind(("127.0.0.2", port))
            .await
            .unwrap();
        let router = Router::new()
            .route("/v1/policies", put(policy_request).delete(policy_request))
            .route(
                "/v1/protocol",
                axum::routing::get(|| async { axum::Json(serde_json::json!({"protocol": 1})) }),
            )
            .with_state(state);
        axum::serve(listener, router).await.unwrap();
    });
    (manager, second, restart)
}

fn holds_grant(state: &AdminState) -> bool {
    state
        .policies
        .lock()
        .unwrap()
        .get("box-1")
        .is_some_and(|row| row.1.is_some())
}

/// A replica that is away for longer than the whole retry budget never stops the delete from
/// reaching the replica that answers: that one is clean, and the caller gets a retryable error
/// because the other has not acknowledged.
#[tokio::test]
async fn a_revocation_reaches_the_reachable_replica_while_the_other_is_away() {
    let h = Harness::new().await;
    let (manager, _second, away) = two_replicas(&h, None).await;

    let error = manager
        .delete("box-1")
        .await
        .expect_err("one replica never acknowledges");
    assert_eq!(error.code, "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE", "{error}");
    assert!(error.retryable, "{error}");
    assert!(
        !holds_grant(&h.state),
        "the reachable replica holds no grant"
    );
    away.abort();
}

/// A replica that comes back within the budget gets the same delete, and the delete then succeeds.
#[tokio::test]
async fn a_revocation_reaches_a_replica_that_returns_during_the_retries() {
    let h = Harness::new().await;
    let (manager, second, back) =
        two_replicas(&h, Some(std::time::Duration::from_millis(1500))).await;

    manager.delete("box-1").await.expect("both acknowledge");
    assert!(!holds_grant(&h.state));
    assert!(!holds_grant(&second));
    back.abort();
}

fn clear_grant(state: &AdminState) {
    state.policies.lock().unwrap().remove("box-1");
}

/// A new grant counts once one replica acknowledges it, however long the other has been away,
/// and the call is bounded: it must not wait out every replica's timeout on every attempt.
#[tokio::test]
async fn a_new_grant_succeeds_while_one_replica_is_away() {
    let h = Harness::new().await;
    let (manager, second, away) = two_replicas(&h, None).await;
    clear_grant(&h.state);
    clear_grant(&second);
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();

    let started = std::time::Instant::now();
    let delivery = manager
        .create("box-1", &policy, now_unix().unwrap() + 600)
        .await
        .expect("one replica acknowledges the grant");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "bounded: {:?}",
        started.elapsed()
    );
    assert!(!delivery.policy.is_empty());
    assert!(
        holds_grant(&h.state),
        "the reachable replica serves the grant"
    );
    assert!(
        !holds_grant(&second),
        "the absent replica holds nothing yet"
    );
    away.abort();
}

/// With no replica answering, a create fails within the budget and says it can be retried.
#[tokio::test]
async fn a_new_grant_with_every_replica_away_fails_fast_and_retryable() {
    let h = Harness::new().await;
    let port = h.manager.admin_url.port().unwrap();
    // Both replicas at addresses nothing listens on.
    let mut config = h.config.clone();
    config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
        dns: String::new(),
        addresses: vec!["127.0.0.3".into(), "127.0.0.4".into()],
        port,
        count: 2,
        min_protocol: 1,
    });
    let manager = SandboxEgressManager::with_endpoints(
        config,
        h.vault.clone(),
        h.manager.gateway.clone(),
        h.manager.admin_url.to_string(),
    )
    .unwrap();
    let away = tokio::spawn(async {});
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();
    let started = std::time::Instant::now();
    let error = manager
        .create("box-1", &policy, now_unix().unwrap() + 600)
        .await
        .expect_err("nothing acknowledges");
    assert_eq!(error.code, "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE", "{error}");
    assert!(error.retryable, "{error}");
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    away.abort();
}

/// A replica whose policy store is full answers 503 too, but with its own code: the create must
/// return that code (not the generic gateway error, and not a read-back success).
#[tokio::test]
async fn a_new_grant_refused_because_the_store_is_full_keeps_that_code() {
    let h = Harness::new().await;
    let port = h.manager.admin_url.port().unwrap();
    let mut config = h.config.clone();
    config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
        dns: String::new(),
        addresses: vec!["127.0.0.1".into()],
        port,
        count: 1,
        min_protocol: 0,
    });
    let manager = SandboxEgressManager::with_endpoints(
        config,
        h.vault.clone(),
        h.manager.gateway.clone(),
        h.manager.admin_url.to_string(),
    )
    .unwrap();
    h.state.store_full.store(true, Ordering::SeqCst);
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();
    let error = manager
        .create("box-1", &policy, now_unix().unwrap() + 600)
        .await
        .expect_err("the store is full");
    assert_eq!(error.code, "SANDBOX_EGRESS_POLICY_STORE_FULL", "{error}");
}

/// An update (which may narrow) still needs every replica, so a replica that missed it can
/// never keep serving the wider policy.
#[tokio::test]
async fn an_update_still_needs_every_replica() {
    let h = Harness::new().await;
    let (manager, _second, away) = two_replicas(&h, None).await;
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["a.example.com"]})).unwrap();
    let error = manager.update("box-1", &policy).await.unwrap_err();
    // A replica that stays away through every attempt is a condition that passes.
    assert_eq!(error.code, "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE", "{error}");
    assert!(error.retryable, "{error}");
    away.abort();
}

/// A revocation whose replica lookup keeps failing is retryable too, and the lookup is retried
/// inside the loop (the failure is not returned on the first attempt).
#[tokio::test]
async fn a_revocation_whose_replica_lookup_keeps_failing_is_retryable() {
    let h = Harness::new().await;
    let mut config = h.config.clone();
    config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
        dns: String::new(),
        addresses: vec!["not-an-address".into()],
        port: h.manager.admin_url.port().unwrap(),
        count: 1,
        min_protocol: 0,
    });
    let manager = SandboxEgressManager::with_endpoints(
        config,
        h.vault.clone(),
        h.manager.gateway.clone(),
        h.manager.admin_url.to_string(),
    )
    .unwrap();
    let started = std::time::Instant::now();
    let error = manager.delete("box-1").await.unwrap_err();
    assert_eq!(error.code, "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE", "{error}");
    assert!(error.retryable, "{error}");
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(500),
        "the lookup was retried with backoff: {:?}",
        started.elapsed()
    );
}

/// A replica that accepts the connection and never answers must not stretch a new grant past its
/// budget: the deadline covers the protocol read, the write and the read-back, not only the pauses.
#[tokio::test]
async fn a_new_grant_against_a_replica_that_never_answers_stays_inside_the_budget() {
    let h = Harness::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.2:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let blackhole = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    let mut config = h.config.clone();
    config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
        dns: String::new(),
        addresses: vec!["127.0.0.2".into()],
        port,
        count: 1,
        min_protocol: 0,
    });
    let manager = SandboxEgressManager::with_endpoints(
        config,
        h.vault.clone(),
        h.manager.gateway.clone(),
        format!("http://127.0.0.2:{port}"),
    )
    .unwrap();
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();
    let started = std::time::Instant::now();
    let error = manager
        .create("box-1", &policy, now_unix().unwrap() + 600)
        .await
        .unwrap_err();
    assert!(error.retryable, "{error}");
    assert!(
        started.elapsed() < super::GRANT_BUDGET + std::time::Duration::from_secs(2),
        "the whole operation is bounded: {:?}",
        started.elapsed()
    );
    blackhole.abort();
}

/// Two real gateway processes: a create while replica A is stopped succeeds, the sandbox reaches
/// its allowed host through B, and A serves the same grant once it has started and synchronized.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires ALIEN_TEST_EGRESS_GATEWAY_BIN and public TCP access to one.one.one.one:443"]
async fn a_create_with_a_stopped_real_replica_succeeds_and_the_replica_catches_up() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let h = Harness::new().await;
    let binary =
        std::env::var("ALIEN_TEST_EGRESS_GATEWAY_BIN").expect("set the gateway executable path");
    let ports = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxies = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let admin_port = ports.local_addr().unwrap().port();
    let proxy_port = proxies.local_addr().unwrap().port();
    drop((ports, proxies));
    let key = KeyPair::from_seed(Seed::new([29; 32]));
    let start = |me: &str, peer: &str| {
        let config = serde_json::json!({
            "listen":format!("{me}:{proxy_port}"),"admin":format!("{me}:{admin_port}"),
            "audiences":["prefix/box"],"defaultPolicies":{},
            "keys":[{"kid":"key-1","publicKey":base64::engine::general_purpose::STANDARD.encode(key.pk.as_ref())}],
            "deniedRanges":[],"credentials":[],
            "peers":{"static":[format!("{peer}:{admin_port}")]},
            "replicaSecret":base64::engine::general_purpose::STANDARD.encode([13u8;32]),
            "policyMemoryBytes":67108864
        });
        tokio::process::Command::new(&binary)
            .kill_on_drop(true)
            .env("ALIEN_DEPLOYMENT_TYPE", "local")
            .env("ALIEN_EGRESS_GATEWAY_CONFIG", config.to_string())
            .env("RUST_LOG", "error")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap()
    };
    let client = reqwest::Client::new();
    let ready = |me: String| {
        let client = client.clone();
        async move {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
            while client
                .get(format!("http://{me}:{admin_port}/health"))
                .send()
                .await
                .map_or(true, |r| !r.status().is_success())
            {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "gateway {me} not ready"
                );
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    };
    let connect = |me: &str, token: String| {
        let address = format!("{me}:{proxy_port}");
        async move {
            let mut connection = tokio::net::TcpStream::connect(address).await.unwrap();
            connection
                .write_all(format!("CONNECT one.one.one.one:443 HTTP/1.1\r\nHost: one.one.one.one:443\r\nProxy-Authorization: Bearer {token}\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut status = String::new();
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(8),
                BufReader::new(connection).read_line(&mut status),
            )
            .await;
            status
                .split_whitespace()
                .nth(1)
                .unwrap_or("none")
                .to_string()
        }
    };

    // Replica A (127.0.0.1) is stopped; only B (127.0.0.2) runs.
    let _b = start("127.0.0.2", "127.0.0.1");
    ready("127.0.0.2".into()).await;
    let mut config = h.config.clone();
    config.replicas = Some(alien_core::bindings::SandboxEgressReplicas {
        dns: String::new(),
        addresses: vec!["127.0.0.1".into(), "127.0.0.2".into()],
        port: admin_port,
        count: 2,
        min_protocol: 1,
    });
    let manager = SandboxEgressManager::with_endpoints(
        config,
        h.vault.clone(),
        format!("127.0.0.2:{proxy_port}"),
        format!("http://127.0.0.2:{admin_port}"),
    )
    .unwrap();
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["one.one.one.one"]})).unwrap();
    let started = std::time::Instant::now();
    let delivery = manager
        .create("box-1", &policy, now_unix().unwrap() + 600)
        .await
        .expect("create succeeds with one replica away");
    assert!(started.elapsed() < std::time::Duration::from_secs(15));
    assert_eq!(connect("127.0.0.2", delivery.token.clone()).await, "200");

    // A starts later: it refuses until synchronized, then serves the grant from its peer.
    let _a = start("127.0.0.1", "127.0.0.2");
    ready("127.0.0.1".into()).await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
    loop {
        let status = connect("127.0.0.1", delivery.token.clone()).await;
        if status == "200" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "replica A never served the grant (last status {status})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Azure publishes a new sandbox's grant under an identity minted before the platform create; that
/// first write follows the same one-replica rule as `create`.
#[tokio::test]
async fn publishing_a_new_azure_grant_succeeds_while_one_replica_is_away() {
    let h = Harness::new().await;
    let (manager, second, away) = two_replicas(&h, None).await;
    clear_grant(&h.state);
    clear_grant(&second);
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();
    let started = std::time::Instant::now();
    manager.publish_new_grant("box-1", &policy).await.unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert!(holds_grant(&h.state) && !holds_grant(&second));
    away.abort();
}

/// A live gateway applies a write before it fans it out, so with its peer away it answers 503 while
/// holding the grant (the harness's `transient_failures` is that "committed, then 503"). That
/// replica is an acknowledgement only because it serves the exact record when read back.
#[tokio::test]
async fn a_replica_that_commits_then_answers_503_because_its_peer_is_away_still_acknowledges() {
    let h = Harness::new().await;
    let (manager, second, away) = two_replicas(&h, None).await;
    clear_grant(&h.state);
    clear_grant(&second);
    h.state.transient_failures.store(1000, Ordering::SeqCst);
    let policy: EgressPolicy =
        serde_json::from_value(serde_json::json!({"allow":["example.com"]})).unwrap();
    manager.publish_new_grant("box-1", &policy).await.unwrap();
    assert!(holds_grant(&h.state));

    // A replica that refuses before committing is not an acknowledgement.
    clear_grant(&h.state);
    h.state.refuse.store(true, Ordering::SeqCst);
    let error = manager
        .publish_new_grant("box-1", &policy)
        .await
        .unwrap_err();
    assert!(error.retryable, "{error}");
    away.abort();
}
