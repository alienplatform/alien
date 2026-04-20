use crate::{
    error::{map_cloud_client_error, ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding, RegistryAuthMethod,
        CrossAccountAccess, CrossAccountPermissions, RepositoryResponse,
    },
};
use alien_azure_clients::long_running_operation::LongRunningOperationClient;
use alien_azure_clients::{
    containerregistry::{AzureContainerRegistryClient, ContainerRegistryApi},
    AzureClientConfig, AzureTokenCache,
};
use alien_core::bindings::ArtifactRegistryBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use tracing::{info, warn};

/// Azure Container Registry implementation of the ArtifactRegistry binding.
#[derive(Debug)]
pub struct AcrArtifactRegistry {
    acr_client: AzureContainerRegistryClient,
    lro_client: LongRunningOperationClient,
    binding_name: String,
    registry_name: String,
    registry_endpoint: String,
    resource_group_name: String,
    repository_prefix: String,
    /// Azure credentials for direct registry access (AAD token exchange).
    azure_token_cache: AzureTokenCache,
    http_client: reqwest::Client,
}

impl AcrArtifactRegistry {
    /// Creates a new Azure Container Registry artifact registry binding from binding parameters.
    ///
    /// # Arguments
    /// * `binding_name` - The name of this binding
    /// * `binding` - The parsed binding parameters
    pub async fn new(
        binding_name: String,
        binding: ArtifactRegistryBinding,
        azure_config: &AzureClientConfig,
    ) -> Result<Self> {
        info!(
            binding_name = %binding_name,
            "Initializing Azure Container Registry"
        );

        // Extract values from binding
        let config = match binding {
            ArtifactRegistryBinding::Acr(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected ACR binding, got different service type".to_string(),
                }));
            }
        };

        let registry_name = config
            .registry_name
            .into_value(&binding_name, "registry_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract registry_name from binding".to_string(),
            })?;

        let resource_group_name = config
            .resource_group_name
            .into_value(&binding_name, "resource_group_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract resource_group_name from binding".to_string(),
            })?;

        // Derive registry endpoint from registry name
        let registry_endpoint = format!("{}.azurecr.io", registry_name);
        let client = crate::http_client::create_http_client();
        let token_cache_1 = AzureTokenCache::new(azure_config.clone());
        let token_cache_2 = AzureTokenCache::new(azure_config.clone());
        let token_cache_3 = AzureTokenCache::new(azure_config.clone());
        let acr_client = AzureContainerRegistryClient::new(client.clone(), token_cache_1);
        let lro_client = LongRunningOperationClient::new(client.clone(), token_cache_2);

        let repository_prefix = match config.repository_prefix {
            Some(bv) => bv
                .into_value(&binding_name, "repository_prefix")
                .unwrap_or_default(),
            None => String::new(),
        };

        Ok(Self {
            acr_client,
            lro_client,
            binding_name,
            registry_name,
            registry_endpoint,
            resource_group_name,
            repository_prefix,
            azure_token_cache: token_cache_3,
            http_client: client,
        })
    }

    /// Creates a valid Azure resource name from a repository name.
    /// Azure resource names must:
    /// - Be less than 50 characters
    /// - Start with a letter
    /// - Only contain alphanumeric characters and hyphens
    fn make_azure_resource_name(&self, repo_name: &str, suffix: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let max_length = 49;
        let combined = format!("{}-{}", repo_name, suffix);

        // Azure ACR resource names must: start with a letter, contain only
        // alphanumeric + single hyphens, no underscores, no consecutive hyphens,
        // length 5-50.
        let is_valid = combined.len() >= 5
            && combined.len() <= max_length
            && combined
                .chars()
                .next()
                .map_or(false, |c| c.is_ascii_alphabetic())
            && combined
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !combined.contains("--");

        if is_valid {
            combined
        } else {
            // Create a hash-based name that fits within Azure's constraints
            let mut hasher = DefaultHasher::new();
            repo_name.hash(&mut hasher);
            suffix.hash(&mut hasher);
            let hash = hasher.finish();

            // Create a name that starts with a letter and includes the hash
            format!("r{:x}-{}", hash, suffix)
        }
    }
}

impl Binding for AcrArtifactRegistry {}

#[async_trait]
impl ArtifactRegistry for AcrArtifactRegistry {
    fn registry_endpoint(&self) -> String {
        format!("https://{}", self.registry_endpoint)
    }

    fn upstream_repository_prefix(&self) -> String {
        self.repository_prefix.clone()
    }

    async fn create_repository(&self, repo_name: &str) -> Result<RepositoryResponse> {
        // ACR repositories are created implicitly on first push.
        // The ACR resource itself is provisioned by alien-infra.
        let repository_uri = format!("{}/{}", self.registry_endpoint, repo_name);

        Ok(RepositoryResponse {
            name: repo_name.to_string(),
            uri: Some(repository_uri),
            created_at: None,
        })
    }

    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse> {
        // ACR repositories are implicit — return the routable name and URI.
        let repository_uri = format!("{}/{}", self.registry_endpoint, repo_id);

        Ok(RepositoryResponse {
            name: repo_id.to_string(),
            uri: Some(repository_uri),
            created_at: None,
        })
    }

