use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex as StdMutex, RwLock as StdRwLock};

use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::future::join_all;
use object_store::path::Path;
use object_store::PutPayload;
use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::RemoteBindings;

const DEPLOYMENT_ID: &str = "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MANAGER_ID: &str = "mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MANAGER_B_ID: &str = "mgr_ffffffffffffffffffffffffffff";
const PROJECT_ID: &str = "prj_cccccccccccccccccccccccccccc";
const DEPLOYMENT_GROUP_ID: &str = "dg_dddddddddddddddddddddddddddd";
const WORKSPACE_ID: &str = "ws_eeeeeeeeeeeeeeeeeeeeeeee";
const PLATFORM_TOKEN: &str = "platform-secret-token";
const GENERATED_MANAGER_TOKEN: &str = "generated-manager-token";

#[derive(Clone)]
struct ManagerAssignment {
    id: String,
    url: String,
    expected_token: Arc<StdRwLock<String>>,
}

#[derive(Debug)]
struct ManualClock {
    now: StdRwLock<DateTime<Utc>>,
}

impl ManualClock {
    fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: StdRwLock::new(now),
        }
    }

    fn set(&self, now: DateTime<Utc>) {
        *self.now.write().expect("manual clock write lock") = now;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.read().expect("manual clock read lock")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Option<serde_json::Value>,
}

#[derive(Clone)]
struct PlatformFixtureState {
    assignment: Arc<StdRwLock<ManagerAssignment>>,
    token_calls: Arc<AtomicUsize>,
    token_expires_in: Arc<StdRwLock<Option<f64>>>,
    requests: Arc<StdMutex<Vec<RecordedRequest>>>,
}

#[derive(Clone)]
struct ManagerFixtureState {
    calls: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    failure_response: Arc<StdRwLock<Option<(StatusCode, FailureBody)>>>,
    invalid_binding: Arc<AtomicBool>,
    advance_clock_to: Arc<StdRwLock<Option<DateTime<Utc>>>>,
    clock: Arc<ManualClock>,
    expires_at: Arc<StdRwLock<DateTime<Utc>>>,
    storage_path: String,
    expected_token: Arc<StdRwLock<String>>,
    requests: Arc<StdMutex<Vec<RecordedRequest>>>,
}

#[derive(Clone)]
enum FailureBody {
    Json(serde_json::Value),
    Text(String),
}

#[derive(Clone)]
struct GeneratedContractState {
    response: Arc<StdRwLock<(StatusCode, serde_json::Value)>>,
    requests: Arc<StdMutex<Vec<RecordedRequest>>>,
}

struct Fixture {
    api_url: String,
    clock: Arc<ManualClock>,
    platform_requests: Arc<StdMutex<Vec<RecordedRequest>>>,
    assignment: Arc<StdRwLock<ManagerAssignment>>,
    token_calls: Arc<AtomicUsize>,
    token_expires_in: Arc<StdRwLock<Option<f64>>>,
    manager: ManagerFixtureState,
    _storage_directory: TempDir,
}

