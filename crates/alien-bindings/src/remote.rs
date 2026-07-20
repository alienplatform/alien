//! Remote, resource-scoped binding resolution for app-facing clients.
//!
//! The Platform API is used only to discover the deployment's assigned manager.
//! Binding topology and short-lived cloud credentials always come from that
//! manager's resource-scoped resolver.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::SdkResultExt;
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use crate::error::{ErrorData, Result};
use crate::provider::BindingsProvider;
use crate::traits::{
    ArtifactRegistry, BindingsProviderApi, Build, Container, Kv, Postgres, Queue, ServiceAccount,
    Storage, Vault, Worker,
};

const DEFAULT_PLATFORM_API_URL: &str = "https://api.alien.dev";
const REFRESH_SKEW_SECONDS: i64 = 300;
const REMOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

trait Clock: Send + Sync + fmt::Debug {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug)]
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// App-facing provider for resource-scoped remote Storage bindings.
///
/// The bearer token and all returned client configurations are deliberately
/// omitted from `Debug` output.
pub struct RemoteBindingsProvider {
    source: Arc<RemoteBindingSource>,
    resolvers: RwLock<HashMap<String, Arc<RemoteStorageResolver>>>,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for RemoteBindingsProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteBindingsProvider")
            .field("source", &self.source)
            .field("resolvers", &"<redacted>")
            .finish()
    }
}

impl RemoteBindingsProvider {
    /// Discovers the deployment's assigned manager through the caller-scoped
    /// Platform API and creates a lazy remote provider.
    pub async fn for_remote_deployment(
        deployment_id: &str,
        token: &str,
        api_base_url: Option<&str>,
    ) -> Result<Self> {
        Self::discover(deployment_id, token, api_base_url, Arc::new(SystemClock)).await
    }

    async fn discover(
        deployment_id: &str,
        token: &str,
        api_base_url: Option<&str>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let base_url = api_base_url.unwrap_or(DEFAULT_PLATFORM_API_URL);
        let auth_value = format!("Bearer {token}");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth_value)
                .into_alien_error()
                .context(ErrorData::RemoteAccessFailed {
                    operation: "build Platform API client with token".to_string(),
                })?,
        );
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(REMOTE_REQUEST_TIMEOUT)
            .build()
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "build remote binding HTTP client".to_string(),
            })?;
        let platform = alien_platform_api::Client::new_with_client(base_url, http.clone());

        let deployment = platform
            .get_deployment()
            .id(deployment_id)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "fetch deployment from Platform API".to_string(),
            })?
            .into_inner();
        let manager_id = deployment.manager_id.to_string();
        let manager = platform
            .get_manager()
            .id(&manager_id)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "fetch assigned manager from Platform API".to_string(),
            })?
            .into_inner();
        let manager_url = manager.url.ok_or_else(|| {
            AlienError::new(ErrorData::RemoteAccessFailed {
                operation: "assigned manager has no reachable URL".to_string(),
            })
        })?;

        Ok(Self {
            source: Arc::new(RemoteBindingSource {
                deployment_id: deployment_id.to_string(),
                manager_url,
                http,
            }),
            resolvers: RwLock::new(HashMap::new()),
            clock,
        })
    }

    async fn resolver(&self, resource_id: &str) -> Arc<RemoteStorageResolver> {
        if let Some(resolver) = self.resolvers.read().await.get(resource_id).cloned() {
            return resolver;
        }

        let mut resolvers = self.resolvers.write().await;
        resolvers
            .entry(resource_id.to_string())
            .or_insert_with(|| {
                Arc::new(RemoteStorageResolver {
                    source: self.source.clone(),
                    resource_id: resource_id.to_string(),
                    cache: RwLock::new(None),
                    refresh_lock: Mutex::new(()),
                    clock: self.clock.clone(),
                })
            })
            .clone()
    }

    fn unsupported(binding_kind: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("load remote {binding_kind} binding"),
            reason: "remote bindings v0 supports Storage only".to_string(),
        })
    }
}

#[async_trait]
impl BindingsProviderApi for RemoteBindingsProvider {
    async fn load_storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>> {
        self.resolver(binding_name).await.storage().await
    }

    async fn load_build(&self, _binding_name: &str) -> Result<Arc<dyn Build>> {
        Err(Self::unsupported("Build"))
    }

