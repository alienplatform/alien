//! Remote, resource-scoped binding resolution for app-facing clients.
//!
//! The Platform API is used only to discover the deployment's assigned manager.
//! Binding topology and short-lived cloud credentials always come from that
//! manager's resource-scoped resolver.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use alien_error::{AlienError, Context, ContextError, GenericError, IntoAlienError};
use alien_manager_api::SdkResultExtReadingBody;
use alien_platform_api::SdkResultExt;
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use crate::error::{ErrorData, Result};
use crate::provider::BindingsProvider;
use crate::refreshing::{RefreshingStorage, StorageProviderApi};
use crate::traits::{BindingsProviderApi, Storage};

const DEFAULT_PLATFORM_API_URL: &str = "https://api.alien.dev";
const MAX_REFRESH_SKEW_SECONDS: i64 = 300;
const MANAGER_DISCOVERY_TTL_SECONDS: i64 = 300;
const REMOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const AZURE_STORAGE_SCOPE: &str = "https://storage.azure.com/.default";

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
        Self::discover_with_manager_resolver(
            deployment_id,
            token,
            api_base_url,
            clock,
            ManagerResolverKind::Generated,
        )
        .await
    }

    #[cfg(test)]
    async fn discover_local_fixture(
        deployment_id: &str,
        token: &str,
        api_base_url: Option<&str>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        Self::discover_with_manager_resolver(
            deployment_id,
            token,
            api_base_url,
            clock,
            ManagerResolverKind::LocalFixture,
        )
        .await
    }

    async fn discover_with_manager_resolver(
        deployment_id: &str,
        token: &str,
        api_base_url: Option<&str>,
        clock: Arc<dyn Clock>,
        resolver_kind: ManagerResolverKind,
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
        let manager_resolver: Arc<dyn ManagerBindingResolver> = match resolver_kind {
            ManagerResolverKind::Generated => {
                Arc::new(GeneratedManagerBindingResolver { http: http.clone() })
            }
            #[cfg(test)]
            ManagerResolverKind::LocalFixture => {
                Arc::new(LocalFixtureManagerBindingResolver { http: http.clone() })
            }
        };

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
                manager_resolver,
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
                    state: RwLock::new(RemoteStorageState::default()),
                    refresh_lock: Mutex::new(()),
                    clock: self.clock.clone(),
                })
            })
            .clone()
    }
}

#[async_trait]
impl StorageProviderApi for RemoteBindingsProvider {
    async fn load_storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>> {
        self.resolver(binding_name).await.storage().await
    }
}

/// Resource-scoped remote Storage bindings for an existing deployment.
///
/// This type intentionally exposes Storage only. Other binding kinds are not
/// part of the remote v0 contract and therefore cannot be requested.
#[derive(Debug)]
pub struct RemoteBindings {
    provider: Arc<RemoteBindingsProvider>,
}

/// The complete remote Storage v0 operation surface.
///
/// This intentionally does not extend [`Storage`] or `object_store::ObjectStore`:
/// copy, rename, multipart, range, and presigned-URL operations are not
/// authorized by the remote v0 contract and cannot be requested through this
/// trait.
#[async_trait]
pub trait RemoteStorage: Send + Sync + fmt::Debug {
    async fn get(
        &self,
        path: &object_store::path::Path,
    ) -> object_store::Result<object_store::GetResult>;
    async fn put(
        &self,
        path: &object_store::path::Path,
        payload: object_store::PutPayload,
    ) -> object_store::Result<object_store::PutResult>;
    async fn head(
        &self,
        path: &object_store::path::Path,
    ) -> object_store::Result<object_store::ObjectMeta>;
    async fn delete(&self, path: &object_store::path::Path) -> object_store::Result<()>;
    fn list(
        &self,
        prefix: Option<&object_store::path::Path>,
    ) -> futures::stream::BoxStream<'static, object_store::Result<object_store::ObjectMeta>>;
}

impl RemoteBindings {
    /// Discovers the deployment's assigned manager through the Platform API.
    pub async fn for_deployment(
        deployment_id: &str,
        token: &str,
        api_base_url: Option<&str>,
    ) -> Result<Self> {
        Ok(Self {
            provider: Arc::new(
                RemoteBindingsProvider::for_remote_deployment(deployment_id, token, api_base_url)
                    .await?,
            ),
        })
    }