impl Fixture {
    async fn new(now: DateTime<Utc>, expires_at: DateTime<Utc>) -> Self {
        let storage_directory = TempDir::new().expect("create fixture storage directory");
        let clock = Arc::new(ManualClock::new(now));
        let expected_token = Arc::new(StdRwLock::new("unminted-manager-token".to_string()));
        let manager = ManagerFixtureState {
            calls: Arc::new(AtomicUsize::new(0)),
            fail: Arc::new(AtomicBool::new(false)),
            failure_response: Arc::new(StdRwLock::new(None)),
            invalid_binding: Arc::new(AtomicBool::new(false)),
            advance_clock_to: Arc::new(StdRwLock::new(None)),
            clock: clock.clone(),
            expires_at: Arc::new(StdRwLock::new(expires_at)),
            storage_path: storage_directory.path().display().to_string(),
            expected_token: expected_token.clone(),
            requests: Arc::new(StdMutex::new(Vec::new())),
        };
        let manager_url = spawn_manager_server(manager.clone()).await;
        let assignment = Arc::new(StdRwLock::new(ManagerAssignment {
            id: MANAGER_ID.to_string(),
            url: manager_url,
            expected_token,
        }));

        let platform_requests = Arc::new(StdMutex::new(Vec::new()));
        let token_calls = Arc::new(AtomicUsize::new(0));
        let token_expires_in = Arc::new(StdRwLock::new(Some(300.0)));
        let api_url = spawn_platform_server(PlatformFixtureState {
            assignment: assignment.clone(),
            token_calls: token_calls.clone(),
            token_expires_in: token_expires_in.clone(),
            requests: platform_requests.clone(),
        })
        .await;

        Self {
            api_url,
            clock,
            platform_requests,
            assignment,
            token_calls,
            token_expires_in,
            manager,
            _storage_directory: storage_directory,
        }
    }

    async fn remote_provider(&self) -> Arc<RemoteBindingsProvider> {
        Arc::new(
            RemoteBindingsProvider::discover_local_fixture(
                DEPLOYMENT_ID,
                PLATFORM_TOKEN,
                Some(&self.api_url),
                self.clock.clone(),
            )
            .await
            .expect("discover assigned manager"),
        )
    }

    fn set_manager_expiry(&self, expires_at: DateTime<Utc>) {
        *self
            .manager
            .expires_at
            .write()
            .expect("manager expiry write lock") = expires_at;
    }

    fn fail_manager_with(&self, status: StatusCode, body: serde_json::Value) {
        *self
            .manager
            .failure_response
            .write()
            .expect("manager failure response write lock") =
            Some((status, FailureBody::Json(body)));
    }

    fn fail_manager_with_text(&self, status: StatusCode, body: impl Into<String>) {
        *self
            .manager
            .failure_response
            .write()
            .expect("manager failure response write lock") =
            Some((status, FailureBody::Text(body.into())));
    }

    fn advance_clock_during_next_resolve(&self, now: DateTime<Utc>) {
        *self
            .manager
            .advance_clock_to
            .write()
            .expect("manager clock advance write lock") = Some(now);
    }

    fn assign_manager(&self, id: &str, url: String, expected_token: Arc<StdRwLock<String>>) {
        *self
            .assignment
            .write()
            .expect("manager assignment write lock") = ManagerAssignment {
            id: id.to_string(),
            url,
            expected_token,
        };
    }

    fn set_binding_token_expiry(&self, expires_in: Option<f64>) {
        *self
            .token_expires_in
            .write()
            .expect("binding token expiry write lock") = expires_in;
    }
}

async fn spawn_server(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind fixture server");
    let address = listener.local_addr().expect("read fixture server address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve HTTP fixture");
    });
    format!("http://{address}")
}

async fn spawn_platform_server(state: PlatformFixtureState) -> String {
    let app = Router::new()
        .route("/v1/deployments/{id}", get(deployment_handler))
        .route(
            "/v1/managers/{id}/binding-token",
            post(binding_token_handler),
        )
        .with_state(state);
    spawn_server(app).await
}

async fn spawn_manager_server(state: ManagerFixtureState) -> String {
    let app = Router::new()
        .route("/v1/bindings/resolve", post(resolve_handler))
        .with_state(state);
    spawn_server(app).await
}

async fn spawn_generated_contract_server(state: GeneratedContractState) -> String {
    let app = Router::new()
        .route("/v1/bindings/resolve", post(generated_contract_handler))
        .with_state(state);
    spawn_server(app).await
}

