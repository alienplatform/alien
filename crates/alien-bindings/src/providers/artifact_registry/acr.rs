use crate::{
    error::{map_cloud_client_error, ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding,
        CrossAccountAccess, CrossAccountPermissions, RegistryAuthMethod, RepositoryResponse,
    },
};
use alien_azure_clients::{AzureClientConfig, AzureTokenCache};
use alien_core::bindings::ArtifactRegistryBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use tracing::info;

/// Azure Container Registry implementation of the ArtifactRegistry binding.
#[derive(Debug)]
pub struct AcrArtifactRegistry {
    registry_name: String,
    registry_endpoint: String,
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

        config
            .resource_group_name
            .into_value(&binding_name, "resource_group_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract resource_group_name from binding".to_string(),
            })?;

        // Derive registry endpoint from registry name
        let registry_endpoint = format!("{}.azurecr.io", registry_name);
        let client = crate::http_client::create_http_client();
        let azure_token_cache = AzureTokenCache::new(azure_config.clone());

        let repository_prefix = match config.repository_prefix {
            Some(bv) => bv.into_value(&binding_name, "repository_prefix").context(
                ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Failed to extract repository_prefix from binding".to_string(),
                },
            )?,
            None => String::new(),
        };

        Ok(Self {
            registry_name,
            registry_endpoint,
            repository_prefix,
            azure_token_cache,
            http_client: client,
        })
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
        let expires_at = Some((chrono::Utc::now() + chrono::Duration::seconds(300)).to_rfc3339());

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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::{AcrArtifactRegistryBinding, ArtifactRegistryBinding, BindingValue};
    use alien_core::AzureCredentials;

    fn test_config() -> AzureClientConfig {
        AzureClientConfig {
            subscription_id: "sub-test".to_string(),
            tenant_id: "tenant-test".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "token-test".to_string(),
            },
            service_overrides: None,
        }
    }

    #[tokio::test]
    async fn new_fails_when_configured_repository_prefix_cannot_be_resolved() {
        let binding = ArtifactRegistryBinding::Acr(AcrArtifactRegistryBinding {
            registry_name: BindingValue::Value("registrytest".to_string()),
            resource_group_name: BindingValue::Value("rg-test".to_string()),
            repository_prefix: Some(BindingValue::expression(serde_json::json!({
                "ref": "repositoryPrefix"
            }))),
        });

        let result =
            AcrArtifactRegistry::new("artifact-registry".to_string(), binding, &test_config())
                .await;
        let Err(error) = result else {
            panic!("configured repository_prefix resolution failure should fail initialization");
        };

        assert!(error
            .to_string()
            .contains("Failed to extract repository_prefix from binding"));
    }

    #[tokio::test]
    async fn new_uses_empty_repository_prefix_when_repository_prefix_is_omitted() {
        let binding = ArtifactRegistryBinding::Acr(AcrArtifactRegistryBinding {
            registry_name: BindingValue::Value("registrytest".to_string()),
            resource_group_name: BindingValue::Value("rg-test".to_string()),
            repository_prefix: None,
        });

        let registry =
            AcrArtifactRegistry::new("artifact-registry".to_string(), binding, &test_config())
                .await
                .expect("omitted repository_prefix should initialize");

        assert_eq!(registry.upstream_repository_prefix(), "");
    }

    #[tokio::test]
    async fn new_uses_configured_repository_prefix() {
        let binding = ArtifactRegistryBinding::Acr(AcrArtifactRegistryBinding {
            registry_name: BindingValue::Value("registrytest".to_string()),
            resource_group_name: BindingValue::Value("rg-test".to_string()),
            repository_prefix: Some(BindingValue::Value("team-a".to_string())),
        });

        let registry =
            AcrArtifactRegistry::new("artifact-registry".to_string(), binding, &test_config())
                .await
                .expect("configured repository_prefix should initialize");

        assert_eq!(registry.upstream_repository_prefix(), "team-a");
    }
}