    async fn load_artifact_registry(
        &self,
        _binding_name: &str,
    ) -> Result<Arc<dyn ArtifactRegistry>> {
        Err(Self::unsupported("ArtifactRegistry"))
    }

    async fn load_vault(&self, _binding_name: &str) -> Result<Arc<dyn Vault>> {
        Err(Self::unsupported("Vault"))
    }

    async fn load_kv(&self, _binding_name: &str) -> Result<Arc<dyn Kv>> {
        Err(Self::unsupported("Kv"))
    }

    async fn load_postgres(&self, _binding_name: &str) -> Result<Arc<dyn Postgres>> {
        Err(Self::unsupported("Postgres"))
    }

    async fn load_queue(&self, _binding_name: &str) -> Result<Arc<dyn Queue>> {
        Err(Self::unsupported("Queue"))
    }

    async fn load_worker(&self, _binding_name: &str) -> Result<Arc<dyn Worker>> {
        Err(Self::unsupported("Worker"))
    }

    async fn load_container(&self, _binding_name: &str) -> Result<Arc<dyn Container>> {
        Err(Self::unsupported("Container"))
    }

    async fn load_service_account(&self, _binding_name: &str) -> Result<Arc<dyn ServiceAccount>> {
        Err(Self::unsupported("ServiceAccount"))
    }
}

struct RemoteBindingSource {
    deployment_id: String,
    manager_url: String,
    http: reqwest::Client,
}

impl fmt::Debug for RemoteBindingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteBindingSource")
            .field("deployment_id", &self.deployment_id)
            .field("manager_url", &self.manager_url)
            .field("credentials", &"<redacted>")
            .finish()
    }
}

impl RemoteBindingSource {
    async fn resolve(&self, resource_id: &str) -> Result<ResolvedRemoteBinding> {
        let url = format!(
            "{}/v1/bindings/resolve",
            self.manager_url.trim_end_matches('/')
        );
        let response = self
            .http
            .post(&url)
            .json(&ResolveBindingRequest {
                deployment_id: &self.deployment_id,
                resource_id,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("resolve remote Storage binding '{resource_id}'"),
            })?;
        let response = response.error_for_status().into_alien_error().context(
            ErrorData::RemoteAccessFailed {
                operation: format!(
                    "resolve remote Storage binding '{resource_id}' (non-success status)"
                ),
            },
        )?;

        response
            .json::<ResolvedRemoteBinding>()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("parse remote Storage binding '{resource_id}'"),
            })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolveBindingRequest<'a> {
    deployment_id: &'a str,
    resource_id: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedRemoteBinding {
    binding: serde_json::Value,
    client_config: alien_core::ClientConfig,
    expires_at: DateTime<Utc>,
}

struct CachedRemoteBinding {
    provider: Arc<BindingsProvider>,
    expires_at: DateTime<Utc>,
}

struct RemoteStorageResolver {
    source: Arc<RemoteBindingSource>,
    resource_id: String,
    cache: RwLock<Option<CachedRemoteBinding>>,
    refresh_lock: Mutex<()>,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for RemoteStorageResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteStorageResolver")
            .field("source", &self.source)
            .field("resource_id", &self.resource_id)
            .field("cache", &"<redacted>")
            .finish()
    }
}

impl RemoteStorageResolver {
    async fn storage(&self) -> Result<Arc<dyn Storage>> {
        self.provider().await?.load_storage(&self.resource_id).await
    }

    async fn provider(&self) -> Result<Arc<BindingsProvider>> {
        let now = self.clock.now();
        if let Some(provider) = self.fresh_cached(now).await {
            return Ok(provider);
        }

        let _flight = self.refresh_lock.lock().await;
        let now = self.clock.now();
        if let Some(provider) = self.fresh_cached(now).await {
            return Ok(provider);
        }

        match self.source.resolve(&self.resource_id).await {
            Ok(resolved) => self.cache_resolved(resolved, now).await,
            Err(error) => {
                if let Some(provider) = self.unexpired_cached(now).await {
                    debug!(
                        deployment_id = %self.source.deployment_id,
                        resource_id = %self.resource_id,
                        "Remote binding refresh failed before lease expiry; using cached credentials"
                    );
                    Ok(provider)
                } else {
                    Err(error)
                }
            }
        }
    }

