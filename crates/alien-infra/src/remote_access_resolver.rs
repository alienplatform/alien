//! Remote access resolver for establishing connections to cloud platforms
//!
//! This module provides functionality to resolve stack state into authenticated client
//! configurations by performing impersonation based on RemoteStackManagement outputs.

use crate::error::{ErrorData, Result};
use crate::ClientConfigExt as _;
#[cfg(feature = "aws")]
use alien_aws_clients::AwsImpersonationConfig;
use alien_core::{
    ClientConfig, ImpersonationConfig, Platform, RemoteStackManagement,
    RemoteStackManagementOutputs, ResourceOutputsDefinition, StackState,
};
use alien_error::{AlienError, Context};
#[cfg(feature = "gcp")]
use alien_gcp_clients::GcpImpersonationConfig;
use std::collections::HashMap;
use tracing::info;
use uuid::Uuid;

/// Service for resolving stack state into authenticated client configurations
#[derive(Debug)]
pub struct RemoteAccessResolver {
    /// Environment variables to use for platform configuration
    env: HashMap<String, String>,
}

impl RemoteAccessResolver {
    /// Create a new remote access resolver with a specific environment
    pub fn new(env: HashMap<String, String>) -> Self {
        Self { env }
    }

    /// Resolve stack state into an authenticated client configuration by impersonating
    /// the RemoteStackManagement resource identity.
    ///
    /// This method extracts the RemoteStackManagement outputs from the stack state and
    /// uses them to configure impersonation from the base configuration (Agent Manager's
    /// ServiceAccount) to the startup cloud identity.
    ///
    /// # Arguments
    ///
    /// * `base_config` - The Agent Manager's ServiceAccount configuration
    /// * `stack_state` - The stack state containing RemoteStackManagement outputs
    ///
    /// # Returns
    ///
    /// An authenticated client configuration that can be used to access the agent's cloud environment
    pub async fn resolve(
        &self,
        base_config: ClientConfig,
        stack_state: &StackState,
    ) -> Result<ClientConfig> {
        // Find RemoteStackManagement resource outputs
        let remote_mgmt_outputs = self.find_remote_stack_management_outputs(stack_state)?;

        // Determine platform and perform appropriate impersonation
        match base_config.platform() {
            Platform::Aws => {
                self.resolve_aws_impersonation(base_config, &remote_mgmt_outputs)
                    .await
            }
            Platform::Gcp => {
                self.resolve_gcp_impersonation(base_config, &remote_mgmt_outputs)
                    .await
            }
            Platform::Azure => {
                self.resolve_azure_impersonation(base_config, &remote_mgmt_outputs)
                    .await
            }
            _ => Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                message: format!(
                    "{:?} platform does not support remote access impersonation",
                    base_config.platform()
                ),
                field_name: Some("platform".to_string()),
            })),
        }
    }

    /// Find RemoteStackManagement outputs in the stack state
    fn find_remote_stack_management_outputs(
        &self,
        stack_state: &StackState,
    ) -> Result<RemoteStackManagementOutputs> {
        // Look for RemoteStackManagement resource in the stack state
        for (_resource_id, resource_state) in &stack_state.resources {
            if resource_state.resource_type == RemoteStackManagement::RESOURCE_TYPE.to_string() {
                if let Some(outputs) = &resource_state.outputs {
                    // Try to downcast to RemoteStackManagementOutputs
                    if let Some(remote_mgmt_outputs) =
                        outputs.downcast_ref::<RemoteStackManagementOutputs>()
                    {
                        return Ok(remote_mgmt_outputs.clone());
                    }
                }
            }
        }

        Err(AlienError::new(ErrorData::InfrastructureError {
            message: "RemoteStackManagement resource not found in stack state or missing outputs"
                .to_string(),
            operation: Some("find_remote_stack_management".to_string()),
            resource_id: None,
        }))
    }

    /// Resolve AWS impersonation using RemoteStackManagement outputs
    async fn resolve_aws_impersonation(
        &self,
        base_config: ClientConfig,
        outputs: &RemoteStackManagementOutputs,
    ) -> Result<ClientConfig> {
        let role_arn = &outputs.access_configuration;
        info!("Resolving AWS impersonation for role: {}", role_arn);

        // Create impersonation config
        let impersonation_config = ImpersonationConfig::Aws(AwsImpersonationConfig {
            role_arn: role_arn.clone(),
            session_name: Some(format!("alien-remote-access-{}", Uuid::new_v4().simple())),
            duration_seconds: Some(3600),
            external_id: None, // External ID is configured in the trust policy, not needed here
        });

        // Perform impersonation
        base_config.impersonate(impersonation_config).await.context(
            ErrorData::AuthenticationFailed {
                message: format!("Failed to assume AWS role: {}", role_arn),
                method: Some("role_assumption".to_string()),
            },
        )
    }

    /// Resolve GCP impersonation using RemoteStackManagement outputs
    async fn resolve_gcp_impersonation(
        &self,
        base_config: ClientConfig,
        outputs: &RemoteStackManagementOutputs,
    ) -> Result<ClientConfig> {
        let service_account_email = &outputs.access_configuration;
        info!(
            "Resolving GCP impersonation for service account: {}",
            service_account_email
        );

        // Create impersonation config
        let impersonation_config = ImpersonationConfig::Gcp(GcpImpersonationConfig {
            service_account_email: service_account_email.clone(),
            scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
            delegates: None,
            lifetime: Some("3600s".to_string()),
        });

        // Perform impersonation
        base_config.impersonate(impersonation_config).await.context(
            ErrorData::AuthenticationFailed {
                message: format!(
                    "Failed to impersonate GCP service account: {}",
                    service_account_email
                ),
                method: Some("service_account_impersonation".to_string()),
            },
        )
    }

    /// Resolve Azure impersonation using RemoteStackManagement outputs
    async fn resolve_azure_impersonation(
        &self,
        base_config: ClientConfig,
        outputs: &RemoteStackManagementOutputs,
    ) -> Result<ClientConfig> {
        // For Azure Lighthouse, the access_configuration contains the registration assignment ID
        // We don't need to do additional impersonation as Lighthouse handles the delegation
        info!(
            "Resolving Azure Lighthouse access with registration: {}",
            outputs.access_configuration
        );

        // Azure Lighthouse works differently - once the registration is set up,
        // the managing tenant's identity automatically has access
        // So we just return the base config as-is
        Ok(base_config)
    }

    /// Create a base client configuration for the specified platform
    ///
    /// This is a convenience method to create a base configuration from environment variables
    /// that can then be used with `resolve()` to establish remote access.
    pub async fn create_base_config(&self, platform: Platform) -> Result<ClientConfig> {
        info!("Creating base client config for platform: {}", platform);

        ClientConfig::from_env(platform, &self.env)
            .await
            .context(ErrorData::ClientConfigInvalid {
                platform,
                message: "Failed to load platform configuration from environment".to_string(),
            })
    }
}

impl Default for RemoteAccessResolver {
    fn default() -> Self {
        Self::new(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_access_resolver_creation() {
        let resolver = RemoteAccessResolver::new(HashMap::new());
        assert!(resolver.env.is_empty());

        let mut env = HashMap::new();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());
        let resolver = RemoteAccessResolver::new(env);
        assert_eq!(
            resolver.env.get("TEST_VAR"),
            Some(&"test_value".to_string())
        );
    }

    #[test]
    fn test_default_resolver() {
        let resolver = RemoteAccessResolver::default();
        assert!(resolver.env.is_empty());
    }
}
