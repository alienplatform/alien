use crate::{
    error::{map_cloud_client_error, Error, ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding,
        CrossAccountAccess, CrossAccountPermissions, RepositoryResponse,
    },
};
use alien_azure_clients::long_running_operation::OperationResult;
use alien_azure_clients::models::containerregistry::{
    ScopeMapProperties, TokenProperties, TokenPropertiesStatus,
};
use alien_azure_clients::{
    containerregistry::{
        AzureContainerRegistryClient, ContainerRegistryApi, RegistryOperationResult,
    },
    AzureClientConfig, AzureTokenCache,
};
use alien_core::bindings::{AcrArtifactRegistryBinding, ArtifactRegistryBinding};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use chrono;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{info, warn};

/// Azure Container Registry implementation of the ArtifactRegistry binding.
#[derive(Debug)]
pub struct AcrArtifactRegistry {
    acr_client: AzureContainerRegistryClient,
    binding_name: String,
    registry_name: String,
    registry_endpoint: String,
    resource_group_name: String,
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
        let acr_client =
            AzureContainerRegistryClient::new(client, AzureTokenCache::new(azure_config.clone()));

        Ok(Self {
            acr_client,
            binding_name,
            registry_name,
            registry_endpoint,
            resource_group_name,
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

        let max_length = 49; // Leave room for potential additional characters
        let combined = format!("{}-{}", repo_name, suffix);

        if combined.len() <= max_length
            && combined
                .chars()
                .next()
                .map_or(false, |c| c.is_ascii_alphabetic())
        {
            // If the name is already valid, use it as-is
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
    async fn create_repository(&self, repo_name: &str) -> Result<RepositoryResponse> {
        info!(
            repo_name = %repo_name,
            registry_name = %self.registry_name,
            "Creating Azure Container Registry repository (via scope map)"
        );

        // In ACR, repositories are created implicitly on first push
        // However, we can create a scope map to control access to the repository
        let scope_map_name = self.make_azure_resource_name(repo_name, "scope");
        let actions = vec![
            format!("repositories/{}/content/read", repo_name),
            format!("repositories/{}/content/write", repo_name),
        ];

        let scope_map_properties = ScopeMapProperties {
            description: Some(format!("Scope map for repository {}", repo_name)),
            actions,
            creation_date: None,
            provisioning_state: None,
            type_: None,
        };

        match self
            .acr_client
            .create_scope_map(
                &self.resource_group_name,
                &self.registry_name,
                &scope_map_name,
                &scope_map_properties,
            )
            .await
        {
            Ok(operation_result) => {
                match operation_result {
                    OperationResult::Completed(_) => {
                        info!(
                            repo_name = %repo_name,
                            "Azure Container Registry repository scope map created successfully"
                        );

                        // Construct the repository URI for Azure Container Registry
                        let repository_uri = format!("{}/{}", self.registry_endpoint, repo_name);

                        Ok(RepositoryResponse {
                            name: repo_name.to_string(),
                            uri: Some(repository_uri),
                            created_at: None, // ACR doesn't provide creation time in this response
                        })
                    }
                    OperationResult::LongRunning(_) => {
                        info!(
                            repo_name = %repo_name,
                            "Azure Container Registry repository scope map creation is in progress"
                        );

                        Ok(RepositoryResponse {
                            name: repo_name.to_string(),
                            uri: None, // Will be available once creation completes
                            created_at: None,
                        })
                    }
                }
            }
            Err(e) => {
                warn!(
                    repo_name = %repo_name,
                    error = %e,
                    "Failed to create Azure Container Registry repository scope map"
                );

                Err(map_cloud_client_error(
                    e,
                    format!(
                        "Failed to create Azure Container Registry repository '{}'",
                        repo_name
                    ),
                    Some(repo_name.to_string()),
                ))
            }
        }
    }

    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse> {
        let repo_name = repo_id;
        let scope_map_name = self.make_azure_resource_name(repo_name, "scope");

        info!(
            repo_name = %repo_name,
            registry_name = %self.registry_name,
            "Getting Azure Container Registry repository details"
        );

        let scope_map = self
            .acr_client
            .get_scope_map(
                &self.resource_group_name,
                &self.registry_name,
                &scope_map_name,
            )
            .await
            .map_err(|_e| {
                warn!(
                    repo_name = %repo_name,
                    "Azure Container Registry repository not found"
                );

                AlienError::new(ErrorData::ResourceNotFound {
                    resource_id: repo_name.to_string(),
                })
            })?;

        // Construct the repository URI for Azure Container Registry
        let repository_uri = format!("{}/{}", self.registry_endpoint, repo_name);

        // Azure scope maps don't directly provide creation time
        let created_at = scope_map.properties.and_then(|props| props.creation_date);

        info!(
            repo_name = %repo_name,
            repo_uri = %repository_uri,
            "Azure Container Registry repository details retrieved"
        );

        Ok(RepositoryResponse {
            name: repo_name.to_string(),
            uri: Some(repository_uri),
            created_at,
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
        ttl_seconds: Option<u32>,
    ) -> Result<ArtifactRegistryCredentials> {
        let repo_name = repo_id;

        info!(
            repo_name = %repo_name,
            permissions = ?permissions,
            ttl_seconds = ?ttl_seconds,
            "Generating Azure Container Registry credentials using built-in tokens"
        );

        // For Azure Container Registry, we can use the admin credentials or registry tokens
        // The built-in token mechanism provides temporary access tokens

        // TODO: In a real implementation, we would:
        // 1. Use the Azure Container Registry token mechanism to generate a token with appropriate scope
        // 2. The scope would be determined by the permissions parameter (pull vs push+pull)
        // 3. Use the Azure authentication to create tokens for the registry

        // For now, we'll use a placeholder implementation that demonstrates the concept
        // In practice, this would involve calling Azure Container Registry APIs to generate tokens

        let scope = match permissions {
            ArtifactRegistryPermissions::Pull => "repository:*:pull",
            ArtifactRegistryPermissions::PushPull => "repository:*:pull,push",
        };

        warn!(
            scope = %scope,
            "Azure Container Registry credential generation using placeholder - should use ACR token APIs"
        );

        // In a real implementation, we would call the Azure Container Registry APIs
        // to generate a registry token with the appropriate scope and TTL
        // For now, return a placeholder response

        // Calculate expiration time
        let expires_at = if let Some(ttl) = ttl_seconds {
            Some((chrono::Utc::now() + chrono::Duration::seconds(ttl as i64)).to_rfc3339())
        } else {
            Some((chrono::Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339())
            // Default 1 hour
        };

        // For Azure Container Registry, we would typically return:
        // - username: the token name or service principal client ID
        // - password: the token password or service principal secret/access token

        // TODO: Replace this with actual Azure Container Registry token generation
        // This is a placeholder that shows the expected structure
        let permissions_str = match permissions {
            ArtifactRegistryPermissions::Pull => "pull",
            ArtifactRegistryPermissions::PushPull => "pushpull",
        };

        Ok(ArtifactRegistryCredentials {
            username: format!("alien-token-{}", permissions_str),
            password: format!("placeholder-token-for-{}-access", scope),
            expires_at,
        })
    }

    async fn delete_repository(&self, repo_id: &str) -> Result<()> {
        let repo_name = repo_id;
        let scope_map_name = self.make_azure_resource_name(repo_name, "scope");

        info!(
            repo_name = %repo_name,
            registry_name = %self.registry_name,
            "Deleting Azure Container Registry repository scope map"
        );

        // Delete the scope map associated with the repository
        match self
            .acr_client
            .delete_scope_map(
                &self.resource_group_name,
                &self.registry_name,
                &scope_map_name,
            )
            .await
        {
            Ok(_) => {
                info!(
                    repo_name = %repo_name,
                    "Azure Container Registry repository scope map deleted successfully"
                );

                // Also delete any associated tokens
                if let Ok(tokens) = self
                    .acr_client
                    .list_tokens(&self.resource_group_name, &self.registry_name)
                    .await
                {
                    // Since we use hashing for long names, check all tokens against possible names
                    for token in tokens {
                        if let Some(token_name) = &token.name {
                            // Check if this token matches any of the possible token names for this repo
                            for index in 0..10 {
                                // Check first 10 possible indices
                                let expected_token_name = self.make_azure_resource_name(
                                    repo_name,
                                    &format!("token-{}", index),
                                );
                                if token_name == &expected_token_name {
                                    let _ = self
                                        .acr_client
                                        .delete_token(
                                            &self.resource_group_name,
                                            &self.registry_name,
                                            token_name,
                                        )
                                        .await;
                                    break;
                                }
                            }
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                warn!(
                    repo_name = %repo_name,
                    error = %e,
                    "Failed to delete Azure Container Registry repository scope map"
                );

                Err(map_cloud_client_error(
                    e,
                    format!(
                        "Failed to delete Azure Container Registry repository '{}'",
                        repo_name
                    ),
                    Some(repo_name.to_string()),
                ))
            }
        }
    }
}