fn authorization(headers: &HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

async fn deployment_handler(
    State(state): State<PlatformFixtureState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    state
        .requests
        .lock()
        .expect("platform requests lock")
        .push(RecordedRequest {
            method: "GET".to_string(),
            path: format!("/v1/deployments/{id}"),
            authorization: authorization(&headers),
            body: None,
        });
    let expected_authorization = format!("Bearer {PLATFORM_TOKEN}");
    if authorization(&headers).as_deref() != Some(expected_authorization.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if id != DEPLOYMENT_ID {
        return StatusCode::NOT_FOUND.into_response();
    }
    let manager_id = state
        .assignment
        .read()
        .expect("manager assignment read lock")
        .id
        .clone();
    Json(json!({
        "id": DEPLOYMENT_ID,
        "name": "remote-storage-test",
        "status": "running",
        "projectId": PROJECT_ID,
        "platform": "local",
        "deploymentProtocolVersion": 1,
        "deploymentGroupId": DEPLOYMENT_GROUP_ID,
        "stackSettings": {},
        "retryRequested": false,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "managerId": manager_id,
        "workspaceId": WORKSPACE_ID
    }))
    .into_response()
}

async fn binding_token_handler(
    State(state): State<PlatformFixtureState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    state
        .requests
        .lock()
        .expect("platform requests lock")
        .push(RecordedRequest {
            method: "POST".to_string(),
            path: format!("/v1/managers/{id}/binding-token"),
            authorization: authorization(&headers),
            body: Some(body.clone()),
        });
    let expected_authorization = format!("Bearer {PLATFORM_TOKEN}");
    if authorization(&headers).as_deref() != Some(expected_authorization.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if body != json!({ "deploymentId": DEPLOYMENT_ID }) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let assignment = state
        .assignment
        .read()
        .expect("manager assignment read lock")
        .clone();
    if id != assignment.id {
        return StatusCode::NOT_FOUND.into_response();
    }
    let token_number = state.token_calls.fetch_add(1, Ordering::SeqCst) + 1;
    let access_token = format!("manager-binding-token-{token_number}");
    *assignment
        .expected_token
        .write()
        .expect("expected manager token write lock") = access_token.clone();
    let expires_in = *state
        .token_expires_in
        .read()
        .expect("binding token expiry read lock");
    Json(json!({
        "accessToken": access_token,
        "expiresIn": expires_in,
        "tokenType": "Bearer",
        "managerUrl": assignment.url,
        "databaseId": null,
        "controlPlaneUrl": null
    }))
    .into_response()
}

async fn resolve_handler(
    State(state): State<ManagerFixtureState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    state.calls.fetch_add(1, Ordering::SeqCst);
    state
        .requests
        .lock()
        .expect("manager requests lock")
        .push(RecordedRequest {
            method: "POST".to_string(),
            path: "/v1/bindings/resolve".to_string(),
            authorization: authorization(&headers),
            body: Some(body),
        });
    let expected_authorization = format!(
        "Bearer {}",
        state
            .expected_token
            .read()
            .expect("expected manager token read lock")
    );
    if authorization(&headers).as_deref() != Some(expected_authorization.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(now) = state
        .advance_clock_to
        .write()
        .expect("manager clock advance write lock")
        .take()
    {
        state.clock.set(now);
    }
    if let Some((status, body)) = state
        .failure_response
        .read()
        .expect("manager failure response read lock")
        .clone()
    {
        return match body {
            FailureBody::Json(body) => (status, Json(body)).into_response(),
            FailureBody::Text(body) => (status, body).into_response(),
        };
    }
    if state.fail.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    let expires_at = *state.expires_at.read().expect("manager expiry read lock");
    let binding = if state.invalid_binding.load(Ordering::SeqCst) {
        json!({ "service": "local-storage" })
    } else {
        json!({
            "service": "local-storage",
            "storagePath": state.storage_path,
        })
    };
    let service = binding
        .get("service")
        .and_then(serde_json::Value::as_str)
        .expect("fixture binding service")
        .to_string();
    let mut binding = binding;
    binding
        .as_object_mut()
        .expect("fixture binding object")
        .remove("service");
    Json(json!({
            "service": service,
            "binding": binding,
            "clientConfig": {
                "platform": "local",
            "state_directory": state.storage_path,
        },
        "expiresAt": expires_at.to_rfc3339(),
    }))
    .into_response()
}

async fn generated_contract_handler(
    State(state): State<GeneratedContractState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    state
        .requests
        .lock()
        .expect("generated contract requests lock")
        .push(RecordedRequest {
            method: "POST".to_string(),
            path: "/v1/bindings/resolve".to_string(),
            authorization: authorization(&headers),
            body: Some(body),
        });
    let (status, body) = state
        .response
        .read()
        .expect("generated contract response lock")
        .clone();
    (status, Json(body)).into_response()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
        .expect("valid fixed timestamp")
        .with_timezone(&Utc)
        + ChronoDuration::seconds(second)
}

#[path = "tests/manager_contract.rs"]
mod manager_contract;

#[tokio::test]
async fn discovers_assigned_manager_and_caches_each_requested_storage() {
    let fixture = Fixture::new(at(0), at(3600)).await;
    let bindings = RemoteBindings::from_provider(fixture.remote_provider().await);
    let bindings_debug = format!("{bindings:?}");
    assert!(bindings_debug.contains("<redacted>"));
    assert!(!bindings_debug.contains(PLATFORM_TOKEN));
    assert!(!bindings_debug.contains("manager-binding-token"));
    let storage = bindings
        .storage("files")
        .await
        .expect("resolve remote Storage");
    storage
        .put(&Path::from("hello.txt"), PutPayload::from_static(b"hello"))
        .await
        .expect("write through resolved Storage");
    let result = storage
        .get(&Path::from("hello.txt"))
        .await
        .expect("read through same Storage handle");
    assert_eq!(
        result.bytes().await.expect("read fixture object bytes"),
        "hello"
    );

    let archive = bindings
        .storage("archive")
        .await
        .expect("resolve a second remote Storage resource");
    archive
        .put(
            &Path::from("archive.txt"),
            PutPayload::from_static(b"archive"),
        )
        .await
        .expect("reuse the second resource's cached lease");

    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        fixture
            .manager
            .requests
            .lock()
            .expect("manager requests lock")
            .as_slice(),
        &[
            RecordedRequest {
                method: "POST".to_string(),
                path: "/v1/bindings/resolve".to_string(),
                authorization: Some("Bearer manager-binding-token-1".to_string()),
                body: Some(json!({
                    "deploymentId": DEPLOYMENT_ID,
                    "resourceId": "files",
                })),
            },
            RecordedRequest {
                method: "POST".to_string(),
                path: "/v1/bindings/resolve".to_string(),
                authorization: Some("Bearer manager-binding-token-1".to_string()),
                body: Some(json!({
                    "deploymentId": DEPLOYMENT_ID,
                    "resourceId": "archive",
                })),
            },
        ]
    );
    assert_eq!(
        fixture
            .platform_requests
            .lock()
            .expect("platform requests lock")
            .as_slice(),
        &[
            RecordedRequest {
                method: "GET".to_string(),
                path: format!("/v1/deployments/{DEPLOYMENT_ID}"),
                authorization: Some(format!("Bearer {PLATFORM_TOKEN}")),
                body: None,
            },
            RecordedRequest {
                method: "POST".to_string(),
                path: format!("/v1/managers/{MANAGER_ID}/binding-token"),
                authorization: Some(format!("Bearer {PLATFORM_TOKEN}")),
                body: Some(json!({ "deploymentId": DEPLOYMENT_ID })),
            },
        ]
    );
    assert_eq!(fixture.token_calls.load(Ordering::SeqCst), 1);
}

#[path = "tests/access_behavior.rs"]
mod access_behavior;

#[tokio::test]
async fn refreshes_once_for_concurrent_operations_without_reconstructing_handle() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider.clone());
    let storage = bindings
        .storage("files")
        .await
        .expect("initial remote Storage resolution");
    storage
        .put(&Path::from("shared.txt"), PutPayload::from_static(b"value"))
        .await
        .expect("seed fixture object");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 1);

    fixture.clock.set(at(481));
    fixture.set_manager_expiry(at(3901));
    let operations = (0..16).map(|_| {
        let storage = storage.clone();
        async move {
            storage
                .head(&Path::from("shared.txt"))
                .await
                .expect("same Storage handle should refresh and read")
        }
    });
    let results = join_all(operations).await;

    assert_eq!(results.len(), 16);
    assert!(results.iter().all(|metadata| metadata.size == 5));
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        fixture
            .platform_requests
            .lock()
            .expect("platform requests lock")
            .len(),
        4,
        "refresh must periodically rediscover the assigned manager"
    );
}

