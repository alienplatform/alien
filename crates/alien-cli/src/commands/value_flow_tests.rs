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

use crate::commands::deploy::create_deployment_with_answers;
use crate::commands::deployment_actions::{
    save_environment_variables, set_environment_variables_task, set_inputs_task, InputChanges,
};
use crate::commands::deployments::create_deployment_with_values;
use std::collections::{HashMap, HashSet};

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
    /// A platform whose manager stores values but cannot be reached.
    Unavailable,
}

#[derive(Clone)]
struct FakePlatformState {
    access: Access,
    inputs: Value,
    created_id: String,
    create_requires_input: Option<String>,
    environment_names: Arc<Mutex<Vec<String>>>,
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
        .push((method.to_string(), route.clone(), body.clone()));
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
            Access::NoRoute => old_platform_not_found(),
            Access::NotServed => not_served(),
            Access::Unavailable => unavailable(),
        },
        ("GET", "environment-variables") => {
            let variables: Vec<Value> = platform
                .environment_names
                .lock()
                .unwrap()
                .iter()
                .map(|name| json!({"name": name, "type": "secret", "targetResources": null}))
                .collect();
            axum::Json(json!({ "variables": variables })).into_response()
        }
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
        ("POST", "deployments") => create_deployment(&platform, &body),
        _ => old_platform_not_found(),
    }
}

fn not_served() -> Response {
    (
        StatusCode::CONFLICT,
        axum::Json(json!({
            "code": "VALUES_NOT_SERVED_BY_MANAGER",
            "message": "This deployment's manager does not store values.",
            "retryable": false,
            "internal": false,
        })),
    )
        .into_response()
}

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(json!({
            "code": "VALUES_MANAGER_UNAVAILABLE",
            "message": "The deployment's manager isn't reachable. Try again.",
            "retryable": true,
            "internal": false,
        })),
    )
        .into_response()
}

/// The 404 an older platform sends for a route it does not have, as captured
/// from a live older deployment: status 404, `text/plain; charset=UTF-8`, and
/// the 13 byte body `404 Not Found`.
fn old_platform_not_found() -> Response {
    let mut response = (StatusCode::NOT_FOUND, "404 Not Found").into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain; charset=UTF-8"),
    );
    response
}

fn create_deployment(platform: &FakePlatformState, body: &str) -> Response {
    let request: Value = serde_json::from_str(body).unwrap();
    let names_held = request
        .get("heldEnvNames")
        .and_then(Value::as_array)
        .is_some_and(|names| !names.is_empty());
    if names_held {
        match platform.access {
            Access::NotServed => return not_served(),
            Access::Unavailable => return unavailable(),
            _ => {}
        }
    }
    if let Some(input) = &platform.create_requires_input {
        if request["inputValues"].get(input).is_none() {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({
                    "code": "INPUT_REQUIRED",
                    "message": format!("Input '{input}' is required."),
                    "retryable": false,
                    "internal": false,
                })),
            )
                .into_response();
        }
    }
    axum::Json(json!({
        "deployment": {
            "id": platform.created_id,
            "name": "customer",
            "status": "pending",
            "projectId": "prj_0c29fq4a2yjb7kx3smwdgxlcxd7f",
            "platform": "aws",
            "deploymentProtocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "deploymentGroupId": "dg_r27ict8c7vcgsumpj90ackf7bnwt",
            "purpose": "application",
            "stackSettings": {},
            "releaseChannel": "stable",
            "retryRequested": false,
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "managerId": "mgr_0c29fq4a2yjb7kx3smwdgxlcxd7f",
            "workspaceId": "ws_It13CUaGEhLLAB87simX07Xb",
        },
        "deploymentModel": "push",
        "token": "ax_dep_not-a-real-token",
    }))
    .into_response()
}

struct FakePlatform {
    client: alien_platform_api::Client,
    environment_names: Arc<Mutex<Vec<String>>>,
    requests: Arc<Mutex<Vec<(String, String, String)>>>,
}

async fn start_platform(access: Access, inputs: Value) -> FakePlatform {
    start_platform_creating(access, inputs, "dep_0c0ffee0c0ffee0c0ffee0c0ffee", None).await
}

