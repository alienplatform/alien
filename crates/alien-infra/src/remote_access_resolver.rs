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
    RemoteStackManagementOutputs, StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
#[cfg(feature = "gcp")]
use alien_gcp_clients::{GcpClientConfigExt, GcpImpersonationConfig};
use std::collections::HashMap;
use tracing::info;
use uuid::Uuid;

const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

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
                    true,
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

    /// Resolve a GCP remote identity for an immediate Credential Access Boundary exchange.
    ///
    /// Unlike [`Self::resolve`], this does not eagerly mint a target access token. The
    /// caller must immediately materialize the returned config while exchanging it for
    /// a downscoped token. Deferring that validation prevents the same two-hop
    /// impersonation chain from being executed twice for one credential lease.
    #[cfg(feature = "gcp")]
    pub async fn resolve_gcp_for_access_boundary(
        &self,
        base_config: ClientConfig,
        stack_state: &StackState,
        target_environment: Option<&EnvironmentInfo>,
    ) -> Result<ClientConfig> {
        let remote_mgmt_outputs = self.find_remote_stack_management_outputs(stack_state)?;
        self.resolve_gcp_impersonation(base_config, &remote_mgmt_outputs, target_environment, false)
            .await
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
            session_name: Some(format!(
                "deployment-remote-access-{}",
                Uuid::new_v4().simple()
            )),
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
        materialize_credentials: bool,
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

        let resolved = base_config
            .impersonate(impersonation_config)
            .await
            .context(ErrorData::AuthenticationFailed {
                message: format!(
                    "Failed to impersonate GCP service account: {}",
                    service_account_email
                ),
                method: Some("service_account_impersonation".to_string()),
            })?;

        let ClientConfig::Gcp(gcp_config) = resolved else {
            return Err(AlienError::new(ErrorData::RemoteAccessInvalid {
                message: "GCP impersonation returned a non-GCP client configuration".to_string(),
                field_name: Some("platform".to_string()),
            }));
        };

        if materialize_credentials {
            // GCP impersonation configs are refreshable and lazy: constructing one
            // does not call IAMCredentials. Materialize the token at the credential
            // handoff boundary so propagation failures are reported to the caller
            // before any deployment operation starts.
            gcp_config
                .get_bearer_token(GCP_CLOUD_PLATFORM_SCOPE)
                .await
                .context(ErrorData::AuthenticationFailed {
                    message: format!(
                        "Failed to materialize GCP service account credentials: {}",
                        service_account_email
                    ),
                    method: Some("service_account_impersonation".to_string()),
                })?;
        }

        Ok(ClientConfig::Gcp(gcp_config))
    }

    /// Resolve Azure impersonation using target-side UAMI Workload Identity.
    ///
    /// The access_configuration from RSM outputs is JSON:
    ///   { "uamiClientId": "<client-id>", "tenantId": "<customer-tenant-id>" }
    ///
    /// The manager process must expose AZURE_FEDERATED_TOKEN_FILE. The target
    /// subscription trusts that token through the Federated Identity Credential
    /// created on the RemoteStackManagement UAMI during setup.
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

        let token_file = self.env.get("AZURE_FEDERATED_TOKEN_FILE").ok_or_else(|| {
            AlienError::new(ErrorData::AuthenticationFailed {
                message: "AZURE_FEDERATED_TOKEN_FILE is required for Azure remote stack access"
                    .to_string(),
                method: Some("azure_workload_identity".to_string()),
            })
        })?;

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

        Ok(ClientConfig::Azure(Box::new(
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
        )))
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
    use alien_core::{
        GcpClientConfig, GcpCredentials, GcpEnvironmentInfo, GcpServiceOverrides, Resource,
        ResourceLifecycle, ResourceOutputs, ResourceStatus, StackResourceState,
    };
    use httpmock::{Method::POST, MockServer};
    use serde_json::json;

    fn gcp_stack_state(service_account_email: &str) -> StackState {
        let config = RemoteStackManagement::new("management".to_string()).build();
        let mut state = StackState::new(Platform::Gcp);
        state.resources.insert(
            config.id.clone(),
            StackResourceState::builder()
                .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(config))
                .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                    management_resource_id: service_account_email.to_string(),
                    access_configuration: service_account_email.to_string(),
                }))
                .lifecycle(ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );
        state
    }

    fn gcp_base_config(iam_credentials_url: String) -> ClientConfig {
        ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "managing-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "source-token".to_string(),
            },
            service_overrides: Some(GcpServiceOverrides {
                endpoints: HashMap::from([("iamcredentials".to_string(), iam_credentials_url)]),
            }),
            project_number: Some("123456789".to_string()),
        }))
    }

    fn gcp_target_environment() -> EnvironmentInfo {
        EnvironmentInfo::Gcp(GcpEnvironmentInfo {
            project_id: "target-project".to_string(),
            project_number: "987654321".to_string(),
            region: "us-east1".to_string(),
        })
    }

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

    #[tokio::test]
    async fn gcp_remote_access_materializes_credentials_at_handoff() {
        let server = MockServer::start_async().await;
        let token_exchange = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(200).json_body(json!({
                    "accessToken": "target-token",
                    "expireTime": "2026-07-16T12:00:00Z"
                }));
            })
            .await;
        let service_account_email = "deployment@target-project.iam.gserviceaccount.com";

        let resolved = RemoteAccessResolver::default()
            .resolve(
                gcp_base_config(server.url("/v1")),
                &gcp_stack_state(service_account_email),
                Some(&gcp_target_environment()),
            )
            .await
            .expect("GCP remote access should materialize a usable target token");

        token_exchange.assert_async().await;
        let gcp = resolved
            .gcp_config()
            .expect("resolved remote access should remain GCP");
        assert_eq!(gcp.project_id, "target-project");
        assert_eq!(gcp.region, "us-east1");
        match &gcp.credentials {
            GcpCredentials::ImpersonatedServiceAccount { source, config } => {
                assert_eq!(config.service_account_email, service_account_email);
                assert_eq!(
                    source.credentials,
                    GcpCredentials::AccessToken {
                        token: "source-token".to_string()
                    }
                );
            }
            credentials => panic!(
                "resolved credentials should remain refreshable after validation, got {credentials:?}"
            ),
        }
    }

    #[tokio::test]
    async fn gcp_remote_access_reports_token_exchange_failure_during_handoff() {
        let server = MockServer::start_async().await;
        let denied_exchange = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(403).json_body(json!({
                    "error": {
                        "code": 403,
                        "message": "Permission 'iam.serviceAccounts.getAccessToken' denied",
                        "status": "PERMISSION_DENIED"
                    }
                }));
            })
            .await;
        let service_account_email = "deployment@target-project.iam.gserviceaccount.com";

        let error = RemoteAccessResolver::default()
            .resolve(
                gcp_base_config(server.url("/v1")),
                &gcp_stack_state(service_account_email),
                Some(&gcp_target_environment()),
            )
            .await
            .expect_err("GCP token exchange failure must fail credential handoff");

        assert!(
            denied_exchange.hits_async().await > 0,
            "credential materialization must attempt the IAM token exchange during handoff"
        );
        assert_eq!(error.code, "AUTHENTICATION_FAILED");
        assert!(
            error
                .to_string()
                .contains("Failed to materialize GCP service account credentials"),
            "handoff error should identify the failed token materialization: {error}"
        );
    }
}