#[tokio::test]
async fn short_valid_lease_is_not_immediately_refreshed() {
    let fixture = Fixture::new(at(0), at(60)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider);
    let storage = bindings
        .storage("files")
        .await
        .expect("initial short lease should resolve");
    storage
        .put(&Path::from("short.txt"), PutPayload::from_static(b"value"))
        .await
        .expect("short lease should remain usable");

    fixture.clock.set(at(1));
    storage
        .head(&Path::from("short.txt"))
        .await
        .expect("a valid short lease must not cause a refresh storm");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 1);

    fixture.clock.set(at(49));
    fixture.set_manager_expiry(at(3600));
    storage
        .head(&Path::from("short.txt"))
        .await
        .expect("lease should refresh inside its proportional window");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn existing_storage_handle_follows_manager_reassignment() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider);
    let storage = bindings
        .storage("files")
        .await
        .expect("initial manager should resolve Storage");
    storage
        .put(
            &Path::from("reassigned.txt"),
            PutPayload::from_static(b"value"),
        )
        .await
        .expect("seed fixture object through manager A");

    let manager_b_token = Arc::new(StdRwLock::new("unminted-manager-b-token".to_string()));
    let manager_b = ManagerFixtureState {
        calls: Arc::new(AtomicUsize::new(0)),
        fail: Arc::new(AtomicBool::new(false)),
        failure_response: Arc::new(StdRwLock::new(None)),
        invalid_binding: Arc::new(AtomicBool::new(false)),
        advance_clock_to: Arc::new(StdRwLock::new(None)),
        clock: fixture.clock.clone(),
        expires_at: Arc::new(StdRwLock::new(at(3901))),
        storage_path: fixture.manager.storage_path.clone(),
        expected_token: manager_b_token.clone(),
        requests: Arc::new(StdMutex::new(Vec::new())),
    };
    let manager_b_url = spawn_manager_server(manager_b.clone()).await;
    fixture.manager.fail.store(true, Ordering::SeqCst);
    fixture.assign_manager(MANAGER_B_ID, manager_b_url, manager_b_token);
    fixture.clock.set(at(481));

    let metadata = storage
        .head(&Path::from("reassigned.txt"))
        .await
        .expect("same handle should rediscover and use manager B");

    assert_eq!(metadata.size, 5);
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 1);
    assert_eq!(manager_b.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        manager_b.requests.lock().expect("manager B requests lock")[0].path,
        "/v1/bindings/resolve"
    );
    let platform_requests = fixture
        .platform_requests
        .lock()
        .expect("platform requests lock");
    assert_eq!(
        platform_requests[3].path,
        format!("/v1/managers/{MANAGER_B_ID}/binding-token")
    );
}

