//! Remote access resolver for establishing connections to cloud platforms
//!
//! This module provides functionality to resolve stack state into authenticated client
//! configurations by performing impersonation based on RemoteStackManagement outputs.

use crate::error::{ErrorData, Result};
use crate::ClientConfigExt as _;
#[cfg(feature = "aws")]
use alien_aws_clients::AwsImpersonationConfig;
use alien_core::{
    ClientConfig, EnvironmentInfo, ImpersonationConfig, Platform, RemoteStackManagement,
    RemoteStackManagementOutputs, ResourceOutputsDefinition, StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
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
    /// uses them to configure impersonation from the base configuration (Manager's
    /// ServiceAccount) to the startup cloud identity.
    ///
    /// # Arguments
    ///
    /// * `base_config` - The Manager's ServiceAccount configuration
    /// * `stack_state` - The stack state containing RemoteStackManagement outputs
    /// * `target_environment` - Optional target environment info. When provided, the
    ///   impersonated config will use the target's region/account/project instead of
    ///   inheriting from the management configuration.
    ///
    /// # Returns
    ///
    /// An authenticated client configuration that can be used to access the agent's cloud environment
    pub async fn resolve(
        &self,
        base_config: ClientConfig,
        stack_state: &StackState,
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        // Find RemoteStackManagement resource outputs
        let remote_mgmt_outputs = self.find_remote_stack_management_outputs(stack_state)?;

        // Determine platform and perform appropriate impersonation
        match base_config.platform() {
            Platform::Aws => {
                self.resolve_aws_impersonation(
                    base_config,
                    &remote_mgmt_outputs,
                    target_environment,
                )
                .await
            }
            Platform::Gcp => {
                self.resolve_gcp_impersonation(
                    base_config,
                    &remote_mgmt_outputs,
                    target_environment,
                )
                .await
            }
            Platform::Azure => {
                self.resolve_azure_impersonation(
                    base_config,
                    &remote_mgmt_outputs,
                    target_environment,
                )
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
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        let role_arn = &outputs.access_configuration;
        info!("Resolving AWS impersonation for role: {}", role_arn);

        // Extract target region from environment info if available.
        let target_region = target_environment.and_then(|env| match env {
            EnvironmentInfo::Aws(info) => Some(info.region.clone()),
            _ => None,
        });

        let impersonation_config = ImpersonationConfig::Aws(AwsImpersonationConfig {
            role_arn: role_arn.clone(),
            session_name: Some(format!("alien-remote-access-{}", Uuid::new_v4().simple())),
            duration_seconds: Some(3600),
            external_id: None,
            target_region,
        });

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
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        let service_account_email = &outputs.access_configuration;
        info!(
            "Resolving GCP impersonation for service account: {}",
            service_account_email
        );

        // Extract target project/region from environment info if available.
        let (target_project_id, target_region) = match target_environment {
            Some(EnvironmentInfo::Gcp(info)) => {
                (Some(info.project_id.clone()), Some(info.region.clone()))
            }
            _ => (None, None),
        };

        let impersonation_config = ImpersonationConfig::Gcp(GcpImpersonationConfig {
            service_account_email: service_account_email.clone(),
            scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
            delegates: None,
            lifetime: Some("3600s".to_string()),
            target_project_id,
            target_region,
        });

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

    /// Resolve Azure impersonation using OIDC token exchange or SP cross-tenant credentials.
    ///
    /// The access_configuration from RSM outputs is JSON:
    ///   { "uamiClientId": "<client-id>", "tenantId": "<customer-tenant-id>" }
    ///
    /// Primary path (production/CI): reads OIDC token from AZURE_FEDERATED_TOKEN_FILE,
    /// exchanges it for an ARM token via the customer's Azure AD token endpoint.
    ///
    /// Fallback path (local dev): uses ServicePrincipal client_credentials grant
    /// against the customer's tenant.
    async fn resolve_azure_impersonation(
        &self,
        base_config: ClientConfig,
        outputs: &RemoteStackManagementOutputs,
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        let access_config: serde_json::Value = serde_json::from_str(&outputs.access_configuration)
            .into_alien_error()
            .context(ErrorData::RemoteAccessInvalid {
                message: "Failed to parse Azure access configuration JSON".to_string(),
                field_name: Some("access_configuration".to_string()),
            })?;

        let uami_client_id = access_config["uamiClientId"].as_str().ok_or_else(|| {
            AlienError::new(ErrorData::RemoteAccessInvalid {
                message: "Azure access configuration missing 'uamiClientId'".to_string(),
                field_name: Some("uamiClientId".to_string()),
            })
        })?;

        let customer_tenant_id = access_config["tenantId"].as_str().ok_or_else(|| {
            AlienError::new(ErrorData::RemoteAccessInvalid {
                message: "Azure access configuration missing 'tenantId'".to_string(),
                field_name: Some("tenantId".to_string()),
            })
        })?;

        let token_url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            customer_tenant_id
        );

        // Extract target subscription/region from environment info
        let (target_subscription, target_region) = match target_environment {
            Some(EnvironmentInfo::Azure(info)) => {
                (info.subscription_id.clone(), Some(info.location.clone()))
            }
            _ => match &base_config {
                ClientConfig::Azure(cfg) => (cfg.subscription_id.clone(), cfg.region.clone()),
                _ => {
                    return Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                        message: "Expected Azure base config for Azure impersonation".to_string(),
                        field_name: Some("platform".to_string()),
                    }))
                }
            },
        };

        // Primary: OIDC WorkloadIdentity (if AZURE_FEDERATED_TOKEN_FILE is set).
        // Return WorkloadIdentity credentials instead of a pre-exchanged AccessToken
        // so each Azure client can request a token with the appropriate scope
        // (ARM for management APIs, vault.azure.net for Key Vault data plane, etc.).
        if let Some(token_file) = self.env.get("AZURE_FEDERATED_TOKEN_FILE") {
            info!(
                uami_client_id = %uami_client_id,
                customer_tenant_id = %customer_tenant_id,
                "Resolving Azure access via OIDC WorkloadIdentity"
            );

            let authority_host = self
                .env
                .get("AZURE_AUTHORITY_HOST")
                .cloned()
                .unwrap_or_else(|| "https://login.microsoftonline.com/".to_string());

            return Ok(ClientConfig::Azure(Box::new(
                alien_azure_clients::AzureClientConfig {
                    subscription_id: target_subscription,
                    tenant_id: customer_tenant_id.to_string(),
                    region: target_region,
                    credentials: alien_azure_clients::AzureCredentials::WorkloadIdentity {
                        client_id: uami_client_id.to_string(),
                        tenant_id: customer_tenant_id.to_string(),
                        federated_token_file: token_file.clone(),
                        authority_host,
                    },
                    service_overrides: None,
                },
            )));
        }

        // Fallback: SP cross-tenant client_credentials (local dev)
        if let ClientConfig::Azure(azure_config) = &base_config {
            if let alien_azure_clients::AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            } = &azure_config.credentials
            {
                info!(
                    customer_tenant_id = %customer_tenant_id,
                    "Resolving Azure access via SP cross-tenant client_credentials"
                );

                let access_token =
                    exchange_sp_credentials(&token_url, client_id, client_secret).await?;

                return Ok(ClientConfig::Azure(Box::new(
                    alien_azure_clients::AzureClientConfig {
                        subscription_id: target_subscription,
                        tenant_id: customer_tenant_id.to_string(),
                        region: target_region,
                        credentials: alien_azure_clients::AzureCredentials::AccessToken {
                            token: access_token,
                        },
                        service_overrides: None,
                    },
                )));
            }
        }

        Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: "No OIDC token file or SP credentials available for Azure cross-tenant access"
                .to_string(),
            method: Some("azure_impersonation".to_string()),
        }))
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

