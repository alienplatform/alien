use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

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
use azure_security_keyvault_secrets::{
    models::SetSecretParameters, SecretClient, SecretClientOptions,
};

use crate::error::{ErrorData, Result};

/// Azure Key Vault binding implementation.
pub struct AzureKeyVault {
    client: SecretClient,
    vault_base_url: String,
}

impl Debug for AzureKeyVault {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureKeyVault")
            .field("vault_base_url", &self.vault_base_url)
            .finish()
    }
}

impl AzureKeyVault {
    /// Create a new Azure Key Vault binding.
    pub fn new(azure_config: &AzureClientConfig, vault_name: String) -> Result<Self> {
        let vault_base_url = azure_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("keyvault"))
            .cloned()
            .unwrap_or_else(|| format!("https://{}.vault.azure.net", vault_name));
        let credential = azure_credential_from_config(azure_config)?;
        let client = SecretClient::new(
            &vault_base_url,
            credential,
            Some(SecretClientOptions {
                client_options: azure_client_options(None),
                ..Default::default()
            }),
        )
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "vault.keyVault".to_string(),
            reason: "Failed to build official Azure Key Vault client".to_string(),
        })?;

        Ok(Self {
            client,
            vault_base_url,
        })
    }

    /// Azure Key Vault secret names only allow alphanumerics and hyphens.
    /// Convert underscores to hyphens for compatibility.
    fn sanitize_secret_name(name: &str) -> String {
        name.replace('_', "-")
    }
}

#[async_trait]
impl crate::traits::Binding for AzureKeyVault {}

#[async_trait]
impl crate::traits::Vault for AzureKeyVault {
    /// Get a secret value by name.
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let sanitized = Self::sanitize_secret_name(secret_name);
        let response = self
            .client
            .get_secret(&sanitized, None)
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get secret '{}' from vault '{}'",
                    secret_name, self.vault_base_url
                ),
                resource_id: None,
            })?;
        let secret =
            response
                .into_model()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to parse secret '{}' from vault '{}'",
                        secret_name, self.vault_base_url
                    ),
                    resource_id: None,
                })?;

        secret.value.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Secret '{}' has no value", secret_name),
                resource_id: None,
            })
        })
    }

    /// Set a secret value.
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let sanitized = Self::sanitize_secret_name(secret_name);
        let parameters = SetSecretParameters {
            value: Some(value.to_string()),
            ..Default::default()
        };

        self.client
            .set_secret(
                &sanitized,
                parameters.try_into().into_alien_error().context(
                    ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to encode secret '{}' for vault '{}'",
                            secret_name, self.vault_base_url
                        ),
                        resource_id: None,
                    },
                )?,
                None,
            )
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to set secret '{}' in vault '{}'",
                    secret_name, self.vault_base_url
                ),
                resource_id: None,
            })?;

        Ok(())
    }

    /// Delete a secret.
    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        let sanitized = Self::sanitize_secret_name(secret_name);
        self.client
            .delete_secret(&sanitized, None)
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete secret '{}' from vault '{}'",
                    secret_name, self.vault_base_url
                ),
                resource_id: None,
            })?;

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
            binding_type: "vault.keyVault".to_string(),
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
            binding_type: "vault.keyVault".to_string(),
            reason: "Failed to build official Azure workload identity credentials".to_string(),
        }),
        AzureCredentials::VmManagedIdentity {
            client_id,
            identity_endpoint,
        } => {
            if let Some(identity_endpoint) = identity_endpoint {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "vault.keyVault".to_string(),
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
                binding_type: "vault.keyVault".to_string(),
                reason: "Failed to build official Azure VM managed identity credentials"
                    .to_string(),
            })
        }
        AzureCredentials::ManagedIdentity {
            client_id,
            identity_endpoint,
            ..
        } => Err(AlienError::new(ErrorData::BindingSetupFailed {
            binding_type: "vault.keyVault".to_string(),
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
