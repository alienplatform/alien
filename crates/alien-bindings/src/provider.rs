//! Unified BindingsProvider implementation that supports multiple cloud providers

use crate::{
    error::{ErrorData, Result},
    traits::{ArtifactRegistry, BindingsProviderApi, Build, Function, Kv, Queue, Storage, Vault},
};

use alien_client_config::ClientConfigExt;
use alien_core::{ClientConfig, Platform, StackState};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

/// Direct platform-specific bindings provider.
/// Routes to appropriate platform implementations based on binding configuration.
#[derive(Debug, Clone)]
pub struct BindingsProvider {
    client_config: ClientConfig,
    bindings: HashMap<String, serde_json::Value>,
}

impl BindingsProvider {
    /// Creates a new BindingsProvider with explicit credentials and bindings.
    ///
    /// This is the base constructor used by all other convenience constructors.
    pub fn new(
        client_config: ClientConfig,
        bindings: HashMap<String, serde_json::Value>,
    ) -> Result<Self> {
        Ok(Self {
            client_config,
            bindings,
        })
    }

    /// Creates a BindingsProvider from environment variables (for runtime use).
    ///
    /// This parses the platform from ALIEN_AGENT_TYPE, loads ClientConfig from environment,
    /// and extracts all ALIEN_*_BINDING environment variables.
    pub async fn from_env(env: HashMap<String, String>) -> Result<Self> {
        // 1. Parse platform from ALIEN_AGENT_TYPE
        let platform = crate::get_platform_from_env(&env)?;

        // 2. Load ClientConfig from environment
        let client_config = ClientConfig::from_env(platform, &env).await.map_err(|e| {
            AlienError::new(ErrorData::ClientConfigInvalid {
                platform,
                message: format!("Failed to load client config: {}", e),
            })
        })?;

        // 3. Parse all ALIEN_*_BINDING environment variables
        let bindings = Self::parse_bindings_from_env(&env)?;

        Self::new(client_config, bindings)
    }

    /// Parses all ALIEN_*_BINDING environment variables into a map.
    fn parse_bindings_from_env(
        env: &HashMap<String, String>,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let mut bindings = HashMap::new();
        for (key, value) in env {
            if key.starts_with("ALIEN_") && key.ends_with("_BINDING") {
                let binding_name = key
                    .strip_prefix("ALIEN_")
                    .unwrap()
                    .strip_suffix("_BINDING")
                    .unwrap()
                    .to_lowercase()
                    .replace('_', "-");
                let parsed: serde_json::Value = serde_json::from_str(value)
                    .into_alien_error()
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.clone(),
                        reason: "Failed to parse binding JSON".to_string(),
                    })?;
                bindings.insert(binding_name, parsed);
            }
        }
        Ok(bindings)
    }

    /// Pattern 1: Creates a BindingsProvider from stack state and client config.
    ///
    /// Convenience helper that extracts bindings from stack state's remote_binding_params.
    /// Only resources with `remote_access: true` will have binding params available.
    ///
    /// **When to use:** You already have `ClientConfig` and `StackState`.
    ///
    /// **Example use cases:**
    /// - alien-deployment (has credentials from RemoteAccessResolver)
    /// - Platform API backend (has stack state from DB)
    pub fn from_stack_state(stack_state: &StackState, client_config: ClientConfig) -> Result<Self> {
        let bindings = stack_state
            .resources
            .iter()
            .filter_map(|(id, state)| {
                state
                    .remote_binding_params
                    .as_ref()
                    .map(|p| (id.clone(), p.clone()))
            })
            .collect();

        Self::new(client_config, bindings)
    }

    /// Pattern 2: Creates a BindingsProvider for remote agent access.
    ///
    /// Fetches credentials remotely using agent ID and auth token, then creates the provider.
    ///
    /// **When to use:** You have an agent ID and auth token, but no credentials yet.
    ///
    /// **Example use cases:**
    /// - CLI commands (e.g., `alien secrets set`)
    /// - External applications connecting to agent
    /// - Local development tools
    ///
    /// **What it does internally:**
    /// 1. GET /api/agents/{id} - Returns agent info (stackState, platform, agentManagerId)
    /// 2. GET /api/agent-managers/{agentManagerId} - Returns agent manager URL
    /// 3. POST {managerUrl}/v1/agent/resolve-credentials - Resolves credentials
    /// 4. Creates BindingsProvider using from_stack_state()
    #[cfg(feature = "platform-sdk")]
    pub async fn for_remote_agent(
        agent_id: &str,
        _token: &str,
        api_base_url: Option<&str>,
    ) -> Result<Self> {
        let base_url = api_base_url.unwrap_or("https://api.alien.dev");

        // Create SDK client
        let sdk_client = alien_platform_api::Client::new(base_url);

        // 1. Get agent info
        let agent_response = sdk_client
            .get_agent()
            .id(agent_id)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "fetch agent from Platform API".to_string(),
            })?
            .into_inner();

        // 2. Get agent manager URL
        let agent_manager_id = agent_response.agent_manager_id.ok_or_else(|| {
            AlienError::new(ErrorData::RemoteAccessFailed {
                operation: "fetch agent manager from Platform API".to_string(),
            })
        })?;

        let agent_manager_response = sdk_client
            .get_agent_manager()
            .id(&agent_manager_id.to_string())
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "fetch agent manager from Platform API".to_string(),
            })?
            .into_inner();

        // 3. Convert SDK stack state to alien-core StackState
        let stack_state = agent_response.stack_state.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::RemoteAccessFailed {
                operation: "Agent has no stack state (not deployed yet)".to_string(),
            })
        })?;

        let alien_stack_state = conversions::convert_stack_state(stack_state)?;

        // 4. Resolve client config from manager
        let manager_url = agent_manager_response.url.ok_or_else(|| {
            AlienError::new(ErrorData::RemoteAccessFailed {
                operation: "fetch manager URL from Platform API".to_string(),
            })
        })?;

        let http_client = reqwest::Client::new();
        let client_config = http_client
            .post(format!(
                "{}/v1/agent/resolve-credentials",
                manager_url
            ))
            .json(&serde_json::json!({
                "platform": agent_response.platform,
                "stackState": stack_state,
            }))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "resolve credentials from agent-manager".to_string(),
            })?
            .json::<ResolveCredentialsResponse>()
            .await
            .into_alien_error()
            .context(ErrorData::RemoteAccessFailed {
                operation: "parse credentials response".to_string(),
            })?
            .client_config;

        // 5. Create provider using from_stack_state (which extracts bindings from stack_state)
        Self::from_stack_state(&alien_stack_state, client_config)
    }
}