#[tokio::test]
async fn manager_rejection_is_rediscovered_once_then_preserved_without_cached_fallback() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider.clone());
    let storage = bindings
        .storage("files")
        .await
        .expect("initial remote Storage resolution");
    storage
        .put(
            &Path::from("private.txt"),
            PutPayload::from_static(b"value"),
        )
        .await
        .expect("seed fixture object");

    fixture.fail_manager_with(
        StatusCode::FORBIDDEN,
        json!({
            "code": "FORBIDDEN",
            "message": "Remote access was revoked",
            "retryable": false,
            "internal": false,
            "httpStatusCode": 403,
        }),
    );
    fixture.clock.set(at(481));
    let error = provider
        .load_storage("files")
        .await
        .expect_err("revoked access must not fall back to a cached lease");

    assert_eq!(error.code, "FORBIDDEN");
    assert!(!error.retryable);
    assert_eq!(error.http_status_code, Some(403));
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.token_calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn refresh_rechecks_expiry_after_the_network_request() {
    let fixture = Fixture::new(at(0), at(600)).await;
    let provider = fixture.remote_provider().await;
    let bindings = RemoteBindings::from_provider(provider.clone());
    let storage = bindings
        .storage("files")
        .await
        .expect("initial remote Storage resolution");
    storage
        .put(&Path::from("lease.txt"), PutPayload::from_static(b"value"))
        .await
        .expect("seed fixture object");

    fixture.manager.fail.store(true, Ordering::SeqCst);
    fixture.clock.set(at(481));
    fixture.advance_clock_during_next_resolve(at(600));
    let error = provider
        .load_storage("files")
        .await
        .expect_err("a lease that expired during refresh must not be used");

    assert_eq!(error.code, "REMOTE_ACCESS_FAILED");
    assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn remote_urls_require_https_except_for_loopback_development() {
    assert!(!validate_platform_base_url("https://api.example.com").unwrap());
    assert!(validate_platform_base_url("http://127.0.0.1:3000").unwrap());
    assert!(validate_platform_base_url("http://localhost:3000").unwrap());
    assert!(validate_platform_base_url("http://api.example.com").is_err());
    assert!(validate_manager_url("https://manager.example.com/", false).is_ok());
    assert!(validate_manager_url("http://127.0.0.1:3001/", true).is_ok());
    assert!(validate_manager_url("http://manager.example.com/", true).is_err());
    assert!(validate_manager_url("https://user@manager.example.com/", false).is_err());
    assert!(validate_manager_url("https://manager.example.com/prefix", false).is_err());

    for error in [
        validate_platform_base_url("not a URL").unwrap_err(),
        validate_manager_url("not a URL", false).unwrap_err(),
        validate_manager_url("http://manager.example.com/", true).unwrap_err(),
    ] {
        assert!(!error.retryable);
        assert_eq!(error.http_status_code, Some(400));
    }
}

#[test]
fn remote_lease_validation_rejects_refreshable_or_overbroad_credentials() {
    let aws = alien_core::AwsClientConfig {
        account_id: "123456789012".to_string(),
        region: "us-east-1".to_string(),
        credentials: alien_core::AwsCredentials::AccessKeys {
            access_key_id: "access".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };
    assert!(validate_aws_remote_client_config(&aws, at(3600)).is_err());

    let gcp = alien_core::GcpClientConfig {
        project_id: "project".to_string(),
        region: "us-central1".to_string(),
        credentials: alien_core::GcpCredentials::ServiceMetadata,
        service_overrides: None,
        project_number: None,
    };
    assert!(validate_gcp_remote_client_config(&gcp).is_err());

    let azure = alien_core::AzureClientConfig {
        subscription_id: "subscription".to_string(),
        tenant_id: "tenant".to_string(),
        region: Some("eastus".to_string()),
        credentials: alien_core::AzureCredentials::ScopedAccessTokens {
            tokens: HashMap::from([
                (
                    "https://storage.azure.com/.default".to_string(),
                    "storage".to_string(),
                ),
                (
                    "https://management.azure.com/.default".to_string(),
                    "management".to_string(),
                ),
            ]),
        },
        service_overrides: None,
    };
    assert!(validate_azure_remote_client_config(&azure).is_err());
}

#[path = "tests/retry_backoff.rs"]
mod retry_backoff;
