use crate::error::{ErrorData, Result};
use crate::traits::{
    AzureServiceAccountInfo, Binding, ImpersonationRequest, ServiceAccount, ServiceAccountInfo,
};
use alien_azure_clients::{AzureClientConfig, AzureClientConfigExt};
use alien_core::bindings::AzureServiceAccountBinding;
use alien_core::{AzureClientConfig as CoreAzureClientConfig, ClientConfig};
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

        // For Azure, "impersonation" means using a different managed identity that's already
        // attached to the workload. We do this by creating a new config with the target
        // identity's client_id, reusing the existing workload identity setup.
        //
        // When multiple UAMIs are attached to a Container App, Azure exposes the same
        // AZURE_TENANT_ID and AZURE_FEDERATED_TOKEN_FILE for all of them. The
        // AZURE_CLIENT_ID environment variable determines which identity to use when
        // requesting tokens. By overriding AZURE_CLIENT_ID and calling from_env(),
        // we effectively "switch" to the target identity.

        // Get current environment variables and override AZURE_CLIENT_ID
        let mut env_vars: HashMap<String, String> = std::env::vars().collect();

        // Override the client_id to select the target managed identity
        env_vars.insert("AZURE_CLIENT_ID".to_string(), client_id.clone());

        // For SP→SP impersonation: also swap client_secret so the Azure Identity SDK
        // authenticates as the management SP rather than the execution SP.
        // On-Azure (UAMI→UAMI): env var absent → current behavior unchanged.
        // Off-Azure (SP→SP): env var present → swaps both client_id and client_secret.
        if let Some(mgmt_secret) = env_vars.remove("ALIEN_AZURE_MANAGEMENT_CLIENT_SECRET") {
            env_vars.insert("AZURE_CLIENT_SECRET".to_string(), mgmt_secret);
        }

        // Use from_env to create the config - it will handle all the credential setup
        let impersonated_config =
            CoreAzureClientConfig::from_env(&env_vars)
                .await
                .context(ErrorData::Other {
                    message: format!(
                        "Failed to create Azure config for impersonation with client_id: {}",
                        client_id
                    ),
                })?;

        Ok(ClientConfig::Azure(Box::new(impersonated_config)))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