#[cfg(feature = "platform-sdk")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveCredentialsResponse {
    client_config: ClientConfig,
}

#[async_trait]
impl BindingsProviderApi for BindingsProvider {
    async fn load_storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>> {
        use alien_core::bindings::StorageBinding;

        // Get binding JSON from our pre-parsed map
        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        // Parse to strongly-typed binding
        let binding: StorageBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse storage binding".to_string(),
            })?;

        match binding {
            #[cfg(feature = "aws")]
            StorageBinding::S3(config) => {
                use crate::providers::storage::aws_s3::S3Storage;

                // Get AWS config from our stored ClientConfig
                let aws_config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;

                // Extract bucket name from binding
                let bucket_name = config
                    .bucket_name
                    .into_value(binding_name, "bucket_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract bucket_name from S3 binding".to_string(),
                    })?;

                let storage = Arc::new(S3Storage::new(bucket_name, aws_config)?);
                Ok(storage)
            }
            #[cfg(not(feature = "aws"))]
            StorageBinding::S3 { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "aws".to_string(),
            })),

            #[cfg(feature = "azure")]
            StorageBinding::Blob(config) => {
                use crate::providers::storage::azure_blob::BlobStorage;

                let azure_config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;

                // Extract container and account names from binding
                let container_name = config
                    .container_name
                    .into_value(binding_name, "container_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract container_name from Blob binding".to_string(),
                    })?;

                let account_name = config
                    .account_name
                    .into_value(binding_name, "account_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract account_name from Blob binding".to_string(),
                    })?;

                let storage = Arc::new(BlobStorage::new(
                    container_name,
                    account_name,
                    azure_config,
                )?);
                Ok(storage)
            }
            #[cfg(not(feature = "azure"))]
            StorageBinding::Blob { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "azure".to_string(),
            })),

            #[cfg(feature = "gcp")]
            StorageBinding::Gcs(config) => {
                use crate::providers::storage::gcp_gcs::GcsStorage;

                let gcp_config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;

                // Extract bucket name from binding
                let bucket_name = config
                    .bucket_name
                    .into_value(binding_name, "bucket_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract bucket_name from Gcs binding".to_string(),
                    })?;

                let storage = Arc::new(GcsStorage::new(bucket_name, gcp_config)?);
                Ok(storage)
            }
            #[cfg(not(feature = "gcp"))]
            StorageBinding::Gcs { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "gcp".to_string(),
            })),

            #[cfg(feature = "local")]
            StorageBinding::Local(config) => {
                use crate::providers::storage::local::LocalStorage;

                // Extract storage path from binding
                let storage_path = config
                    .storage_path
                    .into_value(binding_name, "storage_path")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract storage_path from Local binding".to_string(),
                    })?;

                let storage = Arc::new(LocalStorage::new(storage_path)?);
                Ok(storage)
            }
            #[cfg(not(feature = "local"))]
            StorageBinding::Local { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "local".to_string(),
            })),
        }
    }

    async fn load_build(&self, binding_name: &str) -> Result<Arc<dyn Build>> {
        use alien_core::bindings::BuildBinding;

        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        let binding: BuildBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse build binding".to_string(),
            })?;

        match binding {
            #[cfg(feature = "aws")]
            BuildBinding::Codebuild { .. } => {
                use crate::providers::build::codebuild::CodebuildBuild;

                let aws_config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;

                let build = Arc::new(
                    CodebuildBuild::new(binding_name.to_string(), binding, aws_config)
                        .await
                        .context(ErrorData::BindingConfigInvalid {
                            binding_name: binding_name.to_string(),
                            reason: "Failed to initialize AWS CodeBuild client".to_string(),
                        })?,
                );
                Ok(build)
            }
            #[cfg(not(feature = "aws"))]
            BuildBinding::Codebuild { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "aws".to_string(),
            })),

            #[cfg(feature = "azure")]
            BuildBinding::Aca { .. } => {
                use crate::providers::build::aca::AcaBuild;

                let azure_config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;

                let build = Arc::new(
                    AcaBuild::new(binding_name.to_string(), binding, azure_config)
                        .await
                        .context(ErrorData::BindingConfigInvalid {
                            binding_name: binding_name.to_string(),
                            reason: "Failed to initialize Azure Container Apps build".to_string(),
                        })?,
                );
                Ok(build)
            }
            #[cfg(not(feature = "azure"))]
            BuildBinding::Aca { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "azure".to_string(),
            })),

            #[cfg(feature = "gcp")]
            BuildBinding::Cloudbuild { .. } => {
                use crate::providers::build::cloudbuild::CloudbuildBuild;

                let gcp_config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;

                let build = Arc::new(
                    CloudbuildBuild::new(binding_name.to_string(), binding, gcp_config)
                        .await
                        .context(ErrorData::BindingConfigInvalid {
                            binding_name: binding_name.to_string(),
                            reason: "Failed to initialize GCP Cloud Build client".to_string(),
                        })?,
                );
                Ok(build)
            }
            #[cfg(not(feature = "gcp"))]
            BuildBinding::Cloudbuild { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "gcp".to_string(),
            })),

            #[cfg(feature = "local")]
            BuildBinding::Local { .. } => {
                use crate::providers::build::local::LocalBuild;

                let build = Arc::new(LocalBuild::new(binding_name.to_string(), binding)?);
                Ok(build)
            }
            #[cfg(not(feature = "local"))]
            BuildBinding::Local { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "local".to_string(),
            })),

            #[cfg(feature = "kubernetes")]
            BuildBinding::Kubernetes { .. } => {
                use crate::providers::build::kubernetes::KubernetesBuild;

                let build =
                    Arc::new(KubernetesBuild::new(binding_name.to_string(), binding).await?);
                Ok(build)
            }
            #[cfg(not(feature = "kubernetes"))]
            BuildBinding::Kubernetes { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "kubernetes".to_string(),
            })),
        }
    }

    async fn load_artifact_registry(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn ArtifactRegistry>> {
        use alien_core::bindings::ArtifactRegistryBinding;

        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        let binding: ArtifactRegistryBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse artifact registry binding".to_string(),
            })?;

        match binding {
            #[cfg(feature = "aws")]
            ArtifactRegistryBinding::Ecr { .. } => {
                use crate::providers::artifact_registry::ecr::EcrArtifactRegistry;

                let aws_config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;

                let registry = Arc::new(
                    EcrArtifactRegistry::new(binding_name.to_string(), binding, aws_config)
                        .await
                        .context(ErrorData::BindingConfigInvalid {
                            binding_name: binding_name.to_string(),
                            reason: "Failed to initialize AWS ECR artifact registry".to_string(),
                        })?,
                );
                Ok(registry)
            }
            #[cfg(not(feature = "aws"))]
            ArtifactRegistryBinding::Ecr { .. } => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "aws".to_string(),
                }))
            }

            #[cfg(feature = "azure")]
            ArtifactRegistryBinding::Acr { .. } => {
                use crate::providers::artifact_registry::acr::AcrArtifactRegistry;

                let azure_config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;

                let registry = Arc::new(
                    AcrArtifactRegistry::new(binding_name.to_string(), binding, azure_config)
                        .await
                        .context(ErrorData::BindingConfigInvalid {
                            binding_name: binding_name.to_string(),
                            reason: "Failed to initialize Azure ACR artifact registry".to_string(),
                        })?,
                );
                Ok(registry)
            }
            #[cfg(not(feature = "azure"))]
            ArtifactRegistryBinding::Acr { .. } => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "azure".to_string(),
                }))
            }

            #[cfg(feature = "gcp")]
            ArtifactRegistryBinding::Gar { .. } => {
                use crate::providers::artifact_registry::gar::GarArtifactRegistry;

                let gcp_config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;

                let registry = Arc::new(
                    GarArtifactRegistry::new(binding_name.to_string(), binding, gcp_config)
                        .await
                        .context(ErrorData::BindingConfigInvalid {
                            binding_name: binding_name.to_string(),
                            reason: "Failed to initialize GCP GAR artifact registry".to_string(),
                        })?,
                );
                Ok(registry)
            }
            #[cfg(not(feature = "gcp"))]
            ArtifactRegistryBinding::Gar { .. } => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "gcp".to_string(),
                }))
            }

            #[cfg(feature = "local")]
            ArtifactRegistryBinding::Local { .. } => {
                use crate::providers::artifact_registry::local::LocalArtifactRegistry;

                let registry = Arc::new(
                    LocalArtifactRegistry::new(binding_name.to_string(), binding.clone()).await?,
                );
                Ok(registry)
            }
            #[cfg(not(feature = "local"))]
            ArtifactRegistryBinding::Local { .. } => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "local".to_string(),
                }))
            }
        }
    }

    async fn load_vault(&self, binding_name: &str) -> Result<Arc<dyn Vault>> {
        use alien_core::bindings::VaultBinding;

        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        let binding: VaultBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse vault binding".to_string(),
            })?;

        match binding {
            #[cfg(feature = "aws")]
            VaultBinding::ParameterStore(config) => {
                use crate::providers::vault::aws_parameter_store::AwsParameterStoreVault;
                use alien_aws_clients::ssm::SsmClient;

                let aws_config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;

                let client = Arc::new(SsmClient::new(
                    crate::http_client::create_http_client(),
                    aws_config.clone(),
                ));

                // Extract the vault prefix from the binding configuration
                let vault_prefix = config
                    .vault_prefix
                    .into_value(&binding_name, "vault_prefix")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract vault_prefix from ParameterStore binding"
                            .to_string(),
                    })?;

                let vault = Arc::new(AwsParameterStoreVault::new(client, vault_prefix));
                Ok(vault)
            }
            #[cfg(not(feature = "aws"))]
            VaultBinding::ParameterStore(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "aws".to_string(),
            })),

            #[cfg(feature = "azure")]
            VaultBinding::KeyVault(config) => {
                use crate::providers::vault::azure_key_vault::AzureKeyVault;
                use alien_azure_clients::keyvault::AzureKeyVaultSecretsClient;

                let azure_config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;

                let client = Arc::new(AzureKeyVaultSecretsClient::new(
                    crate::http_client::create_http_client(),
                    azure_config.clone(),
                ));

                // Extract the vault name from the binding configuration
                let vault_name = config
                    .vault_name
                    .into_value(&binding_name, "vault_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract vault_name from KeyVault binding".to_string(),
                    })?;

                // Construct the vault base URL
                // Azure Key Vault URLs typically follow: https://{vault-name}.vault.azure.net/
                let vault_base_url = format!("https://{}.vault.azure.net", vault_name);

                let vault = Arc::new(AzureKeyVault::new(client, vault_base_url));
                Ok(vault)
            }
            #[cfg(not(feature = "azure"))]
            VaultBinding::KeyVault(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "azure".to_string(),
            })),

            #[cfg(feature = "gcp")]
            VaultBinding::SecretManager(config) => {
                use crate::providers::vault::gcp_secret_manager::GcpSecretManagerVault;
                use alien_gcp_clients::secret_manager::SecretManagerClient;

                let gcp_config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;

                let client = Arc::new(SecretManagerClient::new(
                    crate::http_client::create_http_client(),
                    gcp_config.clone(),
                ));

                // Extract the vault prefix from the binding configuration
                let vault_prefix = config
                    .vault_prefix
                    .into_value(&binding_name, "vault_prefix")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract vault_prefix from SecretManager binding"
                            .to_string(),
                    })?;

                let vault = Arc::new(GcpSecretManagerVault::new(
                    client,
                    vault_prefix,
                    gcp_config.project_id.clone(),
                ));
                Ok(vault)
            }
            #[cfg(not(feature = "gcp"))]
            VaultBinding::SecretManager(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "gcp".to_string(),
            })),

            #[cfg(feature = "local")]
            VaultBinding::Local(config) => {
                use crate::providers::vault::local::LocalVault;

                let vault_dir = config
                    .data_dir
                    .into_value(binding_name, "data_dir")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract data_dir from vault binding".to_string(),
                    })?;

                let vault = Arc::new(LocalVault::new(
                    binding_name.to_string(),
                    std::path::PathBuf::from(vault_dir),
                ));
                Ok(vault)
            }
            #[cfg(not(feature = "local"))]
            VaultBinding::Local { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "local".to_string(),
            })),

            #[cfg(feature = "kubernetes")]
            VaultBinding::KubernetesSecret(config) => {
                use crate::providers::vault::kubernetes_secret::KubernetesSecretVault;
                use alien_k8s_clients::{secrets::SecretsApi, KubernetesClient};

                let kubernetes_config =
                    self.client_config.kubernetes_config().ok_or_else(|| {
                        AlienError::new(ErrorData::ClientConfigInvalid {
                            platform: Platform::Kubernetes,
                            message: "Kubernetes config not available".to_string(),
                        })
                    })?;

                let kubernetes_client = KubernetesClient::new(kubernetes_config.clone())
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to create Kubernetes client for vault".to_string(),
                        resource_id: None,
                    })?;

                let client: Arc<dyn SecretsApi> = Arc::new(kubernetes_client);

                // Extract namespace and vault prefix from binding
                let namespace = config
                    .namespace
                    .into_value(binding_name, "namespace")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract namespace from KubernetesSecret binding"
                            .to_string(),
                    })?;

                let vault_prefix = config
                    .vault_prefix
                    .into_value(binding_name, "vault_prefix")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract vault_prefix from KubernetesSecret binding"
                            .to_string(),
                    })?;

                let vault = Arc::new(KubernetesSecretVault::new(client, namespace, vault_prefix));
                Ok(vault)
            }
            #[cfg(not(feature = "kubernetes"))]
            VaultBinding::KubernetesSecret(_) => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "kubernetes".to_string(),
                }))
            }
        }
    }

    async fn load_kv(&self, binding_name: &str) -> Result<Arc<dyn Kv>> {
        use alien_core::bindings::KvBinding;

        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        let binding: KvBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse KV binding".to_string(),
            })?;

        match binding {
            #[cfg(feature = "aws")]
            KvBinding::Dynamodb(config) => {
                use crate::providers::kv::aws_dynamodb::AwsDynamodbKv;

                let table_name = config
                    .table_name
                    .into_value(binding_name, "table_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract table_name from DynamoDB binding".to_string(),
                    })?;

                let aws_config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;

                let kv_impl = AwsDynamodbKv::new(table_name, aws_config.clone()).await?;
                let kv: Arc<dyn Kv> = Arc::new(kv_impl);
                Ok(kv)
            }
            #[cfg(not(feature = "aws"))]
            KvBinding::Dynamodb(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "aws".to_string(),
            })),

            #[cfg(feature = "gcp")]
            KvBinding::Firestore(config) => {
                use crate::providers::kv::gcp_firestore::GcpFirestoreKv;
                use alien_gcp_clients::firestore::FirestoreClient;

                let gcp_config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;

                let client = FirestoreClient::new(
                    crate::http_client::create_http_client(),
                    gcp_config.clone(),
                );

                let project_id = config
                    .project_id
                    .into_value(binding_name, "project_id")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract project_id from Firestore binding".to_string(),
                    })?;

                let database_id = config
                    .database_id
                    .into_value(binding_name, "database_id")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract database_id from Firestore binding".to_string(),
                    })?;

                let collection_name = config
                    .collection_name
                    .into_value(binding_name, "collection_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract collection_name from Firestore binding"
                            .to_string(),
                    })?;

                let kv = Arc::new(GcpFirestoreKv::new(
                    client,
                    project_id,
                    database_id,
                    collection_name,
                )?);
                Ok(kv)
            }
            #[cfg(not(feature = "gcp"))]
            KvBinding::Firestore(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "gcp".to_string(),
            })),

            #[cfg(feature = "azure")]
            KvBinding::TableStorage(config) => {
                use crate::providers::kv::azure_table_storage::AzureTableStorageKv;
                use alien_azure_clients::storage_accounts::{
                    AzureStorageAccountsClient, StorageAccountsApi,
                };
                use alien_azure_clients::tables::AzureTableStorageClient;

                let azure_config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;

                let resource_group_name = config
                    .resource_group_name
                    .into_value(binding_name, "resource_group_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract resource_group_name from TableStorage binding"
                            .to_string(),
                    })?;

                let account_name = config
                    .account_name
                    .into_value(binding_name, "account_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract account_name from TableStorage binding"
                            .to_string(),
                    })?;

                let table_name = config
                    .table_name
                    .into_value(binding_name, "table_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract table_name from TableStorage binding"
                            .to_string(),
                    })?;

                // Fetch storage account key once during initialization
                let storage_accounts_client = AzureStorageAccountsClient::new(
                    crate::http_client::create_http_client(),
                    azure_config.clone(),
                );

                let keys_result = storage_accounts_client
                    .list_storage_account_keys(&resource_group_name, &account_name)
                    .await
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to fetch storage account keys".to_string(),
                    })?;

                let storage_account_key = keys_result
                    .keys
                    .into_iter()
                    .find(|key| key.key_name.as_deref() == Some("key1"))
                    .and_then(|key| key.value)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::BindingConfigInvalid {
                            binding_name: binding_name.to_string(),
                            reason: format!(
                                "No access key found for storage account '{}'",
                                account_name
                            ),
                        })
                    })?;

                let client = AzureTableStorageClient::new(
                    crate::http_client::create_http_client(),
                    azure_config.clone(),
                    storage_account_key,
                );

                let kv_impl =
                    AzureTableStorageKv::new(client, resource_group_name, account_name, table_name);
                let kv: Arc<dyn Kv> = Arc::new(kv_impl);
                Ok(kv)
            }
            #[cfg(not(feature = "azure"))]
            KvBinding::TableStorage(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "azure".to_string(),
            })),

            #[cfg(feature = "local")]
            KvBinding::Local(local_binding) => {
                use crate::providers::kv::local::LocalKv;
                use std::path::PathBuf;

                // Get data directory from binding
                let data_dir = PathBuf::from(
                    local_binding
                        .data_dir
                        .into_value(binding_name, "data_dir")
                        .context(ErrorData::BindingConfigInvalid {
                            binding_name: binding_name.to_string(),
                            reason: "Failed to extract data_dir from Local binding".to_string(),
                        })?,
                );

                // Create local disk-persisted KV implementation
                let kv_impl = LocalKv::new(data_dir).await?;

                let kv: Arc<dyn Kv> = Arc::new(kv_impl);
                Ok(kv)
            }
            #[cfg(not(feature = "local"))]
            KvBinding::Local { .. } => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "local".to_string(),
            })),

            KvBinding::Redis(_) => Err(AlienError::new(ErrorData::NotImplemented {
                operation: "Redis KV binding".to_string(),
                reason: "Redis KV provider is not yet implemented".to_string(),
            })),
        }
    }

    async fn load_queue(&self, binding_name: &str) -> Result<Arc<dyn Queue>> {
        use alien_core::bindings::QueueBinding;

        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        let binding: QueueBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse Queue binding".to_string(),
            })?;

        match binding {
            #[cfg(feature = "aws")]
            QueueBinding::Sqs(config) => {
                use crate::providers::queue::aws_sqs::AwsSqsQueue;

                let queue_url = config
                    .queue_url
                    .into_value(binding_name, "queue_url")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract queue_url from SQS binding".to_string(),
                    })?;

                let aws_config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;
                let q: Arc<dyn Queue> =
                    Arc::new(AwsSqsQueue::new(queue_url, aws_config.clone()).await?);
                Ok(q)
            }
            #[cfg(not(feature = "aws"))]
            QueueBinding::Sqs(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "aws".to_string(),
            })),

            #[cfg(feature = "gcp")]
            QueueBinding::Pubsub(config) => {
                use crate::providers::queue::gcp_pubsub::GcpPubSubQueue;
                let topic_name = config.topic.into_value(binding_name, "topic").context(
                    ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract topic".to_string(),
                    },
                )?;
                let subscription_name = config
                    .subscription
                    .into_value(binding_name, "subscription")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract subscription".to_string(),
                    })?;
                let gcp_config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;

                // Construct full resource names using the project ID from config
                let topic = if topic_name.starts_with("projects/") {
                    topic_name // Already a full resource name
                } else {
                    format!("projects/{}/topics/{}", gcp_config.project_id, topic_name)
                };
                let subscription = if subscription_name.starts_with("projects/") {
                    subscription_name // Already a full resource name
                } else {
                    format!(
                        "projects/{}/subscriptions/{}",
                        gcp_config.project_id, subscription_name
                    )
                };

                let q: Arc<dyn Queue> =
                    Arc::new(GcpPubSubQueue::new(topic, subscription, gcp_config.clone()).await?);
                Ok(q)
            }
            #[cfg(not(feature = "gcp"))]
            QueueBinding::Pubsub(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "gcp".to_string(),
            })),

            #[cfg(feature = "azure")]
            QueueBinding::Servicebus(config) => {
                use crate::providers::queue::azure_service_bus::AzureServiceBusQueue;
                let namespace = config
                    .namespace
                    .into_value(binding_name, "namespace")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract namespace".to_string(),
                    })?;
                let queue_name = config
                    .queue_name
                    .into_value(binding_name, "queue_name")
                    .context(ErrorData::BindingConfigInvalid {
                        binding_name: binding_name.to_string(),
                        reason: "Failed to extract queue_name".to_string(),
                    })?;
                let azure_config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;
                let q: Arc<dyn Queue> = Arc::new(
                    AzureServiceBusQueue::new(namespace, queue_name, azure_config.clone()).await?,
                );
                Ok(q)
            }
            #[cfg(not(feature = "azure"))]
            QueueBinding::Servicebus(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "azure".to_string(),
            })),
        }
    }

    async fn load_function(&self, binding_name: &str) -> Result<Arc<dyn Function>> {
        use alien_core::bindings::FunctionBinding;

        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        let binding: FunctionBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse function binding".to_string(),
            })?;

        match binding {
            #[cfg(feature = "aws")]
            FunctionBinding::Lambda(lambda_binding) => {
                use crate::providers::function::LambdaFunction;

                let aws_config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;
                let client = crate::http_client::create_http_client();

                let function_impl = LambdaFunction::new(client, aws_config.clone(), lambda_binding);
                let function: Arc<dyn Function> = Arc::new(function_impl);
                Ok(function)
            }
            #[cfg(not(feature = "aws"))]
            FunctionBinding::Lambda(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "aws".to_string(),
            })),

            #[cfg(feature = "gcp")]
            FunctionBinding::CloudRun(cloudrun_binding) => {
                use crate::providers::function::CloudRunFunction;

                let gcp_config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;
                let client = crate::http_client::create_http_client();

                let function_impl =
                    CloudRunFunction::new(client, gcp_config.clone(), cloudrun_binding);
                let function: Arc<dyn Function> = Arc::new(function_impl);
                Ok(function)
            }
            #[cfg(not(feature = "gcp"))]
            FunctionBinding::CloudRun(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "gcp".to_string(),
            })),

            #[cfg(feature = "azure")]
            FunctionBinding::ContainerApp(container_app_binding) => {
                use crate::providers::function::ContainerAppFunction;

                let azure_config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;
                let client = crate::http_client::create_http_client();

                let function_impl =
                    ContainerAppFunction::new(client, azure_config.clone(), container_app_binding);
                let function: Arc<dyn Function> = Arc::new(function_impl);
                Ok(function)
            }
            #[cfg(not(feature = "azure"))]
            FunctionBinding::ContainerApp(_) => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "azure".to_string(),
                }))
            }

            #[cfg(feature = "local")]
            FunctionBinding::Local(local_binding) => {
                use crate::providers::function::LocalFunction;

                let function_impl = LocalFunction::new(local_binding);
                let function: Arc<dyn Function> = Arc::new(function_impl);
                Ok(function)
            }
            #[cfg(not(feature = "local"))]
            FunctionBinding::Local(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "local".to_string(),
            })),

            #[cfg(feature = "kubernetes")]
            FunctionBinding::Kubernetes(kubernetes_binding) => {
                use crate::providers::function::KubernetesFunction;

                let function_impl =
                    KubernetesFunction::new(binding_name.to_string(), kubernetes_binding)?;
                let function: Arc<dyn Function> = Arc::new(function_impl);
                Ok(function)
            }
            #[cfg(not(feature = "kubernetes"))]
            FunctionBinding::Kubernetes(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "kubernetes".to_string(),
            })),
        }
    }

    async fn load_container(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn crate::traits::Container>> {
        use alien_core::bindings::ContainerBinding;

        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        let binding: ContainerBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse container binding".to_string(),
            })?;

        match binding {
            ContainerBinding::Horizon(horizon_binding) => {
                use crate::providers::container::HorizonContainer;

                let container_impl = HorizonContainer::new(horizon_binding)?;
                let container: Arc<dyn crate::traits::Container> = Arc::new(container_impl);
                Ok(container)
            }

            #[cfg(feature = "local")]
            ContainerBinding::Local(local_binding) => {
                use crate::providers::container::LocalContainer;

                let container_impl = LocalContainer::new(local_binding)?;
                let container: Arc<dyn crate::traits::Container> = Arc::new(container_impl);
                Ok(container)
            }
            #[cfg(not(feature = "local"))]
            ContainerBinding::Local(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "local".to_string(),
            })),

            #[cfg(feature = "kubernetes")]
            ContainerBinding::Kubernetes(kubernetes_binding) => {
                use crate::providers::container::KubernetesContainer;

                let container_impl =
                    KubernetesContainer::new(binding_name.to_string(), kubernetes_binding)?;
                let container: Arc<dyn crate::traits::Container> = Arc::new(container_impl);
                Ok(container)
            }
            #[cfg(not(feature = "kubernetes"))]
            ContainerBinding::Kubernetes(_) => Err(AlienError::new(ErrorData::FeatureNotEnabled {
                feature: "kubernetes".to_string(),
            })),
        }
    }

    async fn load_service_account(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn crate::traits::ServiceAccount>> {
        use alien_core::bindings::ServiceAccountBinding;

        let binding_json = self.bindings.get(binding_name).ok_or_else(|| {
            AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Binding not found".to_string(),
            })
        })?;

        let binding: ServiceAccountBinding = serde_json::from_value(binding_json.clone())
            .into_alien_error()
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to parse service account binding".to_string(),
            })?;

        match binding {
            #[cfg(feature = "aws")]
            ServiceAccountBinding::AwsIam(aws_binding) => {
                use crate::providers::service_account::aws_iam::AwsIamServiceAccount;

                let aws_config = self.client_config.aws_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: "AWS config not available".to_string(),
                    })
                })?;
                let client = crate::http_client::create_http_client();

                let service_account_impl =
                    AwsIamServiceAccount::new(client, aws_config.clone(), aws_binding);
                let service_account: Arc<dyn crate::traits::ServiceAccount> =
                    Arc::new(service_account_impl);
                Ok(service_account)
            }
            #[cfg(not(feature = "aws"))]
            ServiceAccountBinding::AwsIam(_) => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "aws".to_string(),
                }))
            }

            #[cfg(feature = "gcp")]
            ServiceAccountBinding::GcpServiceAccount(gcp_binding) => {
                use crate::providers::service_account::gcp_service_account::GcpServiceAccount;

                let gcp_config = self.client_config.gcp_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Gcp,
                        message: "GCP config not available".to_string(),
                    })
                })?;
                let client = crate::http_client::create_http_client();

                let service_account_impl =
                    GcpServiceAccount::new(client, gcp_config.clone(), gcp_binding);
                let service_account: Arc<dyn crate::traits::ServiceAccount> =
                    Arc::new(service_account_impl);
                Ok(service_account)
            }
            #[cfg(not(feature = "gcp"))]
            ServiceAccountBinding::GcpServiceAccount(_) => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "gcp".to_string(),
                }))
            }

            #[cfg(feature = "azure")]
            ServiceAccountBinding::AzureManagedIdentity(azure_binding) => {
                use crate::providers::service_account::azure_managed_identity::AzureManagedIdentityServiceAccount;

                let azure_config = self.client_config.azure_config().ok_or_else(|| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Azure,
                        message: "Azure config not available".to_string(),
                    })
                })?;

                let service_account_impl =
                    AzureManagedIdentityServiceAccount::new(azure_config.clone(), azure_binding);
                let service_account: Arc<dyn crate::traits::ServiceAccount> =
                    Arc::new(service_account_impl);
                Ok(service_account)
            }
            #[cfg(not(feature = "azure"))]
            ServiceAccountBinding::AzureManagedIdentity(_) => {
                Err(AlienError::new(ErrorData::FeatureNotEnabled {
                    feature: "azure".to_string(),
                }))
            }
        }
    }
}

/// Conversion functions between SDK types and alien-core types
#[cfg(feature = "platform-sdk")]
mod conversions {
    use super::*;
    use serde::Serialize;

    /// Convert SDK AgentStackState to alien-core StackState
    /// Generic over any serializable type since we convert via JSON
    pub fn convert_stack_state<T: Serialize>(sdk_stack_state: &T) -> Result<StackState> {
        // Convert via JSON serialization/deserialization (same pattern as deploy.rs)
        let stack_state: StackState = serde_json::from_value(
            serde_json::to_value(sdk_stack_state)
                .into_alien_error()
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: "stack_state".to_string(),
                    reason: "Failed to serialize SDK stack state".to_string(),
                })?,
        )
        .into_alien_error()
        .context(ErrorData::BindingConfigInvalid {
            binding_name: "stack_state".to_string(),
            reason: "Failed to parse stack state".to_string(),
        })?;

        Ok(stack_state)
    }
}
