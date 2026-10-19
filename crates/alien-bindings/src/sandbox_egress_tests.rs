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
) -> StatusCode {
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
    if state.refuse.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE;
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
        return StatusCode::SERVICE_UNAVAILABLE;
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
    pub(crate) fn policy(&self, id: &str) -> Option<EgressPolicy> {
        self.state
            .policies
            .lock()
            .unwrap()
            .get(id)
            .and_then(|row| row.1.clone())
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
            .put(format!("{}/v1/policies", self.manager.admin_url))
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
            transient_failures: AtomicUsize::new(0),
            records: StdMutex::new(Vec::new()),
            record_response: StdMutex::new(None),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let admin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/v1/policies", put(policy_request).delete(policy_request))
            .route("/v1/policies/record", axum::routing::get(policy_record))
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
    assert_eq!(manager.admin_url, "http://admin.internal:3129/");
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