    async fn cache_resolved(
        &self,
        resolved: ResolvedRemoteBinding,
        now: DateTime<Utc>,
    ) -> Result<Arc<BindingsProvider>> {
        if resolved.expires_at <= now {
            return Err(AlienError::new(ErrorData::RemoteAccessFailed {
                operation: format!(
                    "manager returned an expired lease for Storage binding '{}'",
                    self.resource_id
                ),
            }));
        }

        let provider = Arc::new(BindingsProvider::new(
            resolved.client_config,
            HashMap::from([(self.resource_id.clone(), resolved.binding)]),
        )?);
        let mut cache = self.cache.write().await;
        *cache = Some(CachedRemoteBinding {
            provider: provider.clone(),
            expires_at: resolved.expires_at,
        });
        Ok(provider)
    }

    async fn fresh_cached(&self, now: DateTime<Utc>) -> Option<Arc<BindingsProvider>> {
        let cache = self.cache.read().await;
        cache.as_ref().and_then(|cached| {
            let refresh_at = cached.expires_at - ChronoDuration::seconds(REFRESH_SKEW_SECONDS);
            (now < refresh_at).then(|| cached.provider.clone())
        })
    }

    async fn unexpired_cached(&self, now: DateTime<Utc>) -> Option<Arc<BindingsProvider>> {
        let cache = self.cache.read().await;
        cache
            .as_ref()
            .and_then(|cached| (now < cached.expires_at).then(|| cached.provider.clone()))
    }
}

#[cfg(test)]
mod tests {
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
    use crate::Bindings;

