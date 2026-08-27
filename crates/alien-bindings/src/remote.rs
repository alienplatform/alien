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
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use crate::error::{ErrorData, Result};
use crate::provider::BindingsProvider;
use crate::refreshing::{KeyProviderApi, RefreshingKey, RefreshingStorage, StorageProviderApi};
use crate::traits::{BindingsProviderApi, Key, Sandbox, Storage};

mod access;
mod manager_conversion;

use access::{ManagerResolverKind, RemoteBindingCapability, RemoteBindingSource};

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

/// App-facing provider for resource-scoped remote bindings.
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

    /// Selects a external environment's deployment for one capability by Project and external
    /// ID and creates a lazy remote provider.
    pub(crate) async fn for_remote_environment(
        project: &str,
        external_id: &str,
        capability: RemoteBindingCapability,
        token: &str,
        api_base_url: Option<&str>,
    ) -> Result<Self> {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        Ok(Self {
            source: Arc::new(
                RemoteBindingSource::discover_external_environment(
                    project,
                    external_id,
                    capability,
                    token,
                    api_base_url,
                    ManagerResolverKind::Generated,
                    clock.clone(),
                )
                .await?,
            ),
            resolvers: RwLock::new(HashMap::new()),
            clock,
        })
    }

    fn from_manager_access(
        deployment_id: &str,
        manager_url: &str,
        manager_token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<Self> {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        Ok(Self {
            source: Arc::new(RemoteBindingSource::from_manager_access(
                deployment_id,
                manager_url,
                manager_token,
                expires_at,
                clock.clone(),
            )?),
            resolvers: RwLock::new(HashMap::new()),
            clock,
        })
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

#[async_trait]
impl KeyProviderApi for RemoteBindingsProvider {
    async fn load_key(&self, binding_name: &str) -> Result<Arc<dyn Key>> {
        self.resolver(binding_name).await.key().await
    }
}

/// Resource-scoped remote bindings for an existing deployment.
#[derive(Debug)]
pub struct RemoteBindings {
    source: BindingsSource,
}

#[derive(Debug)]
enum BindingsSource {
    /// The deployment is already identified, so every binding rides one provider.
    Deployment(Arc<RemoteBindingsProvider>),
    /// Only the customer is identified. Platform picks the deployment by matching its purpose
    /// against the requested capability, so each capability addresses a different deployment and
    /// needs its own manager access. Resolving at construction would pin the handle to one
    /// capability and make every other binding unreachable.
    Environment(EnvironmentSource),
}

struct EnvironmentSource {
    project: String,
    external_id: String,
    token: String,
    api_base_url: Option<String>,
    providers: RwLock<HashMap<RemoteBindingCapability, Arc<RemoteBindingsProvider>>>,
}

impl fmt::Debug for EnvironmentSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentSource")
            .field("project", &self.project)
            .field("external_id", &self.external_id)
            .field("token", &"<redacted>")
            .field("api_base_url", &self.api_base_url)
            .finish()
    }
}

impl EnvironmentSource {
    async fn provider(
        &self,
        capability: RemoteBindingCapability,
    ) -> Result<Arc<RemoteBindingsProvider>> {
        if let Some(provider) = self.providers.read().await.get(&capability) {
            return Ok(provider.clone());
        }
        let provider = Arc::new(
            RemoteBindingsProvider::for_remote_environment(
                &self.project,
                &self.external_id,
                capability,
                &self.token,
                self.api_base_url.as_deref(),
            )
            .await?,
        );
        // Racing callers may both discover; keeping the first keeps one lease per capability.
        Ok(self
            .providers
            .write()
            .await
            .entry(capability)
            .or_insert(provider)
            .clone())
    }
}

impl BindingsSource {
    async fn provider(
        &self,
        capability: RemoteBindingCapability,
    ) -> Result<Arc<RemoteBindingsProvider>> {
        match self {
            Self::Deployment(provider) => Ok(provider.clone()),
            Self::Environment(environment) => environment.provider(capability).await,
        }
    }
}

/// A short-lived managed AI binding and the cloud credential needed to use it.
///
/// Callers must discard this lease at `expires_at`. Credentials are deliberately
/// omitted from `Debug` output.
pub struct RemoteAiLease {
    pub resource_id: String,
    pub binding: alien_core::AiBinding,
    pub client_config: alien_core::ClientConfig,
    pub expires_at: DateTime<Utc>,
}

