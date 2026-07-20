//! Remote, resource-scoped binding resolution for app-facing clients.
//!
//! The Platform API is used only to discover the deployment's assigned manager.
//! Binding topology and short-lived cloud credentials always come from that
//! manager's resource-scoped resolver.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alien_error::{AlienError, Context, ContextError, GenericError, IntoAlienError};
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
const MANAGER_DISCOVERY_TTL_SECONDS: i64 = 300;
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
pub(crate) struct RemoteBindingsProvider {
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
    pub(crate) async fn for_remote_deployment(
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
        let allow_insecure_manager_url = match api_base_url {
            Some(base_url) => validate_platform_base_url(base_url)?,
            None => false,
        };
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
            .map_err(into_remote_error)?
            .into_inner();
        let manager_url = discover_manager_url(
            &platform,
            deployment.manager_id.to_string(),
            allow_insecure_manager_url,
        )
        .await?;

        Ok(Self {
            source: Arc::new(RemoteBindingSource {
                deployment_id: deployment_id.to_string(),
                platform,
                manager: RwLock::new(DiscoveredManager {
                    url: manager_url,
                    discovered_at: clock.now(),
                }),
                manager_refresh_lock: Mutex::new(()),
                allow_insecure_manager_url,
                http,
                clock: clock.clone(),
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
                    refresh_generation: AtomicU64::new(0),
                    last_refresh_error: RwLock::new(None),
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
    platform: alien_platform_api::Client,
    manager: RwLock<DiscoveredManager>,
    manager_refresh_lock: Mutex<()>,
    allow_insecure_manager_url: bool,
    http: reqwest::Client,
    clock: Arc<dyn Clock>,
}

struct DiscoveredManager {
    url: reqwest::Url,
    discovered_at: DateTime<Utc>,
}

impl fmt::Debug for RemoteBindingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteBindingSource")
            .field("deployment_id", &self.deployment_id)
            .field("manager_url", &"<redacted>")
            .field("credentials", &"<redacted>")
            .finish()
    }
}

impl RemoteBindingSource {
    async fn manager_url(&self) -> Result<reqwest::Url> {
        let now = self.clock.now();
        {
            let manager = self.manager.read().await;
            if now < manager.discovered_at + ChronoDuration::seconds(MANAGER_DISCOVERY_TTL_SECONDS)
            {
                return Ok(manager.url.clone());
            }
        }

        let _refresh = self.manager_refresh_lock.lock().await;
        let now = self.clock.now();
        {
            let manager = self.manager.read().await;
            if now < manager.discovered_at + ChronoDuration::seconds(MANAGER_DISCOVERY_TTL_SECONDS)
            {
                return Ok(manager.url.clone());
            }
        }

        let deployment = self
            .platform
            .get_deployment()
            .id(&self.deployment_id)
            .send()
            .await
            .into_sdk_error()
            .map_err(into_remote_error)?
            .into_inner();
        let manager_url = discover_manager_url(
            &self.platform,
            deployment.manager_id.to_string(),
            self.allow_insecure_manager_url,
        )
        .await?;
        *self.manager.write().await = DiscoveredManager {
            url: manager_url.clone(),
            discovered_at: self.clock.now(),
        };
        Ok(manager_url)
    }

    async fn resolve(&self, resource_id: &str) -> Result<ResolvedRemoteBinding> {
        let url = self
            .manager_url()
            .await?
            .join("v1/bindings/resolve")
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "build remote binding URL".to_string(),
            })?;
        let response = self
            .http
            .post(url)
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
        if !response.status().is_success() {
            return Err(remote_http_error(response, resource_id).await);
        }

        response
            .json::<ResolvedRemoteBinding>()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("parse remote Storage binding '{resource_id}'"),
            })
    }
}