/// Exchange an OIDC token for an Azure ARM access token via federated identity credential
async fn exchange_oidc_token(
    token_url: &str,
    uami_client_id: &str,
    oidc_token: &str,
) -> Result<String> {
    let client = reqwest::Client::new();
    let form = [
        ("grant_type", "client_credentials"),
        ("client_id", uami_client_id),
        (
            "client_assertion_type",
            "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
        ),
        ("client_assertion", oidc_token),
        ("scope", "https://management.azure.com/.default"),
    ];

    let response = client
        .post(token_url)
        .form(&form)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::AuthenticationFailed {
            message: "OIDC token exchange request failed".to_string(),
            method: Some("oidc_token_exchange".to_string()),
        })?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: format!("OIDC token exchange failed ({}): {}", token_url, error_text),
            method: Some("oidc_token_exchange".to_string()),
        }));
    }

    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::AuthenticationFailed {
            message: "Failed to parse OIDC token exchange response".to_string(),
            method: Some("oidc_token_exchange".to_string()),
        })?;

    Ok(token_response.access_token)
}

/// Exchange SP credentials for an Azure ARM access token (cross-tenant)
async fn exchange_sp_credentials(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<String> {
    let client = reqwest::Client::new();
    let form = [
        ("grant_type", "client_credentials"),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("scope", "https://management.azure.com/.default"),
    ];

    let response = client
        .post(token_url)
        .form(&form)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::AuthenticationFailed {
            message: "SP cross-tenant token request failed".to_string(),
            method: Some("sp_cross_tenant".to_string()),
        })?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::AuthenticationFailed {
            message: format!(
                "SP cross-tenant token request failed ({}): {}",
                token_url, error_text
            ),
            method: Some("sp_cross_tenant".to_string()),
        }));
    }

    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::AuthenticationFailed {
            message: "Failed to parse SP token response".to_string(),
            method: Some("sp_cross_tenant".to_string()),
        })?;

    Ok(token_response.access_token)
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
