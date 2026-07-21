//! Remote, resource-scoped binding resolution for app-facing clients.
//!
//! The Platform API discovers the deployment's assigned manager and mints a
//! short-lived, deployment-scoped manager capability. Binding topology and
//! short-lived cloud credentials come from that manager's resource resolver.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use crate::error::{ErrorData, Result};
use crate::provider::BindingsProvider;
use crate::refreshing::{RefreshingStorage, StorageProviderApi};
use crate::traits::{BindingsProviderApi, Storage};

mod access;
mod manager_conversion;

use access::{ManagerResolverKind, RemoteBindingSource};

#[cfg(test)]
use access::{
    authenticated_http_client, validate_manager_url, validate_platform_base_url, DiscoveredManager,
    GeneratedManagerBindingResolver, ManagerBindingResolver,
};

const INITIAL_REFRESH_RETRY_DELAY_SECONDS: i64 = 5;
const MAX_REFRESH_RETRY_DELAY_SECONDS: i64 = 30;
const MAX_REFRESH_SKEW_SECONDS: i64 = 300;

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
        Ok(Self {
            source: Arc::new(
                RemoteBindingSource::discover(
                    deployment_id,
                    token,
                    api_base_url,
                    resolver_kind,
                    clock.clone(),
                )
                .await?,
            ),
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
    let alien_core::AzureCredentials::SasToken { query_parameters } = &config.credentials else {
        return Err(invalid_remote_lease(
            "Azure",
            "an exact container SAS is required",
        ));
    };
    const REQUIRED_PARAMETERS: [&str; 13] = [
        "sp", "st", "se", "skoid", "sktid", "skt", "ske", "sks", "skv", "spr", "sv", "sr", "sig",
    ];
    if query_parameters.len() != REQUIRED_PARAMETERS.len()
        || REQUIRED_PARAMETERS.iter().any(|name| {
            !query_parameters
                .get(*name)
                .is_some_and(|value| !value.is_empty())
        })
        || query_parameters.get("sp").map(String::as_str) != Some("rcwdl")
        || query_parameters.get("spr").map(String::as_str) != Some("https")
        || query_parameters.get("sr").map(String::as_str) != Some("c")
        || query_parameters.get("sks").map(String::as_str) != Some("b")
    {
        return Err(invalid_remote_lease(
            "Azure",
            "the credential must contain only one exact container SAS",
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
    retryable_failure_count: u32,
    retry_not_before: Option<DateTime<Utc>>,
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

    fn cooldown_result(&self, now: DateTime<Utc>) -> Option<Result<Arc<BindingsProvider>>> {
        if !self.retry_not_before.is_some_and(|retry_at| now < retry_at) {
            return None;
        }
        let error = self.last_refresh_error.as_ref()?.clone();
        Some(self.unexpired(now).ok_or(error))
    }

    fn record_success(&mut self, cache: CachedRemoteBinding) -> Arc<BindingsProvider> {
        let provider = cache.provider.clone();
        self.cache = Some(cache);
        self.last_refresh_error = None;
        self.retryable_failure_count = 0;
        self.retry_not_before = None;
        provider
    }

    fn record_failure(&mut self, error: AlienError<ErrorData>, now: DateTime<Utc>) {
        if error.retryable {
            self.retryable_failure_count = self.retryable_failure_count.saturating_add(1);
            let retry_at = now + refresh_retry_delay(self.retryable_failure_count);
            self.retry_not_before = Some(match self.cache.as_ref() {
                Some(cache) if cache.expires_at > now => retry_at.min(cache.expires_at),
                _ => retry_at,
            });
        } else {
            self.retryable_failure_count = 0;
            self.retry_not_before = None;
        }
        self.last_refresh_error = Some(error);
    }
}

fn refresh_retry_delay(failure_count: u32) -> ChronoDuration {
    let multiplier = 2_i64.saturating_pow(failure_count.saturating_sub(1));
    let seconds = INITIAL_REFRESH_RETRY_DELAY_SECONDS
        .saturating_mul(multiplier)
        .min(MAX_REFRESH_RETRY_DELAY_SECONDS);
    ChronoDuration::seconds(seconds)
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
            if let Some(result) = state.cooldown_result(now) {
                return result;
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
            if let Some(result) = state.cooldown_result(now) {
                return result;
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

        match result {
            Ok(cache) => {
                let provider = state.record_success(cache);
                Ok(provider)
            }
            Err(error) if error.retryable => {
                state.record_failure(error.clone(), now);
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
            Err(error) => {
                state.record_failure(error.clone(), now);
                Err(error)
            }
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
