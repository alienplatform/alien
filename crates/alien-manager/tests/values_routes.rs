//! The value endpoints of a standalone manager, over HTTP: who may use them,
//! what they return, and that secrets are never shown back.

use std::sync::Arc;
use std::time::Duration;

use alien_core::Platform;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteTokenStore};
use alien_manager::traits::{
    CreateDeploymentGroupParams, CreateDeploymentParams, CreateTokenParams, DeploymentStore,
    TokenStore, TokenType,
};
use alien_manager::AlienManagerBuilder;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const CANARY: &str = "canary-7f3a9c1e-not-a-real-secret";

struct Server {
    url: String,
    admin: String,
    deployment_id: String,
    _dir: tempfile::TempDir,
}

fn admin() -> Subject {
    Subject {
        kind: SubjectKind::ServiceAccount {
            id: "test".to_string(),
        },
        workspace_id: "default".to_string(),
        scope: Scope::Workspace,
        role: Role::WorkspaceAdmin,
        bearer_token: String::new(),
    }
}

fn hash_token(raw: &str) -> (String, String) {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    (raw[..12].to_string(), format!("{:x}", hasher.finalize()))
}

async fn start() -> Server {
    start_with(false).await
}

async fn start_with(ephemeral_state_dir: bool) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}");

    let db_path = dir.path().join("test.db");
    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    );
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));
    let admin_raw = format!("ax_admin_{}", uuid::Uuid::new_v4().simple());
    let (key_prefix, key_hash) = hash_token(&admin_raw);
    token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix,
            key_hash,
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    let deployment_store = SqliteDeploymentStore::new(db.clone());
    let group = deployment_store
        .create_deployment_group(
            &admin(),
            CreateDeploymentGroupParams {
                name: "group".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();
    let deployment = deployment_store
        .create_deployment(
            &admin(),
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "customer".to_string(),
                deployment_group_id: group.id,
                platform: Platform::Local,
                base_platform: None,
                stack_settings: Default::default(),
                stack_state: None,
                environment_variables: None,
                public_subdomain: None,
                input_values: Default::default(),
                setup_item: None,
                deployment_token: None,
            },
        )
        .await
        .unwrap();

    let config = ManagerConfig {
        port,
        host: "127.0.0.1".to_string(),
        db_path: Some(db_path),
        state_dir: Some(dir.path().to_path_buf()),
        deployment_interval_secs: 999,
        heartbeat_interval_secs: 999,
        self_heartbeat_interval_secs: 999,
        otlp_endpoint: None,
        base_url: Some(url.clone()),
        releases_url: None,
        targets: vec![],
        supported_aws_regions: Vec::new(),
        disable_deployment_loop: true,
        disable_heartbeat_loop: true,
        enable_local_log_ingest: false,
        stop_local_containers_on_shutdown: false,
        allowed_origins: None,
        response_signing_key: b"test-signing-key".to_vec(),
    };
    let manager = AlienManagerBuilder::new(config)
        .token_store(token_store)
        .ephemeral_state_dir(ephemeral_state_dir)
        .with_standalone_defaults(&Default::default())
        .await
        .unwrap()
        .build()
        .await
        .unwrap();
    tokio::spawn(async move { manager.start_with_listener(listener).await.unwrap() });

    let client = reqwest::Client::new();
    for _ in 0..50 {
        if client
            .get(format!("{url}/health"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Server {
                url,
                admin: admin_raw,
                deployment_id: deployment.id,
                _dir: dir,
            };
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("manager did not become healthy");
}

impl Server {
    fn scope(&self) -> String {
        format!("dep:{}", self.deployment_id)
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new().request(method, format!("{}{path}", self.url));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn put(&self, values: Value) -> (u16, Value) {
        self.call(
            reqwest::Method::PUT,
            &format!("/v1/values/{}", self.scope()),
            Some(&self.admin),
            Some(json!({ "values": values })),
        )
        .await
    }

    async fn get(&self) -> (u16, Value) {
        self.call(
            reqwest::Method::GET,
            &format!("/v1/values/{}", self.scope()),
            Some(&self.admin),
            None,
        )
        .await
    }
}

#[tokio::test]
async fn the_manager_advertises_the_values_capability() {
    let server = start().await;
    let (status, info) = server
        .call(
            reqwest::Method::GET,
            "/v1/manager",
            Some(&server.admin),
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(info["capabilities"]["values"], 1);
}

#[tokio::test]
async fn values_are_stored_listed_and_deleted_and_secrets_are_never_shown() {
    let server = start().await;

    let (status, put) = server
        .put(json!([
            { "slot": "env:REGION", "value": "eu-west-1", "kind": "plain" },
            { "slot": "env:API_TOKEN", "value": CANARY, "kind": "secret" },
        ]))
        .await;
    assert_eq!(status, 200, "{put}");
    let slots = put["slots"].as_array().unwrap();
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[1]["slot"], "env:API_TOKEN");
    assert_eq!(slots[1]["kind"], "secret");
    assert_eq!(slots[1]["digest"].as_str().unwrap().len(), 64);
    assert!(slots[1]["updatedAt"].is_string());
    assert!(!put.to_string().contains(CANARY));

    let (status, listed) = server.get().await;
    assert_eq!(status, 200);
    let slots = listed["slots"].as_array().unwrap();
    assert_eq!(slots.len(), 2);
    let region = slots.iter().find(|s| s["slot"] == "env:REGION").unwrap();
    let token = slots.iter().find(|s| s["slot"] == "env:API_TOKEN").unwrap();
    assert_eq!(region["value"], "eu-west-1");
    assert!(token.get("value").is_none());
    assert!(!listed.to_string().contains(CANARY));
    assert_eq!(token["digest"], put["slots"][1]["digest"]);

    // Replacing a value changes its digest.
    let (_, replaced) = server
        .put(json!([{ "slot": "env:API_TOKEN", "value": "other", "kind": "secret" }]))
        .await;
    assert_ne!(replaced["slots"][0]["digest"], token["digest"]);

    let (status, _) = server
        .call(
            reqwest::Method::DELETE,
            &format!("/v1/values/{}/env:API_TOKEN", server.scope()),
            Some(&server.admin),
            None,
        )
        .await;
    assert_eq!(status, 204);
    let (_, listed) = server.get().await;
    assert_eq!(listed["slots"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn values_are_kept_in_the_encrypted_database_file() {
    let server = start().await;
    server
        .put(json!([{ "slot": "env:A", "value": CANARY, "kind": "secret" }]))
        .await;
    // Nothing the manager wrote to disk, including the write-ahead log, has the value in the clear.
    for entry in std::fs::read_dir(server._dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                !bytes.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()),
                "{} holds the value in the clear",
                path.display()
            );
        }
    }
    // The value is on disk, not only in memory: a second handle reads it.
    let db = SqliteDatabase::new(&server._dir.path().join("test.db").to_string_lossy())
        .await
        .unwrap();
    let backend = alien_manager::stores::sqlite::SqliteValueBackend::new(Arc::new(db));
    let scope = format!("dep:{}", server.deployment_id).parse().unwrap();
    let rows = alien_manager::values::ValueBackend::list(&backend, &scope)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value, CANARY);
}

#[tokio::test]
async fn an_ephemeral_state_dir_without_a_values_key_refuses_writes_and_keeps_reads() {
    let server = start_with(true).await;

    let (status, body) = server
        .put(json!([{ "slot": "env:A", "value": CANARY, "kind": "secret" }]))
        .await;
    assert_eq!(status, 501, "{body}");
    assert_eq!(body["code"], "VALUES_UNAVAILABLE");
    assert!(body["message"]
        .as_str()
        .unwrap()
        .contains("not on a persistent volume"));
    assert!(!body.to_string().contains(CANARY));

    let (status, listed) = server.get().await;
    assert_eq!(status, 200);
    assert!(listed["slots"].as_array().unwrap().is_empty());
    let (status, _) = server
        .call(
            reqwest::Method::DELETE,
            &format!("/v1/values/{}/env:A", server.scope()),
            Some(&server.admin),
            None,
        )
        .await;
    assert_eq!(status, 204);
}

#[tokio::test]
async fn limits_and_bad_requests_are_refused_without_changing_anything() {
    let server = start().await;

    let too_big = "x".repeat(24_001);
    let (status, error) = server
        .put(json!([
            { "slot": "env:SMALL", "value": "ok", "kind": "plain" },
            { "slot": "env:BIG", "value": too_big, "kind": "secret" },
        ]))
        .await;
    assert_eq!(status, 413, "{error}");
    let (_, listed) = server.get().await;
    assert!(
        listed["slots"].as_array().unwrap().is_empty(),
        "a rejected batch must write nothing"
    );

    let at_limit = "x".repeat(24_000);
    let (status, _) = server
        .put(json!([{ "slot": "env:BIG", "value": at_limit, "kind": "secret" }]))
        .await;
    assert_eq!(status, 200);

    let (status, _) = server
        .put(json!([{ "slot": "NOT_A_SLOT", "value": "v", "kind": "plain" }]))
        .await;
    assert!((400..500).contains(&status), "{status}");

    // An input is read back as JSON, so text that is not JSON is refused whole.
    let (status, error) = server
        .put(json!([
            { "slot": "env:FINE", "value": "ok", "kind": "plain" },
            { "slot": "input:size", "value": "not json", "kind": "plain" },
        ]))
        .await;
    assert_eq!(status, 400, "{error}");
    let (status, _) = server
        .put(json!([{ "slot": "input:size", "value": "{\"n\":3}", "kind": "plain" }]))
        .await;
    assert_eq!(status, 200);
    let (_, listed) = server.get().await;
    let slots: Vec<_> = listed["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["slot"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        slots,
        ["env:BIG", "input:size"],
        "FINE must not have been written"
    );
}

#[tokio::test]
async fn a_request_over_the_item_or_byte_limit_is_refused_and_states_the_limits() {
    let server = start().await;

    let many: Vec<Value> = (0..101)
        .map(|i| json!({ "slot": format!("env:V{i}"), "value": "x", "kind": "plain" }))
        .collect();
    let (status, error) = server.put(Value::Array(many)).await;
    assert_eq!(status, 413, "{error}");
    assert_eq!(error["code"], "VALUES_REQUEST_TOO_LARGE");
    assert!(error["message"].as_str().unwrap().contains("100 values"));

    let hundred: Vec<Value> = (0..100)
        .map(|i| json!({ "slot": format!("env:V{i}"), "value": "x", "kind": "plain" }))
        .collect();
    let (status, _) = server.put(Value::Array(hundred)).await;
    assert_eq!(status, 200);

    // A body over 8 MiB is refused before it is read in full.
    let body = json!({ "values": [{ "slot": "env:HUGE", "value": "x".repeat(9 * 1024 * 1024), "kind": "plain" }] });
    let (status, error) = server
        .call(
            reqwest::Method::PUT,
            &format!("/v1/values/{}", server.scope()),
            Some(&server.admin),
            Some(body),
        )
        .await;
    assert_eq!(status, 413, "{error}");
    assert_eq!(error["code"], "VALUES_REQUEST_TOO_LARGE");
    assert!(error["message"].as_str().unwrap().contains("8 MiB"));

    let (status, error) = server
        .put(json!([{ "slot": "env:BIG", "value": "x".repeat(24_001), "kind": "plain" }]))
        .await;
    assert_eq!(status, 413);
    assert!(
        error["message"].as_str().unwrap().contains("24,000 bytes"),
        "{error}"
    );
}

#[tokio::test]
async fn callers_without_rights_and_unknown_scopes_are_refused() {
    let server = start().await;
    let path = format!("/v1/values/{}", server.scope());
    let body = json!({ "values": [{ "slot": "env:A", "value": CANARY, "kind": "secret" }] });

    let (status, _) = server
        .call(reqwest::Method::PUT, &path, None, Some(body.clone()))
        .await;
    assert_eq!(status, 401);
    let (status, _) = server
        .call(
            reqwest::Method::PUT,
            &path,
            Some("ax_admin_wrong"),
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, 401);

    let (status, _) = server
        .call(
            reqwest::Method::PUT,
            "/v1/values/dep:dep_missing",
            Some(&server.admin),
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, 404);

    // Only deployments have values.
    let (status, _) = server
        .call(
            reqwest::Method::PUT,
            "/v1/values/link:key1",
            Some(&server.admin),
            Some(body.clone()),
        )
        .await;
    assert_eq!(status, 400);

    // Verification is only for migration, which a standalone manager has none of.
    let (status, _) = server
        .call(
            reqwest::Method::POST,
            &format!("{path}/verify"),
            Some(&server.admin),
            Some(json!({ "values": [{ "slot": "env:A", "value": "guess" }] })),
        )
        .await;
    assert_eq!(status, 403);

    let (_, listed) = server.get().await;
    assert!(listed["slots"].as_array().unwrap().is_empty());
}
