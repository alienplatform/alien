use crate::{
    error::{map_cloud_client_error, Error, ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding,
        CrossAccountAccess, CrossAccountPermissions, RepositoryResponse,
    },
};
use alien_azure_clients::long_running_operation::{
    LongRunningOperationApi, LongRunningOperationClient, OperationResult,
};
use alien_azure_clients::models::containerregistry::{
    GenerateCredentialsParameters, GenerateCredentialsResult, ScopeMapProperties, TokenProperties,
    TokenPropertiesStatus,
};
use alien_azure_clients::{
    containerregistry::{
        AzureContainerRegistryClient, ContainerRegistryApi, RegistryOperationResult,
    },
    AzureClientConfig, AzureTokenCache,
};
use alien_core::bindings::{AcrArtifactRegistryBinding, ArtifactRegistryBinding};
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
            AzureContainerRegistryClient::new(client.clone(), AzureTokenCache::new(azure_config.clone()));
        let lro_client = LongRunningOperationClient::new(client, AzureTokenCache::new(azure_config.clone()));

        Ok(Self {
            acr_client,
            lro_client,
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
        _ttl_seconds: Option<u32>,
    ) -> Result<ArtifactRegistryCredentials> {
        let repo_name = repo_id;

        info!(
            repo_name = %repo_name,
            permissions = ?permissions,
            "Generating Azure Container Registry credentials via scope map + token"
        );

        // Deterministic names so repeated calls for the same repo reuse resources.
        let scope_map_name = self.make_azure_resource_name(repo_name, "pull-scope");
        let token_name = self.make_azure_resource_name(repo_name, "pull-token");

        // 1. Create (or update) a scope map with the requested permissions.
        let actions = match permissions {
            ArtifactRegistryPermissions::Pull => {
                vec![format!("repositories/{}/content/read", repo_name)]
            }
            ArtifactRegistryPermissions::PushPull => {
                vec![
                    format!("repositories/{}/content/read", repo_name),
                    format!("repositories/{}/content/write", repo_name),
                ]
            }
        };

        let scope_map_result = self
            .acr_client
            .create_scope_map(
                &self.resource_group_name,
                &self.registry_name,
                &scope_map_name,
                &ScopeMapProperties {
                    description: Some(format!(
                        "Alien auto-generated scope map for {}",
                        repo_name
                    )),
                    actions,
                    creation_date: None,
                    provisioning_state: None,
                    type_: None,
                },
            )
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to create ACR scope map for '{}'", repo_name),
                    Some(repo_name.to_string()),
                )
            })?;

        scope_map_result
            .wait_for_operation_completion(&self.lro_client, "CreateScopeMap", &scope_map_name)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed waiting for ACR scope map creation for '{}'", repo_name),
                    Some(repo_name.to_string()),
                )
            })?;

        // Get the scope map ID.
        let scope_map = self
            .acr_client
            .get_scope_map(&self.resource_group_name, &self.registry_name, &scope_map_name)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to get ACR scope map for '{}'", repo_name),
                    Some(repo_name.to_string()),
                )
            })?;
        let scope_map_id = scope_map.id.ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_credentials".to_string(),
                reason: "Scope map missing ID after creation".to_string(),
            })
        })?;

        // 2. Create (or update) a token linked to the scope map.
        let token_result = self
            .acr_client
            .create_token(
                &self.resource_group_name,
                &self.registry_name,
                &token_name,
                &TokenProperties {
                    scope_map_id: Some(scope_map_id),
                    status: Some(TokenPropertiesStatus::Enabled),
                    credentials: None,
                    creation_date: None,
                    provisioning_state: None,
                },
            )
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to create ACR token for '{}'", repo_name),
                    Some(repo_name.to_string()),
                )
            })?;

        token_result
            .wait_for_operation_completion(&self.lro_client, "CreateToken", &token_name)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed waiting for ACR token creation for '{}'", repo_name),
                    Some(repo_name.to_string()),
                )
            })?;

        // Get the token to retrieve its resource ID.
        let token = self
            .acr_client
            .get_token(&self.resource_group_name, &self.registry_name, &token_name)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to get ACR token for '{}'", repo_name),
                    Some(repo_name.to_string()),
                )
            })?;
        let token_id = token.id.ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_credentials".to_string(),
                reason: "Token missing ID after creation".to_string(),
            })
        })?;

        // 3. Generate credentials (username + password) for the token.
        let cred_op = self
            .acr_client
            .generate_credentials(
                &self.resource_group_name,
                &self.registry_name,
                &GenerateCredentialsParameters {
                    token_id: Some(token_id),
                    expiry: None,
                    name: None,
                },
            )
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to generate ACR credentials for '{}'", repo_name),
                    Some(repo_name.to_string()),
                )
            })?;

        // Extract credentials from the operation result.
        // Azure only returns the password in the generateCredentials response —
        // subsequent GETs on the token do NOT include the password value.
        let cred_result = match cred_op {
            OperationResult::Completed(result) => result,
            OperationResult::LongRunning(lro) => {
                self.lro_client
                    .wait_for_completion(&lro, "GenerateCredentials", &token_name)
                    .await
                    .map_err(|e| {
                        map_cloud_client_error(
                            e,
                            format!(
                                "Failed waiting for ACR credential generation for '{}'",
                                repo_name
                            ),
                            Some(repo_name.to_string()),
                        )
                    })?;

                self.lro_client
                    .fetch_location_result::<GenerateCredentialsResult>(
                        &lro,
                        "GenerateCredentials",
                        &token_name,
                    )
                    .await
                    .map_err(|e| {
                        map_cloud_client_error(
                            e,
                            format!(
                                "Failed to fetch ACR credential result for '{}'",
                                repo_name
                            ),
                            Some(repo_name.to_string()),
                        )
                    })?
            }
        };

        let password = cred_result
            .passwords
            .into_iter()
            .find_map(|p| p.value)
            .ok_or_else(|| {
                AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_credentials".to_string(),
                    reason: "generateCredentials returned no password value".to_string(),
                })
            })?;

        info!(
            repo_name = %repo_name,
            token = %token_name,
            "ACR pull credentials generated successfully"
        );

        Ok(ArtifactRegistryCredentials {
            username: token_name,
            password,
            expires_at: None,
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
