use crate::error::{ErrorData, Result};
use crate::traits::{
    AzureServiceAccountInfo, Binding, ImpersonationRequest, ServiceAccount, ServiceAccountInfo,
};
use alien_core::bindings::AzureServiceAccountBinding;
use alien_core::{AzureClientConfig, AzureCredentials, ClientConfig};
use alien_error::Context;
use async_trait::async_trait;
use std::collections::HashMap;

/// Azure User-Assigned Managed Identity service account binding implementation
///
/// Note: Azure impersonation works differently than AWS/GCP. The managed identity
/// must already be attached to the workload (Container App, VM, etc.) at provisioning time.
/// This binding allows selecting which attached identity to use at runtime by providing
/// its client_id to the Azure Identity SDK.
#[derive(Debug)]
pub struct AzureManagedIdentityServiceAccount {
    config: AzureClientConfig,
    binding: AzureServiceAccountBinding,
}

impl AzureManagedIdentityServiceAccount {
    pub fn new(config: AzureClientConfig, binding: AzureServiceAccountBinding) -> Self {
        Self { config, binding }
    }

    /// Get the client ID from the binding, resolving template expressions if needed
    fn get_client_id(&self) -> Result<String> {
        self.binding
            .client_id
            .clone()
            .into_value("service-account", "client_id")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "service-account".to_string(),
                reason: "Failed to resolve client_id from binding".to_string(),
            })
    }

    /// Get the resource ID from the binding, resolving template expressions if needed
    fn get_resource_id(&self) -> Result<String> {
        self.binding
            .resource_id
            .clone()
            .into_value("service-account", "resource_id")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "service-account".to_string(),
                reason: "Failed to resolve resource_id from binding".to_string(),
            })
    }

    /// Get the principal ID from the binding, resolving template expressions if needed
    fn get_principal_id(&self) -> Result<String> {
        self.binding
            .principal_id
            .clone()
            .into_value("service-account", "principal_id")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "service-account".to_string(),
                reason: "Failed to resolve principal_id from binding".to_string(),
            })
    }
}

impl Binding for AzureManagedIdentityServiceAccount {}

#[async_trait]
impl ServiceAccount for AzureManagedIdentityServiceAccount {
    async fn get_info(&self) -> Result<ServiceAccountInfo> {
        let client_id = self.get_client_id()?;
        let resource_id = self.get_resource_id()?;
        let principal_id = self.get_principal_id()?;

        Ok(ServiceAccountInfo::Azure(AzureServiceAccountInfo {
            client_id,
            resource_id,
            principal_id,
        }))
    }

    async fn impersonate(&self, _request: ImpersonationRequest) -> Result<ClientConfig> {
        let client_id = self.get_client_id()?;

        let env_vars = std::env::vars().collect::<HashMap<_, _>>();
        let tenant_id = env_vars
            .get("AZURE_TENANT_ID")
            .cloned()
            .unwrap_or_else(|| self.config.tenant_id.clone());

        let credentials = if let Some(federated_token_file) =
            env_vars.get("AZURE_FEDERATED_TOKEN_FILE")
        {
            AzureCredentials::WorkloadIdentity {
                client_id: client_id.clone(),
                tenant_id: tenant_id.clone(),
                federated_token_file: federated_token_file.clone(),
                authority_host: env_vars
                    .get("AZURE_AUTHORITY_HOST")
                    .cloned()
                    .unwrap_or_else(|| "https://login.microsoftonline.com/".to_string()),
            }
        } else if let (Some(identity_endpoint), Some(identity_header)) = (
            env_vars.get("IDENTITY_ENDPOINT"),
            env_vars.get("IDENTITY_HEADER"),
        ) {
            AzureCredentials::ManagedIdentity {
                client_id: client_id.clone(),
                identity_endpoint: identity_endpoint.clone(),
                identity_header: identity_header.clone(),
            }
        } else {
            return Err(alien_error::AlienError::new(ErrorData::Other {
                    message: "Azure managed identity impersonation requires workload identity (AZURE_FEDERATED_TOKEN_FILE) or managed identity (IDENTITY_ENDPOINT and IDENTITY_HEADER) credentials".to_string(),
                }));
        };

        let impersonated_config = AzureClientConfig {
            subscription_id: self.config.subscription_id.clone(),
            tenant_id,
            region: self.config.region.clone(),
            credentials,
            service_overrides: self.config.service_overrides.clone(),
        };

        Ok(ClientConfig::Azure(Box::new(impersonated_config)))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