impl fmt::Debug for RemoteAiLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteAiLease")
            .field("resource_id", &self.resource_id)
            .field("binding", &self.binding)
            .field("client_config", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
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
    async fn get_opts(
        &self,
        path: &object_store::path::Path,
        options: object_store::GetOptions,
    ) -> object_store::Result<object_store::GetResult>;
    async fn put(
        &self,
        path: &object_store::path::Path,
        payload: object_store::PutPayload,
    ) -> object_store::Result<object_store::PutResult>;
    async fn put_opts(
        &self,
        path: &object_store::path::Path,
        payload: object_store::PutPayload,
        options: object_store::PutOptions,
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
    /// Addresses an external environment by stable application external ID.
    ///
    /// Each binding discovers its own deployment and Manager on first use, because Platform
    /// selects the deployment by matching its purpose to the requested capability: a customer
    /// running only a Sandbox has no Storage deployment, and vice versa. Nothing is contacted
    /// here, so an unreachable customer surfaces at the first `storage`/`sandbox` call.
    pub async fn for_environment(
        project: &str,
        external_id: &str,
        token: &str,
        api_base_url: Option<&str>,
    ) -> Result<Self> {
        Ok(Self {
            source: BindingsSource::Environment(EnvironmentSource {
                project: project.to_string(),
                external_id: external_id.to_string(),
                token: token.to_string(),
                api_base_url: api_base_url.map(ToString::to_string),
                providers: RwLock::new(HashMap::new()),
            }),
        })
    }

    /// Discovers the deployment's assigned manager through the Platform API.
    pub async fn for_deployment(
        deployment_id: &str,
        token: &str,
        api_base_url: Option<&str>,
    ) -> Result<Self> {
        Ok(Self {
            source: BindingsSource::Deployment(Arc::new(
                RemoteBindingsProvider::for_remote_deployment(deployment_id, token, api_base_url)
                    .await?,
            )),
        })
    }

    /// Uses an already-issued, deployment-scoped Manager capability.
    ///
    /// This path does not contact Platform and cannot refresh the capability.
    /// Construct a new instance after `expires_at`; callers should load the
    /// required binding immediately and discard it with the returned cloud
    /// credentials.
    pub fn from_manager_access(
        deployment_id: &str,
        manager_url: &str,
        manager_token: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<Self> {
        Ok(Self {
            source: BindingsSource::Deployment(Arc::new(
                RemoteBindingsProvider::from_manager_access(
                    deployment_id,
                    manager_url,
                    manager_token,
                    expires_at,
                )?,
            )),
        })
    }

    #[cfg(test)]
    fn from_provider(provider: Arc<RemoteBindingsProvider>) -> Self {
        Self {
            source: BindingsSource::Deployment(provider),
        }
    }

    /// Loads a Storage binding and keeps its short-lived credential lease fresh.
    pub async fn storage(&self, resource_id: &str) -> Result<Arc<dyn RemoteStorage>> {
        let provider = self.source.provider(RemoteBindingCapability::Storage).await?;
        let initial = provider.load_storage(resource_id).await?;
        Ok(Arc::new(RefreshingStorage::new(
            provider,
            resource_id.to_string(),
            initial,
        )))
    }

    /// Loads a Key binding and keeps its short-lived credential lease fresh.
    pub async fn key(&self, resource_id: &str) -> Result<Arc<dyn Key>> {
        let provider = self.source.provider(RemoteBindingCapability::Storage).await?;
        provider.load_key(resource_id).await?;
        Ok(Arc::new(RefreshingKey::new(provider, resource_id.to_string())))
    }

    /// Loads a Sandbox binding for running untrusted code in the customer's cloud.
    ///
    /// No refreshing wrapper, mirroring the in-cloud accessor: a sandbox handle addresses a
    /// control plane rather than holding data-plane credentials, and each session capability is
    /// minted per call. The handle keeps the credential lease that was current when it was
    /// created, so call this again once that lease expires.
    ///
    /// The cached lease is served while it is unexpired even if a refresh fails, as Storage
    /// does: a caller already holding the handle keeps that authority either way, so refusing
    /// during a Manager blip withholds nothing.
    pub async fn sandbox(&self, resource_id: &str) -> Result<Arc<dyn Sandbox>> {
        self.source
            .provider(RemoteBindingCapability::Sandbox)
            .await?
            .resolver(resource_id)
            .await
            .sandbox()
            .await
    }

    /// Loads one short-lived managed AI binding lease.
    pub async fn ai(&self) -> Result<RemoteAiLease> {
        let provider = self.source.provider(RemoteBindingCapability::Storage).await?;
        let resolved = provider.source.resolve_ai().await?;
        resolved.into_ai_lease(provider.clock.now())
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
    Kms {
        binding: alien_core::AwsKmsKeyBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::AwsClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    #[serde(rename = "cloud-kms")]
    CloudKms {
        binding: alien_core::GcpCloudKmsKeyBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::GcpClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    #[serde(rename = "key-vault-key")]
    KeyVaultKey {
        binding: alien_core::AzureKeyVaultKeyBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::AzureClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    Bedrock {
        resource_id: String,
        binding: alien_core::BedrockAiBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::AwsClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    Vertex {
        resource_id: String,
        binding: alien_core::VertexAiBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::GcpClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    Foundry {
        resource_id: String,
        binding: alien_core::FoundryAiBinding,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::AzureClientConfig>,
        #[serde(rename = "expiresAt")]
        expires_at: DateTime<Utc>,
    },
    #[serde(rename = "sandbox-aws")]
    SandboxAws {
        binding: Box<alien_core::AwsSandboxBinding>,
        #[serde(rename = "clientConfig")]
        client_config: Box<alien_core::AwsClientConfig>,
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
    fn into_ai_lease(self, now: DateTime<Utc>) -> Result<RemoteAiLease> {
        let (resource_id, binding, client_config, expires_at) = match self {
            Self::Bedrock {
                resource_id,
                binding,
                client_config,
                expires_at,
            } => {
                validate_aws_remote_client_config(&client_config, expires_at)?;
                (
                    resource_id,
                    alien_core::AiBinding::Bedrock(binding),
                    alien_core::ClientConfig::Aws(client_config),
                    expires_at,
                )
            }
            Self::Vertex {
                resource_id,
                binding,
                client_config,
                expires_at,
            } => {
                validate_gcp_remote_client_config(&client_config)?;
                (
                    resource_id,
                    alien_core::AiBinding::Vertex(binding),
                    alien_core::ClientConfig::Gcp(client_config),
                    expires_at,
                )
            }
            Self::Foundry {
                resource_id,
                binding,
                client_config,
                expires_at,
            } => {
                validate_azure_remote_client_config(&client_config)?;
                (
                    resource_id,
                    alien_core::AiBinding::Foundry(binding),
                    alien_core::ClientConfig::Azure(client_config),
                    expires_at,
                )
            }
            _ => {
                return Err(AlienError::new(ErrorData::RemoteAccessFailed {
                    operation: format!(
                        "manager returned a non-AI lease for the deployment's remote AI binding"
                    ),
                }));
            }
        };
        if expires_at <= now {
            return Err(AlienError::new(ErrorData::RemoteAccessFailed {
                operation: format!(
                    "manager returned an expired lease for remote AI binding '{resource_id}'"
                ),
            }));
        }
        Ok(RemoteAiLease {
            resource_id,
            binding,
            client_config,
            expires_at,
        })
    }

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
                    serialize_remote_binding(alien_core::StorageBinding::S3(binding))?,
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
                    serialize_remote_binding(alien_core::StorageBinding::Blob(binding))?,
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
                    serialize_remote_binding(alien_core::StorageBinding::Gcs(binding))?,
                    expires_at,
                )
            }
            Self::Kms {
                binding,
                client_config,
                expires_at,
            } => {
                validate_aws_remote_client_config(&client_config, expires_at)?;
                (
                    alien_core::ClientConfig::Aws(client_config),
                    serialize_remote_binding(alien_core::KeyBinding::AwsKms(binding))?,
                    expires_at,
                )
            }
            Self::CloudKms {
                binding,
                client_config,
                expires_at,
            } => {
                validate_gcp_remote_client_config(&client_config)?;
                (
                    alien_core::ClientConfig::Gcp(client_config),
                    serialize_remote_binding(alien_core::KeyBinding::GcpCloudKms(binding))?,
                    expires_at,
                )
            }
            Self::KeyVaultKey {
                binding,
                client_config,
                expires_at,
            } => {
                validate_azure_remote_client_config(&client_config)?;
                (
                    alien_core::ClientConfig::Azure(client_config),
                    serialize_remote_binding(alien_core::KeyBinding::AzureKeyVault(binding))?,
                    expires_at,
                )
            }
            Self::SandboxAws {
                binding,
                client_config,
                expires_at,
            } => {
                validate_aws_remote_client_config(&client_config, expires_at)?;
                (
                    alien_core::ClientConfig::Aws(client_config),
                    serialize_remote_binding(alien_core::SandboxBinding::Aws(*binding))?,
                    expires_at,
                )
            }
            Self::Bedrock { .. } | Self::Vertex { .. } | Self::Foundry { .. } => {
                return Err(AlienError::new(ErrorData::RemoteAccessFailed {
                    operation: "use an AI lease as a Storage, Key or Sandbox binding".to_string(),
                }));
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
                serialize_remote_binding(alien_core::StorageBinding::Local(binding))?,
                expires_at,
            ),
        };
        Ok((client_config, binding, expires_at))
    }
}

fn serialize_remote_binding(binding: impl Serialize) -> Result<serde_json::Value> {
    serde_json::to_value(binding)
        .into_alien_error()
        .context(ErrorData::RemoteAccessFailed {
            operation: "convert typed remote binding lease".to_string(),
        })
}

fn invalid_remote_lease(provider: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::RemoteAccessFailed {
        operation: format!("validate {provider} remote credential lease: {reason}"),
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
    let alien_core::AwsCredentials::SessionCredentials {
        access_key_id,
        secret_access_key,
        session_token,
        expires_at,
    } = &config.credentials
    else {
        return Err(invalid_remote_lease(
            "AWS",
            "short-lived session credentials are required",
        ));
    };
    if access_key_id.is_empty() || secret_access_key.is_empty() || session_token.is_empty() {
        return Err(invalid_remote_lease(
            "AWS",
            "session credential fields must be nonempty",
        ));
    }
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
    let alien_core::GcpCredentials::AccessToken { token } = &config.credentials else {
        return Err(invalid_remote_lease(
            "GCP",
            "one access token without service endpoint overrides is required",
        ));
    };
    if config.service_overrides.is_some() || token.is_empty() {
        return Err(invalid_remote_lease(
            "GCP",
            "one nonempty access token without service endpoint overrides is required",
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
    let alien_core::AzureCredentials::AccessToken { token } = &config.credentials else {
        return Err(invalid_remote_lease(
            "Azure",
            "a storage-audience access token is required",
        ));
    };
    if token.is_empty() {
        return Err(invalid_remote_lease(
            "Azure",
            "the storage-audience access token must be nonempty",
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

#[derive(Clone, Copy)]
enum RequestedBindingKind {
    Storage,
    Key,
    Sandbox,
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
        BindingsProviderApi::load_storage(
            &*self.provider(RequestedBindingKind::Storage).await?,
            &self.resource_id,
        )
        .await
    }

    async fn key(&self) -> Result<Arc<dyn Key>> {
        BindingsProviderApi::load_key(
            &*self.provider(RequestedBindingKind::Key).await?,
            &self.resource_id,
        )
        .await
    }

    async fn sandbox(&self) -> Result<Arc<dyn Sandbox>> {
        BindingsProviderApi::load_sandbox(
            &*self.provider(RequestedBindingKind::Sandbox).await?,
            &self.resource_id,
        )
        .await
    }

    async fn provider(
        &self,
        requested_kind: RequestedBindingKind,
    ) -> Result<Arc<BindingsProvider>> {
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
            Ok(resolved) => self.build_cache_entry(resolved, now, requested_kind).await,
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
        requested_kind: RequestedBindingKind,
    ) -> Result<CachedRemoteBinding> {
        let (client_config, binding, expires_at) = resolved.into_provider_parts()?;
        if expires_at <= now {
            return Err(AlienError::new(ErrorData::RemoteAccessFailed {
                operation: format!(
                    "manager returned an expired lease for remote binding '{}'",
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
        match requested_kind {
            RequestedBindingKind::Storage => {
                BindingsProviderApi::load_storage(&*provider, &self.resource_id).await?;
            }
            RequestedBindingKind::Key => {
                BindingsProviderApi::load_key(&*provider, &self.resource_id).await?;
            }
            RequestedBindingKind::Sandbox => {
                BindingsProviderApi::load_sandbox(&*provider, &self.resource_id).await?;
            }
        }
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
