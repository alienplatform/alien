use crate::{
    error::{ErrorData, Result},
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions, Binding,
        CrossAccountAccess, CrossAccountPermissions, RegistryAuthMethod, RepositoryResponse,
    },
};
use std::path::PathBuf;
use std::sync::Arc;

use alien_core::bindings::ArtifactRegistryBinding;
use alien_core::{AzureClientConfig, AzureCredentials};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use azure_core::{
    cloud::{CloudConfiguration, CustomConfiguration},
    credentials::{AccessToken, Secret, TokenCredential, TokenRequestOptions},
    http::ClientOptions,
    time::{Duration as AzureDuration, OffsetDateTime},
};
use azure_identity::{
    ClientAssertionCredentialOptions, ClientSecretCredential, ClientSecretCredentialOptions,
    ManagedIdentityCredential, ManagedIdentityCredentialOptions, UserAssignedId,
    WorkloadIdentityCredential, WorkloadIdentityCredentialOptions,
};
use tracing::info;

const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

/// Azure Container Registry implementation of the ArtifactRegistry binding.
#[derive(Debug)]
pub struct AcrArtifactRegistry {
    registry_name: String,
    registry_endpoint: String,
    repository_prefix: String,
    /// Azure credentials for direct registry access (AAD token exchange).
    credential: Arc<dyn TokenCredential>,
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
        let credential = azure_credential_from_config(azure_config)?;

        let repository_prefix = match config.repository_prefix {
            Some(bv) => bv
                .into_value(&binding_name, "repository_prefix")
                .unwrap_or_default(),
            None => String::new(),
        };

        Ok(Self {
            registry_name,
            registry_endpoint,
            repository_prefix,
            credential,
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
            .credential
            .get_token(&[MANAGEMENT_SCOPE], None)
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.acr".to_string(),
                reason: "Failed to get Azure management bearer token for ACR".to_string(),
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
                ("access_token", aad_token.token.secret()),
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

#[derive(Debug)]
struct StaticAzureAccessTokenCredential {
    token: String,
}

#[async_trait]
impl TokenCredential for StaticAzureAccessTokenCredential {
    async fn get_token(
        &self,
        scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> azure_core::Result<AccessToken> {
        if scopes.is_empty() {
            return Err(azure_core::Error::with_message(
                azure_core::error::ErrorKind::Credential,
                "no scopes specified",
            ));
        }

        Ok(AccessToken::new(
            self.token.clone(),
            OffsetDateTime::now_utc() + AzureDuration::days(365),
        ))
    }
}

fn azure_credential_from_config(config: &AzureClientConfig) -> Result<Arc<dyn TokenCredential>> {
    match &config.credentials {
        AzureCredentials::AccessToken { token } => Ok(Arc::new(StaticAzureAccessTokenCredential {
            token: token.clone(),
        })),
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => ClientSecretCredential::new(
            &config.tenant_id,
            client_id.clone(),
            Secret::new(client_secret.clone()),
            Some(ClientSecretCredentialOptions {
                client_options: azure_client_options(None),
            }),
        )
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "artifactRegistry.acr".to_string(),
            reason: "Failed to build official Azure service principal credentials".to_string(),
        }),
        AzureCredentials::WorkloadIdentity {
            client_id,
            tenant_id,
            federated_token_file,
            authority_host,
        } => WorkloadIdentityCredential::new(Some(WorkloadIdentityCredentialOptions {
            credential_options: ClientAssertionCredentialOptions {
                client_options: azure_client_options(Some(authority_host)),
            },
            client_id: Some(client_id.clone()),
            tenant_id: Some(tenant_id.clone()),
            token_file_path: Some(PathBuf::from(federated_token_file)),
        }))
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "artifactRegistry.acr".to_string(),
            reason: "Failed to build official Azure workload identity credentials".to_string(),
        }),
        AzureCredentials::VmManagedIdentity {
            client_id,
            identity_endpoint,
        } => {
            if let Some(identity_endpoint) = identity_endpoint {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "artifactRegistry.acr".to_string(),
                    reason: format!(
                        "Official Azure ManagedIdentityCredential does not support per-config IMDS endpoint override '{}'; use the standard IMDS endpoint or provide an access token",
                        identity_endpoint
                    ),
                }));
            }

            ManagedIdentityCredential::new(Some(ManagedIdentityCredentialOptions {
                user_assigned_id: Some(UserAssignedId::ClientId(client_id.clone())),
                client_options: azure_client_options(None),
            }))
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "artifactRegistry.acr".to_string(),
                reason: "Failed to build official Azure VM managed identity credentials"
                    .to_string(),
            })
        }
        AzureCredentials::ManagedIdentity {
            client_id,
            identity_endpoint,
            ..
        } => Err(AlienError::new(ErrorData::BindingSetupFailed {
            binding_type: "artifactRegistry.acr".to_string(),
            reason: format!(
                "Official Azure ManagedIdentityCredential cannot be constructed from explicit App Service identity endpoint '{}' for client '{}'; use workload identity, VM managed identity, or provide an access token",
                identity_endpoint, client_id
            ),
        })),
    }
}

fn azure_client_options(authority_host: Option<&str>) -> ClientOptions {
    let cloud = authority_host.map(|authority_host| {
        let mut custom = CustomConfiguration::default();
        custom.authority_host = authority_host.to_string();
        Arc::new(CloudConfiguration::Custom(custom))
    });

    ClientOptions {
        cloud,
        ..Default::default()
    }
}