    #[cfg(test)]
    fn from_provider(provider: Arc<RemoteBindingsProvider>) -> Self {
        Self { provider }
    }

    /// Loads a Storage binding and keeps its short-lived credential lease fresh.
    pub async fn storage(&self, resource_id: &str) -> Result<Arc<dyn RemoteStorage>> {
        let initial = self.provider.load_storage(resource_id).await?;
        Ok(Arc::new(RefreshingStorage::new(
            self.provider.clone(),
            resource_id.to_string(),
            initial,
        )))
    }
}

struct RemoteBindingSource {
    deployment_id: String,
    platform: alien_platform_api::Client,
    manager: RwLock<DiscoveredManager>,
    manager_refresh_lock: Mutex<()>,
    allow_insecure_manager_url: bool,
    manager_resolver: Arc<dyn ManagerBindingResolver>,
    clock: Arc<dyn Clock>,
}

enum ManagerResolverKind {
    Generated,
    #[cfg(test)]
    LocalFixture,
}

#[async_trait]
trait ManagerBindingResolver: Send + Sync + fmt::Debug {
    async fn resolve(
        &self,
        manager_url: &reqwest::Url,
        deployment_id: &str,
        resource_id: &str,
    ) -> Result<ResolvedRemoteBinding>;
}

#[derive(Debug)]
struct GeneratedManagerBindingResolver {
    http: reqwest::Client,
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
        let manager_url = self.manager_url().await?;
        self.manager_resolver
            .resolve(&manager_url, &self.deployment_id, resource_id)
            .await
    }
}

#[async_trait]
impl ManagerBindingResolver for GeneratedManagerBindingResolver {
    async fn resolve(
        &self,
        manager_url: &reqwest::Url,
        deployment_id: &str,
        resource_id: &str,
    ) -> Result<ResolvedRemoteBinding> {
        let manager = alien_manager_api::Client::new_with_client(
            manager_url.as_str().trim_end_matches('/'),
            self.http.clone(),
        );
        let response = manager
            .resolve_binding()
            .body(alien_manager_api::types::ResolveBindingRequest {
                deployment_id: deployment_id.to_string(),
                resource_id: resource_id.to_string(),
            })
            .send()
            .await
            .into_sdk_error_reading_body()
            .await
            .map_err(into_remote_error)?
            .into_inner();

        serde_json::to_value(response)
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("convert remote Storage binding '{resource_id}'"),
            })
            .and_then(|response| {
                serde_json::from_value(response).into_alien_error().context(
                    ErrorData::RemoteAccessFailed {
                        operation: format!("parse remote Storage binding '{resource_id}'"),
                    },
                )
            })
    }
}

/// Test-only adapter for typed local leases. Local is deliberately absent from
/// the hosted API contract; cache tests inject this adapter explicitly instead
/// of changing the production generated-client path.
#[cfg(test)]
#[derive(Debug)]
struct LocalFixtureManagerBindingResolver {
    http: reqwest::Client,
}

#[cfg(test)]
#[async_trait]
impl ManagerBindingResolver for LocalFixtureManagerBindingResolver {
    async fn resolve(
        &self,
        manager_url: &reqwest::Url,
        deployment_id: &str,
        resource_id: &str,
    ) -> Result<ResolvedRemoteBinding> {
        let url = manager_url
            .join("v1/bindings/resolve")
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "build remote binding fixture URL".to_string(),
            })?;
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "deploymentId": deployment_id,
                "resourceId": resource_id,
            }))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("resolve remote Storage binding '{resource_id}'"),
            })?;
        if !response.status().is_success() {
            return Err(test_fixture_http_error(response, resource_id).await);
        }
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: format!("parse remote Storage binding '{resource_id}'"),
            })
    }
}

#[cfg(test)]
async fn test_fixture_http_error(
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
            error.retryable = status.is_server_error();
            error.http_status_code = Some(status.as_u16());
            error
        }
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