async fn discover_manager_url(
    platform: &alien_platform_api::Client,
    manager_id: String,
    allow_insecure: bool,
) -> Result<reqwest::Url> {
    let manager = platform
        .get_manager()
        .id(&manager_id)
        .send()
        .await
        .into_sdk_error()
        .map_err(into_remote_error)?
        .into_inner();
    let manager_url = manager
        .url
        .ok_or_else(|| remote_configuration_error("assigned manager has no reachable URL"))?;
    validate_manager_url(&manager_url, allow_insecure)
}

fn validate_manager_url(raw: &str, allow_insecure: bool) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw)
        .into_alien_error()
        .map_err(|error| remote_configuration_source_error(error, "parse assigned manager URL"))?;
    let valid_scheme =
        url.scheme() == "https" || (allow_insecure && url.scheme() == "http" && is_loopback(&url));
    if !valid_scheme
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(remote_configuration_error("validate assigned manager URL"));
    }
    Ok(url)
}

/// Returns whether a caller-supplied Platform base URL may discover a local
/// HTTP manager. Production discovery is HTTPS-only; loopback HTTP exists for
/// local development and deterministic tests.
fn validate_platform_base_url(raw: &str) -> Result<bool> {
    let url = reqwest::Url::parse(raw)
        .into_alien_error()
        .map_err(|error| remote_configuration_source_error(error, "parse Platform API base URL"))?;
    let allow_insecure = url.scheme() == "http" && is_loopback(&url);
    let valid_scheme = url.scheme() == "https" || allow_insecure;
    if !valid_scheme
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(remote_configuration_error("validate Platform API base URL"));
    }
    Ok(allow_insecure)
}

fn remote_configuration_error(operation: &str) -> AlienError<ErrorData> {
    let mut error = AlienError::new(ErrorData::RemoteAccessFailed {
        operation: operation.to_string(),
    });
    error.retryable = false;
    error.http_status_code = Some(400);
    error
}

fn remote_configuration_source_error(
    source: AlienError<GenericError>,
    operation: &str,
) -> AlienError<ErrorData> {
    let mut error = source.context(ErrorData::RemoteAccessFailed {
        operation: operation.to_string(),
    });
    error.retryable = false;
    error.http_status_code = Some(400);
    error
}