    const DEPLOYMENT_ID: &str = "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const MANAGER_ID: &str = "mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const PROJECT_ID: &str = "prj_cccccccccccccccccccccccccccc";
    const DEPLOYMENT_GROUP_ID: &str = "dg_dddddddddddddddddddddddddddd";
    const WORKSPACE_ID: &str = "ws_eeeeeeeeeeeeeeeeeeeeeeee";
    const TOKEN: &str = "remote-secret-token";

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
        manager_url: String,
        requests: Arc<StdMutex<Vec<RecordedRequest>>>,
    }

    #[derive(Clone)]
    struct ManagerFixtureState {
        calls: Arc<AtomicUsize>,
        fail: Arc<AtomicBool>,
        expires_at: Arc<StdRwLock<DateTime<Utc>>>,
        storage_path: String,
        requests: Arc<StdMutex<Vec<RecordedRequest>>>,
    }

    struct Fixture {
        api_url: String,
        clock: Arc<ManualClock>,
        platform_requests: Arc<StdMutex<Vec<RecordedRequest>>>,
        manager: ManagerFixtureState,
        _storage_directory: TempDir,
    }

    impl Fixture {
        async fn new(now: DateTime<Utc>, expires_at: DateTime<Utc>) -> Self {
            let storage_directory = TempDir::new().expect("create fixture storage directory");
            let manager = ManagerFixtureState {
                calls: Arc::new(AtomicUsize::new(0)),
                fail: Arc::new(AtomicBool::new(false)),
                expires_at: Arc::new(StdRwLock::new(expires_at)),
                storage_path: storage_directory.path().display().to_string(),
                requests: Arc::new(StdMutex::new(Vec::new())),
            };
            let manager_url = spawn_manager_server(manager.clone()).await;

            let platform_requests = Arc::new(StdMutex::new(Vec::new()));
            let api_url = spawn_platform_server(PlatformFixtureState {
                manager_url,
                requests: platform_requests.clone(),
            })
            .await;

            Self {
                api_url,
                clock: Arc::new(ManualClock::new(now)),
                platform_requests,
                manager,
                _storage_directory: storage_directory,
            }
        }

        async fn remote_provider(&self) -> Arc<RemoteBindingsProvider> {
            Arc::new(
                RemoteBindingsProvider::discover(
                    DEPLOYMENT_ID,
                    TOKEN,
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
            .route("/v1/managers/{id}", get(manager_handler))
            .with_state(state);
        spawn_server(app).await
    }

    async fn spawn_manager_server(state: ManagerFixtureState) -> String {
        let app = Router::new()
            .route("/v1/bindings/resolve", post(resolve_handler))
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
    ) -> Json<serde_json::Value> {
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
            "managerId": MANAGER_ID,
            "workspaceId": WORKSPACE_ID
        }))
    }

    async fn manager_handler(
        State(state): State<PlatformFixtureState>,
        AxumPath(id): AxumPath<String>,
        headers: HeaderMap,
    ) -> Json<serde_json::Value> {
        state
            .requests
            .lock()
            .expect("platform requests lock")
            .push(RecordedRequest {
                method: "GET".to_string(),
                path: format!("/v1/managers/{id}"),
                authorization: authorization(&headers),
                body: None,
            });
        Json(json!({
            "id": MANAGER_ID,
            "name": "fixture-manager",
            "targets": ["local"],
            "managementConfigs": {},
            "isSystem": true,
            "workspaceId": WORKSPACE_ID,
            "status": "healthy",
            "url": state.manager_url,
            "managedDeploymentCount": 1,
            "defaultProjectCount": 0,
            "createdAt": "2026-01-01T00:00:00Z"
        }))
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
        if state.fail.load(Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }

        let expires_at = *state.expires_at.read().expect("manager expiry read lock");
        Json(json!({
            "binding": {
                "service": "local-storage",
                "storagePath": state.storage_path,
            },
            "clientConfig": {
                "platform": "local",
                "state_directory": state.storage_path,
            },
            "expiresAt": expires_at.to_rfc3339(),
        }))
        .into_response()
    }

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .expect("valid fixed timestamp")
            .with_timezone(&Utc)
            + ChronoDuration::seconds(second)
    }

    #[tokio::test]
    async fn discovers_assigned_manager_and_caches_each_requested_storage() {
        let fixture = Fixture::new(at(0), at(3600)).await;
        let bindings =
            Bindings::for_remote_deployment(DEPLOYMENT_ID, TOKEN, Some(&fixture.api_url))
                .await
                .expect("construct app-facing remote Bindings");
        let bindings_debug = format!("{bindings:?}");
        assert!(bindings_debug.contains("<redacted>"));
        assert!(!bindings_debug.contains(TOKEN));
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
                    authorization: Some(format!("Bearer {TOKEN}")),
                    body: Some(json!({
                        "deploymentId": DEPLOYMENT_ID,
                        "resourceId": "files",
                    })),
                },
                RecordedRequest {
                    method: "POST".to_string(),
                    path: "/v1/bindings/resolve".to_string(),
                    authorization: Some(format!("Bearer {TOKEN}")),
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
                    authorization: Some(format!("Bearer {TOKEN}")),
                    body: None,
                },
                RecordedRequest {
                    method: "GET".to_string(),
                    path: format!("/v1/managers/{MANAGER_ID}"),
                    authorization: Some(format!("Bearer {TOKEN}")),
                    body: None,
                },
            ]
        );

        let error = bindings
            .kv("not-storage")
            .await
            .expect_err("remote v0 must reject KV");
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    }

    #[tokio::test]
    async fn refreshes_once_for_concurrent_operations_without_reconstructing_handle() {
        let fixture = Fixture::new(at(0), at(600)).await;
        let provider = fixture.remote_provider().await;
        let bindings = Bindings::from_provider(provider);
        let storage = bindings
            .storage("files")
            .await
            .expect("initial remote Storage resolution");
        storage
            .put(&Path::from("shared.txt"), PutPayload::from_static(b"value"))
            .await
            .expect("seed fixture object");
        assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 1);

        fixture.clock.set(at(301));
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
    }

    #[tokio::test]
    async fn serves_unexpired_cache_on_refresh_failure_then_fails_closed_at_expiry() {
        let fixture = Fixture::new(at(0), at(600)).await;
        let provider = fixture.remote_provider().await;
        let bindings = Bindings::from_provider(provider);
        let storage = bindings
            .storage("files")
            .await
            .expect("initial remote Storage resolution");
        storage
            .put(&Path::from("lease.txt"), PutPayload::from_static(b"valid"))
            .await
            .expect("seed fixture object");

        fixture.manager.fail.store(true, Ordering::SeqCst);
        fixture.clock.set(at(301));
        let metadata = storage
            .head(&Path::from("lease.txt"))
            .await
            .expect("unexpired lease should survive failed refresh");
        assert_eq!(metadata.size, 5);
        assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);

        fixture.clock.set(at(600));
        let error = storage
            .head(&Path::from("lease.txt"))
            .await
            .expect_err("expired lease must fail closed when refresh fails");
        assert!(error.to_string().contains("Remote access failed"));
        assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 3);
    }
}