    async fn add_cross_account_access(
        &self,
        repo_id: &str,
        _access: CrossAccountAccess,
    ) -> Result<()> {
        let repo_name = repo_id;

        info!(
            repo_name = %repo_name,
            registry_name = %self.registry_name,
            "Azure Container Registry cross-account access not supported"
        );

        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "add_cross_account_access".to_string(),
            reason: "Azure Container Registry uses token-based access via generate_credentials - cross-account permissions are not supported".to_string(),
        }))
    }

    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        _access: CrossAccountAccess,
    ) -> Result<()> {
        let repo_name = repo_id;

        info!(
            repo_name = %repo_name,
            registry_name = %self.registry_name,
            "Azure Container Registry cross-account access not supported"
        );

        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "remove_cross_account_access".to_string(),
            reason: "Azure Container Registry uses token-based access via generate_credentials - cross-account permissions are not supported".to_string(),
        }))
    }

    async fn get_cross_account_access(&self, repo_id: &str) -> Result<CrossAccountPermissions> {
        let repo_name = repo_id;

        info!(
            repo_name = %repo_name,
            registry_name = %self.registry_name,
            "Azure Container Registry cross-account access not supported"
        );

        Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "get_cross_account_access".to_string(),
            reason: "Azure Container Registry uses token-based access via generate_credentials - cross-account permissions are not supported".to_string(),
        }))
    }

    async fn generate_credentials(
        &self,
        repo_id: &str,
        permissions: ArtifactRegistryPermissions,
        _ttl_seconds: Option<u32>,
    ) -> Result<ArtifactRegistryCredentials> {
        info!(
            registry = %self.registry_endpoint,
            repo_id = %repo_id,
            permissions = ?permissions,
            "Generating ACR credentials via AAD → refresh → access token flow"
        );

        // Step 1: Get an AAD access token for the management API.
        let aad_token = self
            .azure_token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await
            .map_err(|e| {
                map_cloud_client_error(e, "Failed to get AAD token for ACR".to_string(), None)
            })?;

        // Step 2: Exchange AAD token for an ACR refresh token.
        // See: https://github.com/Azure/acr/blob/main/docs/AAD-OAuth.md
        let exchange_url = format!("https://{}/oauth2/exchange", self.registry_endpoint);
        let exchange_resp = self
            .http_client
            .post(&exchange_url)
            .form(&[
                ("grant_type", "access_token"),
                ("service", &self.registry_endpoint),
                ("access_token", &aad_token),
            ])
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: "ACR OAuth2 exchange request failed".to_string(),
            })?;

        if !exchange_resp.status().is_success() {
            let status = exchange_resp.status();
            let body = exchange_resp.text().await.unwrap_or_default();
            return Err(AlienError::new(ErrorData::Other {
                message: format!("ACR OAuth2 exchange failed with {}: {}", status, body),
            }));
        }

        #[derive(serde::Deserialize)]
        struct ExchangeResponse {
            refresh_token: String,
        }
        let refresh_token = exchange_resp
            .json::<ExchangeResponse>()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse ACR exchange response".to_string(),
            })?
            .refresh_token;

        // Step 3: Exchange refresh token for a scoped access token.
        // The access token is what ACR's /v2/ API accepts as Bearer auth.
        // Scope: "repository:{repo}:pull,push" or "repository:{repo}:pull"
        let scope = if repo_id.is_empty() {
            // No specific repo — request registry-wide catalog access
            "registry:catalog:*".to_string()
        } else {
            let actions = match permissions {
                ArtifactRegistryPermissions::Pull => "pull",
                ArtifactRegistryPermissions::PushPull => "pull,push",
            };
            format!("repository:{}:{}", repo_id, actions)
        };

        let token_url = format!("https://{}/oauth2/token", self.registry_endpoint);
        let token_resp = self
            .http_client
            .post(&token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("service", &self.registry_endpoint),
                ("scope", &scope),
                ("refresh_token", &refresh_token),
            ])
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: "ACR OAuth2 token request failed".to_string(),
            })?;

        if !token_resp.status().is_success() {
            let status = token_resp.status();
            let body = token_resp.text().await.unwrap_or_default();
            return Err(AlienError::new(ErrorData::Other {
                message: format!("ACR OAuth2 token failed with {}: {}", status, body),
            }));
        }

        #[derive(serde::Deserialize)]
        struct TokenResponse {
            access_token: String,
        }
        let access_token = token_resp
            .json::<TokenResponse>()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to parse ACR token response".to_string(),
            })?
            .access_token;

        info!(
            registry = %self.registry_endpoint,
            scope = %scope,
            "ACR access token generated"
        );

        // ACR OAuth2 access tokens expire in ~5 minutes
        let expires_at = Some(
            (chrono::Utc::now() + chrono::Duration::seconds(300)).to_rfc3339(),
        );

        Ok(ArtifactRegistryCredentials {
            auth_method: RegistryAuthMethod::Bearer,
            username: String::new(),
            password: access_token,
            expires_at,
        })
    }

    // No-op: generate_credentials() uses the stateless AAD → refresh → access token
    // OAuth2 flow. No persistent resources (scope maps, tokens) are created, so
    // there is nothing to clean up.

    async fn delete_repository(&self, _repo_id: &str) -> Result<()> {
        // ACR repositories are implicit (created on push). Nothing to delete.
        Ok(())
    }
}