/// A fake platform whose create route answers with `created_id`, and rejects
/// the create unless `create_requires_input` is among the input values.
async fn start_platform_creating(
    access: Access,
    inputs: Value,
    created_id: &str,
    create_requires_input: Option<&str>,
) -> FakePlatform {
    let requests = Arc::default();
    let environment_names = Arc::<Mutex<Vec<String>>>::default();
    let app = Router::new()
        .fallback(any(platform_handler))
        .with_state(FakePlatformState {
            access,
            inputs,
            created_id: created_id.to_string(),
            create_requires_input: create_requires_input.map(str::to_string),
            environment_names: Arc::clone(&environment_names),
            requests: Arc::clone(&requests),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    FakePlatform {
        client: alien_platform_api::Client::new(&url),
        environment_names,
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

fn new_deployment(variables: Option<Vec<EnvironmentVariableConfig>>, inputs: Value) -> Value {
    let mut request = json!({
        "name": "customer",
        "platform": "aws",
        "project": "prj_0c29fq4a2yjb7kx3smwdgxlcxd7f",
        "inputValues": inputs,
    });
    if let Some(variables) = variables {
        request["environmentVariables"] = serde_json::to_value(variables).unwrap();
    }
    request
}

fn create_routes(platform: &FakePlatform) -> Vec<(String, String)> {
    platform
        .requests()
        .into_iter()
        .map(|(method, path, _)| (method, path))
        .collect()
}

#[tokio::test]
async fn a_created_deployment_gets_its_secret_env_through_the_manager() {
    let manager = start_manager().await;
    let platform = start_platform_creating(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: manager.admin.clone(),
        },
        Value::Null,
        &manager.deployment_id,
        None,
    )
    .await;
    let mut request = serde_json::from_value(new_deployment(Some(variables()), json!({}))).unwrap();

    let response = create_deployment_with_values(
        &platform.client,
        WORKSPACE,
        &WORKSPACE.try_into().unwrap(),
        &mut request,
    )
    .await
    .expect("the deployment is created");
    assert_eq!(response.deployment.id.as_str(), manager.deployment_id);

    assert_eq!(
        create_routes(&platform),
        vec![
            ("POST".to_string(), "/v1/deployments".to_string()),
            (
                "POST".to_string(),
                format!("/v1/deployments/{}/value-access", manager.deployment_id)
            ),
            (
                "PATCH".to_string(),
                format!(
                    "/v1/deployments/{}/environment-variables",
                    manager.deployment_id
                )
            ),
        ]
    );
    for (method, path, body) in platform.requests() {
        assert!(
            !body.contains(CANARY) && !body.contains("eu-west-1"),
            "{method} {path} carried a value: {body}"
        );
    }
    let created = platform.body_of("POST", "/v1/deployments");
    assert!(
        created.get("environmentVariables").is_none(),
        "the create request carries no values: {created}"
    );
    assert_eq!(created["heldEnvNames"], json!(["API_KEY", "REGION"]));
    let patch = platform.body_of("PATCH", "environment-variables");
    let rows = patch["variables"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "the update lists every name: {patch}");
    assert_eq!(rows[0]["name"], "API_KEY");
    assert_eq!(rows[0]["valueRef"]["slot"], "env:API_KEY");
    assert_eq!(rows[1]["name"], "REGION");
    assert_eq!(rows[1]["valueRef"]["slot"], "env:REGION");
    let stored = manager.slots().await;
    let slots: Vec<&Value> = stored["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| &entry["slot"])
        .collect();
    assert_eq!(slots, vec!["env:API_KEY", "env:REGION"]);
    assert!(!stored.to_string().contains(CANARY));
}

#[tokio::test]
async fn an_older_platform_gets_the_secret_env_in_a_second_request() {
    let platform = start_platform_creating(
        Access::NoRoute,
        Value::Null,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        None,
    )
    .await;
    let mut request = serde_json::from_value(new_deployment(Some(variables()), json!({}))).unwrap();

    create_deployment_with_values(
        &platform.client,
        WORKSPACE,
        &WORKSPACE.try_into().unwrap(),
        &mut request,
    )
    .await
    .expect("the deployment is created");

    assert_eq!(
        create_routes(&platform)
            .iter()
            .map(|(_, path)| path.rsplit('/').next().unwrap().to_string())
            .collect::<Vec<_>>(),
        vec!["deployments", "value-access", "environment-variables"]
    );
    let patch = platform.body_of("PATCH", "environment-variables");
    assert_eq!(patch["variables"][0]["value"], CANARY);
    assert_eq!(patch["variables"][1]["value"], "eu-west-1");
}

fn variables_plain_only() -> Vec<EnvironmentVariableConfig> {
    serde_json::from_value(json!([
        {"name": "REGION", "value": "eu-west-1", "type": "plain"},
    ]))
    .unwrap()
}

#[tokio::test]
async fn a_manager_that_does_not_store_values_gets_the_older_create_and_a_second_request() {
    let platform = start_platform_creating(
        Access::NotServed,
        Value::Null,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        None,
    )
    .await;
    let mut request = serde_json::from_value(new_deployment(Some(variables()), json!({}))).unwrap();

    create_deployment_with_values(
        &platform.client,
        WORKSPACE,
        &WORKSPACE.try_into().unwrap(),
        &mut request,
    )
    .await
    .expect("the deployment is created");

    let requests = platform.requests();
    let bodies: Vec<Value> = requests
        .iter()
        .filter(|(method, path, _)| method == "POST" && path == "/v1/deployments")
        .map(|(_, _, body)| serde_json::from_str(body).unwrap())
        .collect();
    assert_eq!(bodies.len(), 2, "one refused create and one retry");
    assert_eq!(bodies[0]["heldEnvNames"], json!(["API_KEY", "REGION"]));
    assert!(bodies[0].get("environmentVariables").is_none());
    assert!(
        bodies[1]
            .get("heldEnvNames")
            .is_none_or(|names| names == &json!([])),
        "the retry names nothing held: {}",
        bodies[1]
    );
    let sent = bodies[1]["environmentVariables"].as_array().unwrap();
    assert_eq!(sent.len(), 1, "the retry carries plain variables only");
    assert_eq!(sent[0]["name"], "REGION");
    for (method, path, body) in &requests {
        assert!(
            !body.contains(CANARY) || method == "PATCH",
            "{method} {path} carried the secret outside the update: {body}"
        );
    }
    let patch = platform.body_of("PATCH", "environment-variables");
    assert_eq!(patch["variables"][0]["value"], CANARY);
    assert_eq!(patch["variables"][1]["value"], "eu-west-1");
}

#[tokio::test]
async fn plain_variables_alone_are_created_the_older_way_when_the_manager_stores_no_values() {
    let platform = start_platform_creating(
        Access::NotServed,
        Value::Null,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        None,
    )
    .await;
    let mut request =
        serde_json::from_value(new_deployment(Some(variables_plain_only()), json!({}))).unwrap();

    create_deployment_with_values(
        &platform.client,
        WORKSPACE,
        &WORKSPACE.try_into().unwrap(),
        &mut request,
    )
    .await
    .expect("the deployment is created");

    assert_eq!(
        create_routes(&platform),
        vec![
            ("POST".to_string(), "/v1/deployments".to_string()),
            ("POST".to_string(), "/v1/deployments".to_string()),
        ],
        "no second request: the retry already carries the plain variables"
    );
    let retry = platform.requests()[1].2.clone();
    let retry: Value = serde_json::from_str(&retry).unwrap();
    assert_eq!(retry["environmentVariables"][0]["name"], "REGION");
    assert_eq!(retry["environmentVariables"][0]["value"], "eu-west-1");
}

#[tokio::test]
async fn an_unreachable_manager_fails_the_create_and_never_falls_back() {
    let platform = start_platform_creating(
        Access::Unavailable,
        Value::Null,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        None,
    )
    .await;
    let mut request = serde_json::from_value(new_deployment(Some(variables()), json!({}))).unwrap();

    let error = create_deployment_with_values(
        &platform.client,
        WORKSPACE,
        &WORKSPACE.try_into().unwrap(),
        &mut request,
    )
    .await
    .expect_err("the create is refused");

    assert!(error.to_string().contains("isn't reachable"), "{error}");
    assert_eq!(
        create_routes(&platform),
        vec![("POST".to_string(), "/v1/deployments".to_string())],
        "no retry that would send the values elsewhere"
    );
    for (method, path, body) in platform.requests() {
        assert!(!body.contains(CANARY), "{method} {path}: {body}");
    }
}

#[tokio::test]
async fn an_unreachable_manager_fails_an_env_update_and_the_values_never_reach_the_platform() {
    let platform = start_platform(Access::Unavailable, Value::Null).await;

    let error = save_environment_variables(
        &platform.client,
        WORKSPACE,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        &variables(),
    )
    .await
    .expect_err("the update fails");

    assert!(error.to_string().contains("isn't reachable"), "{error}");
    let requests = platform.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(requests[0].1.ends_with("value-access"));
    for (method, path, body) in &requests {
        assert!(!body.contains(CANARY), "{method} {path}: {body}");
    }
}

#[tokio::test]
async fn a_failed_value_write_keeps_the_deployment_and_names_the_retry() {
    let manager = start_manager().await;
    let platform = start_platform_creating(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: "ax_dep_not-a-real-token".to_string(),
        },
        Value::Null,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        None,
    )
    .await;
    let mut request = serde_json::from_value(new_deployment(Some(variables()), json!({}))).unwrap();

    let error = create_deployment_with_values(
        &platform.client,
        WORKSPACE,
        &WORKSPACE.try_into().unwrap(),
        &mut request,
    )
    .await
    .expect_err("the value write is refused");

    let message = error.to_string();
    assert!(
        message.contains("dep_0c0ffee0c0ffee0c0ffee0c0ffee"),
        "{message}"
    );
    assert!(
        message.contains("alien deployments set-env dep_0c0ffee0c0ffee0c0ffee0c0ffee"),
        "{message}"
    );
    assert!(message.contains("--secret API_KEY="), "{message}");
    assert!(
        !message.contains(CANARY),
        "the retry must not print the value: {message}"
    );
    assert!(
        platform
            .requests()
            .iter()
            .all(|(method, _, _)| method != "DELETE"),
        "the deployment is not deleted"
    );
    for (method, path, body) in platform.requests() {
        assert!(!body.contains(CANARY), "{method} {path}: {body}");
    }
}

fn secret_answers() -> (HashMap<String, Value>, HashSet<String>) {
    (
        HashMap::from([
            ("api_key".to_string(), json!(CANARY)),
            ("region".to_string(), json!("eu")),
        ]),
        HashSet::from(["api_key".to_string()]),
    )
}

fn deploy_request(
    answers: &HashMap<String, Value>,
) -> alien_platform_api::types::NewDeploymentRequest {
    serde_json::from_value(new_deployment(None, json!(answers))).unwrap()
}

#[tokio::test]
async fn a_created_deployment_gets_its_secret_answers_through_the_manager() {
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
    let platform = start_platform_creating(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: manager.admin.clone(),
        },
        inputs,
        &manager.deployment_id,
        None,
    )
    .await;
    let (answers, secrets) = secret_answers();
    let mut request = deploy_request(&answers);

    create_deployment_with_answers(
        &platform.client,
        WORKSPACE,
        &mut request,
        &answers,
        &secrets,
    )
    .await
    .expect("the deployment is created");

    for (method, path, body) in platform.requests() {
        assert!(
            !body.contains(CANARY),
            "{method} {path} carried the secret: {body}"
        );
    }
    let created = platform.body_of("POST", "/v1/deployments");
    assert_eq!(created["inputValues"], json!({"region": "eu"}));
    assert_eq!(
        platform.body_of("POST", "value-access"),
        json!({"slots": ["input:api_key"], "access": "write"})
    );
    let patch = platform.body_of("PATCH", "inputs");
    assert_eq!(patch["inputValueRefs"]["api_key"]["slot"], "input:api_key");
    assert_eq!(manager.slots().await["slots"][0]["slot"], "input:api_key");
}

#[tokio::test]
async fn a_platform_that_needs_the_secret_answer_in_the_create_gets_it_there() {
    let inputs = json!({
        "inputs": [{"id": "api_key", "kind": "secret", "required": true}],
        "providedInputIds": [],
        "values": {},
    });
    let platform = start_platform_creating(
        Access::NoRoute,
        inputs,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        Some("api_key"),
    )
    .await;
    let (answers, secrets) = secret_answers();
    let mut request = deploy_request(&answers);

    create_deployment_with_answers(
        &platform.client,
        WORKSPACE,
        &mut request,
        &answers,
        &secrets,
    )
    .await
    .expect("the retry with the answer succeeds");

    let creates: Vec<Value> = platform
        .requests()
        .iter()
        .filter(|(method, path, _)| method == "POST" && path == "/v1/deployments")
        .map(|(_, _, body)| serde_json::from_str(body).unwrap())
        .collect();
    assert_eq!(creates.len(), 2, "{creates:?}");
    assert!(creates[0]["inputValues"].get("api_key").is_none());
    assert_eq!(creates[1]["inputValues"]["api_key"], CANARY);
    assert!(
        platform
            .requests()
            .iter()
            .all(|(_, path, _)| !path.ends_with("value-access")),
        "nothing was withheld, so there is nothing to store"
    );
}

#[tokio::test]
async fn a_failed_answer_write_keeps_the_deployment_and_names_the_retry() {
    let manager = start_manager().await;
    let inputs = json!({
        "inputs": [{"id": "api_key", "kind": "secret", "required": true}],
        "providedInputIds": [],
        "heldInputIds": ["api_key"],
        "values": {},
    });
    let platform = start_platform_creating(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: "ax_dep_not-a-real-token".to_string(),
        },
        inputs,
        "dep_0c0ffee0c0ffee0c0ffee0c0ffee",
        None,
    )
    .await;
    let (answers, secrets) = secret_answers();
    let mut request = deploy_request(&answers);

    let error = create_deployment_with_answers(
        &platform.client,
        WORKSPACE,
        &mut request,
        &answers,
        &secrets,
    )
    .await
    .expect_err("the answer write is refused");

    let message = error.to_string();
    assert!(
        message.contains("dep_0c0ffee0c0ffee0c0ffee0c0ffee"),
        "{message}"
    );
    assert!(
        message.contains("alien deployments set-inputs"),
        "{message}"
    );
    assert!(message.contains("api_key"), "{message}");
    assert!(!message.contains(CANARY), "{message}");
    for (method, path, body) in platform.requests() {
        assert!(!body.contains(CANARY), "{method} {path}: {body}");
    }
}

impl Manager {
    async fn slot_names(&self) -> Vec<String> {
        self.slots().await["slots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["slot"].as_str().unwrap().to_string())
            .collect()
    }
}

#[tokio::test]
async fn variables_dropped_by_a_replace_are_removed_from_the_manager() {
    let manager = start_manager().await;
    let platform = start_platform(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: manager.admin.clone(),
        },
        Value::Null,
    )
    .await;
    let mut before = variables();
    before.push(
        serde_json::from_value(json!({"name": "OLD_TOKEN", "value": CANARY, "type": "secret"}))
            .unwrap(),
    );
    save_environment_variables(&platform.client, WORKSPACE, &manager.deployment_id, &before)
        .await
        .unwrap();
    assert_eq!(
        manager.slot_names().await,
        vec!["env:API_KEY", "env:OLD_TOKEN", "env:REGION"]
    );
    *platform.environment_names.lock().unwrap() =
        vec!["API_KEY".into(), "OLD_TOKEN".into(), "REGION".into()];
    let deployment: alien_platform_api::types::DeploymentDetailResponse =
        serde_json::from_value(json!({
            "id": manager.deployment_id,
            "name": "customer",
            "status": "running",
            "projectId": "prj_0c29fq4a2yjb7kx3smwdgxlcxd7f",
            "platform": "aws",
            "deploymentProtocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "deploymentGroupId": "dg_r27ict8c7vcgsumpj90ackf7bnwt",
            "purpose": "application",
            "stackSettings": {},
            "releaseChannel": "stable",
            "retryRequested": false,
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "managerId": "mgr_0c29fq4a2yjb7kx3smwdgxlcxd7f",
            "workspaceId": "ws_It13CUaGEhLLAB87simX07Xb",
        }))
        .expect("a deployment detail");

    set_environment_variables_task(
        &platform.client,
        WORKSPACE,
        &deployment,
        variables(),
        crate::interaction::ConfirmationMode::Skip,
        true,
    )
    .await
    .expect("the variables are replaced");

    assert_eq!(
        manager.slot_names().await,
        vec!["env:API_KEY", "env:REGION"]
    );
}

#[tokio::test]
async fn a_cleared_held_input_is_removed_from_the_manager() {
    let manager = start_manager().await;
    let inputs = json!({
        "inputs": [{"id": "api_key", "kind": "secret", "required": false}],
        "providedInputIds": ["api_key"],
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
            secret_inputs: vec![format!("api_key={CANARY}")],
            ..Default::default()
        },
        true,
    )
    .await
    .unwrap();
    assert_eq!(manager.slot_names().await, vec!["input:api_key"]);

    set_inputs_task(
        &platform.client,
        WORKSPACE,
        &manager.deployment_id,
        InputChanges {
            clear: vec!["api_key".to_string()],
            ..Default::default()
        },
        true,
    )
    .await
    .unwrap();

    assert!(manager.slot_names().await.is_empty());
}

#[tokio::test]
async fn a_failed_removal_is_a_warning_and_the_command_still_succeeds() {
    let manager = start_manager().await;
    let inputs = json!({
        "inputs": [{"id": "api_key", "kind": "secret", "required": false}],
        "providedInputIds": ["api_key"],
        "heldInputIds": ["api_key"],
        "values": {},
    });
    let platform = start_platform(
        Access::Grant {
            manager_url: manager.url.clone(),
            token: "ax_dep_not-a-real-token".to_string(),
        },
        inputs,
    )
    .await;

    set_inputs_task(
        &platform.client,
        WORKSPACE,
        &manager.deployment_id,
        InputChanges {
            clear: vec!["api_key".to_string()],
            ..Default::default()
        },
        true,
    )
    .await
    .expect("the platform change is made even when the manager cannot be reached for removal");

    assert_eq!(
        platform.body_of("PATCH", "inputs")["clearInputIds"],
        json!(["api_key"])
    );
}