#[derive(Deserialize)]
#[serde(tag = "service", rename_all = "lowercase")]
enum ResolvedRemoteBinding {
    S3 {
        binding: alien_core::S3StorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::AwsClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    Blob {
        binding: alien_core::BlobStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::AzureClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    Gcs {
        binding: alien_core::GcsStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::GcpClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    #[cfg(test)]
    #[serde(rename = "local-storage")]
    Local {
        binding: alien_core::LocalStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: TestLocalClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
}

#[cfg(test)]
#[derive(Deserialize)]
struct TestLocalClientConfig {
    state_directory: String,
}

impl ResolvedRemoteBinding {
    fn into_provider_parts(
        self,
    ) -> Result<(alien_core::ClientConfig, serde_json::Value, DateTime<Utc>)> {
        let (client_config, binding, expires_at) = match self {
            Self::S3 {
                binding,
                client_config,
                expires_at,
            } => {
                validate_aws_remote_client_config(&client_config, expires_at)?;
                (
                    alien_core::ClientConfig::Aws(client_config),
                    alien_core::StorageBinding::S3(binding),
                    expires_at,
                )
            }
            Self::Blob {
                binding,
                client_config,
                expires_at,
            } => {
                validate_azure_remote_client_config(&client_config)?;
                (
                    alien_core::ClientConfig::Azure(client_config),
                    alien_core::StorageBinding::Blob(binding),
                    expires_at,
                )
            }
            Self::Gcs {
                binding,
                client_config,
                expires_at,
            } => {
                validate_gcp_remote_client_config(&client_config)?;
                (
                    alien_core::ClientConfig::Gcp(client_config),
                    alien_core::StorageBinding::Gcs(binding),
                    expires_at,
                )
            }
            #[cfg(test)]
            Self::Local {
                binding,
                client_config,
                expires_at,
            } => (
                alien_core::ClientConfig::Local {
                    state_directory: client_config.state_directory,
                },
                alien_core::StorageBinding::Local(binding),
                expires_at,
            ),
        };
        let binding = serde_json::to_value(binding).into_alien_error().context(
            ErrorData::RemoteAccessFailed {
                operation: "convert typed remote Storage lease".to_string(),
            },
        )?;
        Ok((client_config, binding, expires_at))
    }
}

fn invalid_remote_lease(provider: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::RemoteAccessFailed {
        operation: format!("validate {provider} remote Storage credential lease: {reason}"),
    })
}

fn validate_aws_remote_client_config(
    config: &alien_core::AwsClientConfig,
    lease_expires_at: DateTime<Utc>,
) -> Result<()> {
    if config.service_overrides.is_some() {
        return Err(invalid_remote_lease(
            "AWS",
            "service endpoint overrides are forbidden",
        ));
    }
    let alien_core::AwsCredentials::SessionCredentials { expires_at, .. } = &config.credentials
    else {
        return Err(invalid_remote_lease(
            "AWS",
            "short-lived session credentials are required",
        ));
    };
    let credential_expires_at = DateTime::parse_from_rfc3339(expires_at)
        .map_err(|_| invalid_remote_lease("AWS", "credential expiry is invalid"))?
        .with_timezone(&Utc);
    if credential_expires_at < lease_expires_at {
        return Err(invalid_remote_lease(
            "AWS",
            "credential expires before its lease",
        ));
    }
    Ok(())
}

fn validate_gcp_remote_client_config(config: &alien_core::GcpClientConfig) -> Result<()> {
    if config.service_overrides.is_some()
        || !matches!(
            config.credentials,
            alien_core::GcpCredentials::AccessToken { .. }
        )
    {
        return Err(invalid_remote_lease(
            "GCP",
            "one access token without service endpoint overrides is required",
        ));
    }
    Ok(())
}

fn validate_azure_remote_client_config(config: &alien_core::AzureClientConfig) -> Result<()> {
    if config.service_overrides.is_some() {
        return Err(invalid_remote_lease(
            "Azure",
            "service endpoint overrides are forbidden",
        ));
    }
    let alien_core::AzureCredentials::ScopedAccessTokens { tokens } = &config.credentials else {
        return Err(invalid_remote_lease(
            "Azure",
            "an exact storage-scoped access token is required",
        ));
    };
    if tokens.len() != 1 || !tokens.contains_key(AZURE_STORAGE_SCOPE) {
        return Err(invalid_remote_lease(
            "Azure",
            "the credential must contain only the Azure Storage audience",
        ));
    }
    Ok(())
}

struct CachedRemoteBinding {
    provider: Arc<BindingsProvider>,
    refresh_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Default)]
struct RemoteStorageState {
    cache: Option<CachedRemoteBinding>,
    generation: u64,
    last_refresh_error: Option<AlienError<ErrorData>>,
}

impl RemoteStorageState {
    fn fresh(&self, now: DateTime<Utc>) -> Option<Arc<BindingsProvider>> {
        self.cache
            .as_ref()
            .and_then(|cached| (now < cached.refresh_at).then(|| cached.provider.clone()))
    }

    fn unexpired(&self, now: DateTime<Utc>) -> Option<Arc<BindingsProvider>> {
        self.cache
            .as_ref()
            .and_then(|cached| (now < cached.expires_at).then(|| cached.provider.clone()))
    }
}

struct RemoteStorageResolver {
    source: Arc<RemoteBindingSource>,
    resource_id: String,
    state: RwLock<RemoteStorageState>,
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
        BindingsProviderApi::load_storage(&*self.provider().await?, &self.resource_id).await
    }

    async fn provider(&self) -> Result<Arc<BindingsProvider>> {
        let now = self.clock.now();
        let observed_generation = {
            let state = self.state.read().await;
            if let Some(provider) = state.fresh(now) {
                return Ok(provider);
            }
            state.generation
        };

        let _flight = self.refresh_lock.lock().await;
        let now = self.clock.now();
        {
            let state = self.state.read().await;
            if let Some(provider) = state.fresh(now) {
                return Ok(provider);
            }
            if state.generation != observed_generation {
                if let Some(error) = state.last_refresh_error.clone() {
                    if error.retryable {
                        if let Some(provider) = state.unexpired(now) {
                            return Ok(provider);
                        }
                    }
                    return Err(error);
                }
                if let Some(provider) = state.unexpired(now) {
                    return Ok(provider);
                }
            }
        }

        let result = self.source.resolve(&self.resource_id).await;
        let now = self.clock.now();
        let result = match result {
            Ok(resolved) => self.build_cache_entry(resolved, now).await,
            Err(error) => Err(error),
        };
        let mut state = self.state.write().await;
        state.generation = state.generation.wrapping_add(1);
        state.last_refresh_error = result.as_ref().err().cloned();

        match result {
            Ok(cache) => {
                let provider = cache.provider.clone();
                state.cache = Some(cache);
                Ok(provider)
            }
            Err(error) if error.retryable => {
                if let Some(provider) = state.unexpired(now) {
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

    async fn build_cache_entry(
        &self,
        resolved: ResolvedRemoteBinding,
        now: DateTime<Utc>,
    ) -> Result<CachedRemoteBinding> {
        let (client_config, binding, expires_at) = resolved.into_provider_parts()?;
        if expires_at <= now {
            return Err(AlienError::new(ErrorData::RemoteAccessFailed {
                operation: format!(
                    "manager returned an expired lease for Storage binding '{}'",
                    self.resource_id
                ),
            }));
        }

        let provider = Arc::new(BindingsProvider::new(
            client_config,
            HashMap::from([(self.resource_id.clone(), binding)]),
        )?);
        // Validate the typed binding and provider feature before committing the
        // lease. An invalid response must not poison the cache until expiry.
        BindingsProviderApi::load_storage(&*provider, &self.resource_id).await?;
        let lifetime = expires_at - now;
        let refresh_skew = std::cmp::min(
            ChronoDuration::seconds(MAX_REFRESH_SKEW_SECONDS),
            lifetime / 5,
        );
        Ok(CachedRemoteBinding {
            provider,
            refresh_at: expires_at - refresh_skew,
            expires_at,
        })
    }
}

#[cfg(test)]
#[path = "remote/tests.rs"]
mod tests;
