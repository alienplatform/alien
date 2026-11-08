//! The CLI's value flow against a real standalone manager and a fake platform
//! that records every request it receives.

use std::sync::{Arc, Mutex};
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
use alien_platform_api::types::EnvironmentVariableConfig;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::commands::deployment_actions::{
    save_environment_variables, set_inputs_task, InputChanges,
};

const CANARY: &str = "canary-3b9e61d0-not-a-real-secret";
const WORKSPACE: &str = "acme";

struct Manager {
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

async fn start_manager() -> Manager {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let db_path = dir.path().join("test.db");
    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    );
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));
    let admin_raw = format!("ax_admin_{}", uuid::Uuid::new_v4().simple());
    token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: admin_raw[..12].to_string(),
            key_hash: format!("{:x}", Sha256::digest(admin_raw.as_bytes())),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    let deployments = SqliteDeploymentStore::new(db.clone());
    let group = deployments
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
    let deployment = deployments
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
        port: listener.local_addr().unwrap().port(),
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
            .is_ok_and(|response| response.status().is_success())
        {
            return Manager {
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

impl Manager {
    async fn slots(&self) -> Value {
        reqwest::Client::new()
            .get(format!("{}/v1/values/dep:{}", self.url, self.deployment_id))
            .bearer_auth(&self.admin)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}

/// How the fake platform answers the value-access route.
#[derive(Clone)]
enum Access {
    /// Grants access to the real manager, with this token.
    Grant { manager_url: String, token: String },
    /// A platform without the route.
    NoRoute,
    /// A platform whose manager does not store values.
    NotServed,
}

#[derive(Clone)]
struct FakePlatformState {
    access: Access,
    inputs: Value,
    requests: Arc<Mutex<Vec<(String, String, String)>>>,
}

async fn platform_handler(
    State(platform): State<FakePlatformState>,
    method: Method,
    uri: Uri,
    body: String,
) -> Response {
    let route = uri.path().to_string();
    platform
        .requests
        .lock()
        .unwrap()
        .push((method.to_string(), route.clone(), body));
    match (
        method.as_str(),
        route.rsplit('/').next().unwrap_or_default(),
    ) {
        ("POST", "value-access") => match &platform.access {
            Access::Grant { manager_url, token } => axum::Json(json!({
                "managerUrl": manager_url,
                "token": token,
                "scope": "dep:x",
                "expiresAt": "2099-01-01T00:00:00Z",
            }))
            .into_response(),
            Access::NoRoute => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
            Access::NotServed => (
                StatusCode::CONFLICT,
                axum::Json(json!({
                    "code": "VALUES_NOT_SERVED_BY_MANAGER",
                    "message": "This deployment's manager does not store values.",
                    "retryable": false,
                    "internal": false,
                })),
            )
                .into_response(),
        },
        ("PATCH", "environment-variables") => (
            StatusCode::ACCEPTED,
            axum::Json(json!({ "message": "Environment variables updated." })),
        )
            .into_response(),
        ("GET", "inputs") => axum::Json(platform.inputs.clone()).into_response(),
        ("PATCH", "inputs") => axum::Json(json!({
            "outcome": "updated",
            "inputs": platform.inputs["inputs"],
            "providedInputIds": [],
            "runtimeUpdateRequested": false,
        }))
        .into_response(),
        _ => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}

struct FakePlatform {
    client: alien_platform_api::Client,
    requests: Arc<Mutex<Vec<(String, String, String)>>>,
}

async fn start_platform(access: Access, inputs: Value) -> FakePlatform {
    let requests = Arc::default();
    let app = Router::new()
        .fallback(any(platform_handler))
        .with_state(FakePlatformState {
            access,
            inputs,
            requests: Arc::clone(&requests),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    FakePlatform {
        client: alien_platform_api::Client::new(&url),
        requests,
    }
}

impl FakePlatform {
    fn requests(&self) -> Vec<(String, String, String)> {
        self.requests.lock().unwrap().clone()
    }

    fn body_of(&self, method: &str, last_segment: &str) -> Value {
        let requests = self.requests();
        let (_, _, body) = requests
            .iter()
            .find(|(m, path, _)| m == method && path.ends_with(last_segment))
            .unwrap_or_else(|| panic!("no {method} .../{last_segment} request in {requests:?}"));
        serde_json::from_str(body).unwrap()
    }
}

fn variables() -> Vec<EnvironmentVariableConfig> {
    serde_json::from_value(json!([
        {"name": "API_KEY", "value": CANARY, "type": "secret"},
        {"name": "REGION", "value": "eu-west-1", "type": "plain"},
    ]))
    .unwrap()
}

#[tokio::test]
async fn environment_values_go_to_the_manager_and_never_to_the_platform() {
    let manager = start_manager().await;
    let platform = start_platform(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: manager.admin.clone(),
        },
        Value::Null,
    )
    .await;

    let message = save_environment_variables(
        &platform.client,
        WORKSPACE,
        &manager.deployment_id,
        &variables(),
    )
    .await
    .expect("values are stored");
    assert_eq!(message, "Environment variables updated.");

    let requests = platform.requests();
    let routes: Vec<(&str, String)> = requests
        .iter()
        .map(|(method, path, _)| (method.as_str(), path.clone()))
        .collect();
    assert_eq!(
        routes,
        vec![
            (
                "POST",
                format!("/v1/deployments/{}/value-access", manager.deployment_id)
            ),
            (
                "PATCH",
                format!(
                    "/v1/deployments/{}/environment-variables",
                    manager.deployment_id
                )
            ),
        ]
    );
    for (method, path, body) in &requests {
        assert!(
            !body.contains(CANARY),
            "{method} {path} carried the secret: {body}"
        );
        assert!(
            !body.contains("eu-west-1"),
            "{method} {path} carried a plain value: {body}"
        );
    }
    assert_eq!(
        platform.body_of("POST", "value-access"),
        json!({"slots": ["env:API_KEY", "env:REGION"], "access": "write"})
    );

    let patch = platform.body_of("PATCH", "environment-variables");
    let rows = patch["variables"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let stored = manager.slots().await;
    for (row, (name, kind)) in rows
        .iter()
        .zip([("API_KEY", "secret"), ("REGION", "plain")])
    {
        assert_eq!(row["name"], name);
        assert_eq!(row["type"], kind);
        assert!(row.get("value").is_none(), "{row}");
        let reference = &row["valueRef"];
        assert_eq!(reference["slot"], format!("env:{name}"));
        assert_eq!(reference["kind"], kind);
        let entry = stored["slots"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["slot"] == format!("env:{name}"))
            .unwrap_or_else(|| panic!("the manager has no slot env:{name}: {stored}"));
        assert_eq!(entry["digest"], reference["digest"]);
        assert_eq!(entry["updatedAt"], reference["updatedAt"]);
    }
    // The manager keeps a plain value readable and a secret write-only.
    let shown = stored.to_string();
    assert!(!shown.contains(CANARY), "{shown}");
    assert!(shown.contains("eu-west-1"), "{shown}");
}

#[tokio::test]
async fn a_platform_without_the_route_gets_the_values_the_older_way() {
    let platform = start_platform(Access::NoRoute, Value::Null).await;

    save_environment_variables(
        &platform.client,
        WORKSPACE,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        &variables(),
    )
    .await
    .expect("the older path saves the values");

    let patch = platform.body_of("PATCH", "environment-variables");
    assert_eq!(patch["variables"][0]["value"], CANARY);
    assert!(patch["variables"][0].get("valueRef").is_none());
    assert_eq!(platform.requests().len(), 2);
}

#[tokio::test]
async fn a_manager_that_does_not_store_values_gets_the_older_path() {
    let platform = start_platform(Access::NotServed, Value::Null).await;

    save_environment_variables(
        &platform.client,
        WORKSPACE,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        &variables(),
    )
    .await
    .expect("the older path saves the values");

    let patch = platform.body_of("PATCH", "environment-variables");
    assert_eq!(patch["variables"][1]["value"], "eu-west-1");
}

#[tokio::test]
async fn a_manager_that_refuses_the_write_is_an_error_and_the_platform_gets_nothing() {
    let manager = start_manager().await;
    let platform = start_platform(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: "ax_dep_not-a-real-token".to_string(),
        },
        Value::Null,
    )
    .await;

    let error = save_environment_variables(
        &platform.client,
        WORKSPACE,
        &manager.deployment_id,
        &variables(),
    )
    .await
    .expect_err("a refused write must not fall back to the platform");
    assert_eq!(error.http_status_code, Some(401), "{error}");

    let requests = platform.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(requests[0].1.ends_with("/value-access"));
    assert_eq!(manager.slots().await["slots"], json!([]));
}

#[tokio::test]
async fn held_inputs_go_to_the_manager_as_json_and_the_platform_gets_a_reference() {
    let manager = start_manager().await;
    let inputs = json!({
        "inputs": [
            {"id": "api_key", "kind": "secret", "required": true},
            {"id": "region", "kind": "string", "required": false},
        ],
        "providedInputIds": [],
        "heldInputIds": ["api_key"],
        "values": {},
    });
    let platform = start_platform(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: manager.admin.clone(),
        },
        inputs,
    )
    .await;

    set_inputs_task(
        &platform.client,
        WORKSPACE,
        &manager.deployment_id,
        InputChanges {
            inputs: vec!["region=eu".to_string()],
            secret_inputs: vec![format!("api_key={CANARY}")],
            ..Default::default()
        },
        true,
    )
    .await
    .expect("inputs are saved");

    for (method, path, body) in platform.requests() {
        assert!(
            !body.contains(CANARY),
            "{method} {path} carried the secret: {body}"
        );
    }
    assert_eq!(
        platform.body_of("POST", "value-access"),
        json!({"slots": ["input:api_key"], "access": "write"})
    );
    let patch = platform.body_of("PATCH", "inputs");
    assert_eq!(patch["inputValues"], json!({"region": "eu"}));
    assert_eq!(patch["clearInputIds"], json!([]));
    let reference = &patch["inputValueRefs"]["api_key"];
    assert_eq!(reference["slot"], "input:api_key");
    assert_eq!(reference["kind"], "secret");
    let stored = manager.slots().await;
    assert_eq!(stored["slots"][0]["slot"], "input:api_key");
    assert_eq!(stored["slots"][0]["digest"], reference["digest"]);
    assert!(!stored.to_string().contains(CANARY));
}

#[tokio::test]
async fn inputs_the_manager_does_not_hold_stay_on_the_platform() {
    let inputs = json!({
        "inputs": [{"id": "region", "kind": "string", "required": false}],
        "providedInputIds": [],
        "values": {},
    });
    let platform = start_platform(Access::NoRoute, inputs).await;

    set_inputs_task(
        &platform.client,
        WORKSPACE,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        InputChanges {
            inputs: vec!["region=eu".to_string()],
            ..Default::default()
        },
        true,
    )
    .await
    .expect("inputs are saved");

    let patch = platform.body_of("PATCH", "inputs");
    assert_eq!(patch["inputValues"], json!({"region": "eu"}));
    assert!(patch.get("inputValueRefs").is_none());
    assert!(platform
        .requests()
        .iter()
        .all(|(_, path, _)| !path.ends_with("value-access")));
}