fn is_loopback(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

async fn remote_http_error(
    response: reqwest::Response,
    resource_id: &str,
) -> AlienError<ErrorData> {
    let status = response.status();
    match response.json::<AlienError<GenericError>>().await {
        Ok(error) => into_remote_error(error),
        Err(_) => {
            let mut error = AlienError::new(ErrorData::RemoteAccessFailed {
                operation: format!(
                    "resolve remote Storage binding '{resource_id}' (HTTP {status})"
                ),
            });
            error.retryable = status.is_server_error()
                || status == reqwest::StatusCode::REQUEST_TIMEOUT
                || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
            error.http_status_code = Some(status.as_u16());
            error
        }
    }
}

fn into_remote_error(error: AlienError<GenericError>) -> AlienError<ErrorData> {
    AlienError {
        code: error.code,
        message: error.message,
        context: error.context,
        hint: error.hint,
        retryable: error.retryable,
        internal: error.internal,
        http_status_code: error.http_status_code,
        source: error.source,
        human_layer_presentation: error.human_layer_presentation,
        error: None,
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
    refresh_generation: AtomicU64,
    last_refresh_error: RwLock<Option<AlienError<ErrorData>>>,
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
        let observed_generation = self.refresh_generation.load(Ordering::Acquire);
        let now = self.clock.now();
        if let Some(provider) = self.fresh_cached(now).await {
            return Ok(provider);
        }

        let _flight = self.refresh_lock.lock().await;
        let now = self.clock.now();
        if let Some(provider) = self.fresh_cached(now).await {
            return Ok(provider);
        }

        if self.refresh_generation.load(Ordering::Acquire) != observed_generation {
            if let Some(provider) = self.unexpired_cached(now).await {
                return Ok(provider);
            }
            if let Some(error) = self.last_refresh_error.read().await.clone() {
                return Err(error);
            }
        }

        let result = self.source.resolve(&self.resource_id).await;
        let now = self.clock.now();
        let result = match result {
            Ok(resolved) => self.cache_resolved(resolved, now).await,
            Err(error) => Err(error),
        };
        *self.last_refresh_error.write().await = result.as_ref().err().cloned();
        self.refresh_generation.fetch_add(1, Ordering::Release);

        match result {
            Ok(provider) => Ok(provider),
            Err(error) if error.retryable => {
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
            Err(error) => Err(error),
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
        // Validate the typed binding and provider feature before committing the
        // lease. An invalid response must not poison the cache until expiry.
        provider.load_storage(&self.resource_id).await?;
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
        manager_url: Arc<StdRwLock<String>>,
        requests: Arc<StdMutex<Vec<RecordedRequest>>>,
    }

    #[derive(Clone)]
    struct ManagerFixtureState {
        calls: Arc<AtomicUsize>,
        fail: Arc<AtomicBool>,
        failure_response: Arc<StdRwLock<Option<(StatusCode, serde_json::Value)>>>,
        invalid_binding: Arc<AtomicBool>,
        advance_clock_to: Arc<StdRwLock<Option<DateTime<Utc>>>>,
        clock: Arc<ManualClock>,
        expires_at: Arc<StdRwLock<DateTime<Utc>>>,
        storage_path: String,
        requests: Arc<StdMutex<Vec<RecordedRequest>>>,
    }

    struct Fixture {
        api_url: String,
        clock: Arc<ManualClock>,
        platform_requests: Arc<StdMutex<Vec<RecordedRequest>>>,
        manager_url: Arc<StdRwLock<String>>,
        manager: ManagerFixtureState,
        _storage_directory: TempDir,
    }

    impl Fixture {
        async fn new(now: DateTime<Utc>, expires_at: DateTime<Utc>) -> Self {
            let storage_directory = TempDir::new().expect("create fixture storage directory");
            let clock = Arc::new(ManualClock::new(now));
            let manager = ManagerFixtureState {
                calls: Arc::new(AtomicUsize::new(0)),
                fail: Arc::new(AtomicBool::new(false)),
                failure_response: Arc::new(StdRwLock::new(None)),
                invalid_binding: Arc::new(AtomicBool::new(false)),
                advance_clock_to: Arc::new(StdRwLock::new(None)),
                clock: clock.clone(),
                expires_at: Arc::new(StdRwLock::new(expires_at)),
                storage_path: storage_directory.path().display().to_string(),
                requests: Arc::new(StdMutex::new(Vec::new())),
            };
            let manager_url = Arc::new(StdRwLock::new(spawn_manager_server(manager.clone()).await));

            let platform_requests = Arc::new(StdMutex::new(Vec::new()));
            let api_url = spawn_platform_server(PlatformFixtureState {
                manager_url: manager_url.clone(),
                requests: platform_requests.clone(),
            })
            .await;

            Self {
                api_url,
                clock,
                platform_requests,
                manager_url,
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

        fn fail_manager_with(&self, status: StatusCode, body: serde_json::Value) {
            *self
                .manager
                .failure_response
                .write()
                .expect("manager failure response write lock") = Some((status, body));
        }

        fn advance_clock_during_next_resolve(&self, now: DateTime<Utc>) {
            *self
                .manager
                .advance_clock_to
                .write()
                .expect("manager clock advance write lock") = Some(now);
        }

        fn assign_manager_url(&self, manager_url: String) {
            *self.manager_url.write().expect("manager URL write lock") = manager_url;
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
        let manager_url = state
            .manager_url
            .read()
            .expect("manager URL read lock")
            .clone();
        Json(json!({
            "id": MANAGER_ID,
            "name": "fixture-manager",
            "targets": ["local"],
            "managementConfigs": {},
            "isSystem": true,
            "workspaceId": WORKSPACE_ID,
            "status": "healthy",
            "url": manager_url,
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
            return (status, Json(body)).into_response();
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
        Json(json!({
            "binding": binding,
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
        let bindings = Bindings::from_provider(provider.clone());
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
    async fn existing_storage_handle_follows_manager_reassignment() {
        let fixture = Fixture::new(at(0), at(600)).await;
        let provider = fixture.remote_provider().await;
        let bindings = Bindings::from_provider(provider);
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

        let manager_b = ManagerFixtureState {
            calls: Arc::new(AtomicUsize::new(0)),
            fail: Arc::new(AtomicBool::new(false)),
            failure_response: Arc::new(StdRwLock::new(None)),
            invalid_binding: Arc::new(AtomicBool::new(false)),
            advance_clock_to: Arc::new(StdRwLock::new(None)),
            clock: fixture.clock.clone(),
            expires_at: Arc::new(StdRwLock::new(at(3901))),
            storage_path: fixture.manager.storage_path.clone(),
            requests: Arc::new(StdMutex::new(Vec::new())),
        };
        let manager_b_url = spawn_manager_server(manager_b.clone()).await;
        fixture.manager.fail.store(true, Ordering::SeqCst);
        fixture.assign_manager_url(manager_b_url);
        fixture.clock.set(at(301));

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
    }

    #[tokio::test]
    async fn concurrent_failed_refresh_is_single_flight_while_cache_is_unexpired() {
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

        fixture.manager.fail.store(true, Ordering::SeqCst);
        fixture.clock.set(at(301));
        let operations = (0..16).map(|_| {
            let storage = storage.clone();
            async move { storage.head(&Path::from("shared.txt")).await }
        });
        let results = join_all(operations).await;

        assert!(results.iter().all(|result| result.is_ok()));
        assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn non_retryable_manager_error_is_preserved_and_never_uses_cached_credentials() {
        let fixture = Fixture::new(at(0), at(600)).await;
        let provider = fixture.remote_provider().await;
        let bindings = Bindings::from_provider(provider.clone());
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
        fixture.clock.set(at(301));
        let error = provider
            .load_storage("files")
            .await
            .expect_err("revoked access must not fall back to a cached lease");

        assert_eq!(error.code, "FORBIDDEN");
        assert!(!error.retryable);
        assert_eq!(error.http_status_code, Some(403));
        assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn refresh_rechecks_expiry_after_the_network_request() {
        let fixture = Fixture::new(at(0), at(600)).await;
        let provider = fixture.remote_provider().await;
        let bindings = Bindings::from_provider(provider.clone());
        let storage = bindings
            .storage("files")
            .await
            .expect("initial remote Storage resolution");
        storage
            .put(&Path::from("lease.txt"), PutPayload::from_static(b"value"))
            .await
            .expect("seed fixture object");

        fixture.manager.fail.store(true, Ordering::SeqCst);
        fixture.clock.set(at(301));
        fixture.advance_clock_during_next_resolve(at(600));
        let error = provider
            .load_storage("files")
            .await
            .expect_err("a lease that expired during refresh must not be used");

        assert_eq!(error.code, "REMOTE_ACCESS_FAILED");
        assert_eq!(fixture.manager.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn malformed_manager_response_does_not_poison_the_cache() {
        let fixture = Fixture::new(at(0), at(600)).await;
        fixture
            .manager
            .invalid_binding
            .store(true, Ordering::SeqCst);
        let provider = fixture.remote_provider().await;
        let bindings = Bindings::from_provider(provider);

        bindings
            .storage("files")
            .await
            .expect_err("invalid binding must fail before caching");
        fixture
            .manager
            .invalid_binding
            .store(false, Ordering::SeqCst);
        bindings
            .storage("files")
            .await
            .expect("a subsequent valid response must be retried and cached");

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
