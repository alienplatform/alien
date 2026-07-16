use crate::traits::credential_resolver::CredentialResolver;
use crate::traits::deployment_store::DeploymentRecord;
use alien_core::{ClientConfig, Platform};
use alien_error::{AlienError, Context, GenericError};
use alien_infra::ClientConfigExt;
use async_trait::async_trait;
use std::collections::HashMap;

pub struct EnvironmentCredentialResolver;

impl EnvironmentCredentialResolver {
    pub fn new() -> Self {
        Self
    }

    pub(crate) async fn resolve_from_env(
        &self,
        deployment: &DeploymentRecord,
        env: HashMap<String, String>,
    ) -> Result<ClientConfig, AlienError> {
        let base_config = ClientConfig::from_env(deployment.platform, &env)
            .await
            .map_err(|error| error.into_generic())?;
        let base_config = if deployment.platform == Platform::Azure {
            super::impersonation_credentials::apply_target_environment(
                base_config,
                deployment.environment_info.as_ref(),
            )?
        } else {
            base_config
        };

        let status = super::impersonation_credentials::parse_status(&deployment.status);
        if super::impersonation_credentials::uses_direct_management_credentials(status) {
            return Ok(base_config);
        }

        // Direct environment credentials remain the complete credential source
        // for single-account AWS and GCP deployments. Azure is different: the
        // same OIDC token file is trusted first by the external management
        // identity and, after setup, by the generated target UAMI.
        if deployment.platform != Platform::Azure {
            return Ok(base_config);
        }

        let Some(stack_state) = deployment.stack_state.as_ref() else {
            return Ok(base_config);
        };
        let has_remote_management = stack_state.resources.values().any(|resource| {
            resource.resource_type == alien_core::RemoteStackManagement::RESOURCE_TYPE.as_ref()
        });
        if !has_remote_management {
            return Ok(base_config);
        }

        alien_infra::RemoteAccessResolver::new(env)
            .resolve(
                base_config,
                stack_state,
                deployment.environment_info.as_ref(),
            )
            .await
            .context(GenericError {
                message: "Failed to resolve remote access from stack state".to_string(),
            })
    }
}

#[async_trait]
impl CredentialResolver for EnvironmentCredentialResolver {
    async fn resolve(&self, deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        // Read platform-specific environment credentials:
        // AWS: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION, etc.
        // GCP: GOOGLE_APPLICATION_CREDENTIALS, etc.
        // Azure: AZURE_SUBSCRIPTION_ID, AZURE_TENANT_ID, etc.
        // Imported stacks keep using these credentials through InitialSetup,
        // then hand off to the stack's RemoteStackManagement identity.
        self.resolve_from_env(deployment, std::env::vars().collect())
            .await
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use alien_core::{
        AzureCredentials, AzureEnvironmentInfo, EnvironmentInfo, Platform, RemoteStackManagement,
        RemoteStackManagementOutputs, Resource, ResourceOutputs, ResourceStatus,
        StackResourceState, StackState,
    };
    use chrono::Utc;

    pub(in crate::providers) fn azure_env() -> HashMap<String, String> {
        HashMap::from([
            (
                "AZURE_SUBSCRIPTION_ID".to_string(),
                "management-subscription".to_string(),
            ),
            (
                "AZURE_TENANT_ID".to_string(),
                "management-tenant".to_string(),
            ),
            (
                "AZURE_CLIENT_ID".to_string(),
                "management-client".to_string(),
            ),
            (
                "AZURE_FEDERATED_TOKEN_FILE".to_string(),
                "/tmp/federated-token".to_string(),
            ),
            ("AZURE_REGION".to_string(), "westus".to_string()),
        ])
    }

    pub(in crate::providers) fn azure_deployment(status: &str) -> DeploymentRecord {
        let remote_management = RemoteStackManagement::new("management".to_string()).build();
        let remote_state = StackResourceState::new_pending(
            RemoteStackManagement::RESOURCE_TYPE.to_string(),
            Resource::new(remote_management),
            None,
            Vec::new(),
        )
        .with_updates(|state| {
            state.status = ResourceStatus::Running;
            state.outputs = Some(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: "target-uami-resource-id".to_string(),
                access_configuration: serde_json::json!({
                    "uamiClientId": "target-uami-client",
                    "tenantId": "target-tenant"
                })
                .to_string(),
            }));
        });
        let mut stack_state =
            StackState::with_resource_prefix(Platform::Azure, "test-prefix".to_string());
        stack_state
            .resources
            .insert("management".to_string(), remote_state);

        DeploymentRecord {
            id: "deployment".to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: "deployment".to_string(),
            deployment_group_id: "group".to_string(),
            platform: Platform::Azure,
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: status.to_string(),
            stack_settings: None,
            stack_state: Some(stack_state),
            environment_info: Some(EnvironmentInfo::Azure(AzureEnvironmentInfo {
                tenant_id: "target-tenant".to_string(),
                subscription_id: "target-subscription".to_string(),
                location: "eastus".to_string(),
            })),
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn azure_initial_setup_keeps_real_management_workload_identity() {
        let resolved = EnvironmentCredentialResolver::new()
            .resolve_from_env(&azure_deployment("initial-setup"), azure_env())
            .await
            .expect("initial setup credentials should resolve");
        let azure = resolved
            .azure_config()
            .expect("Azure config should resolve");

        assert_eq!(azure.subscription_id, "target-subscription");
        assert_eq!(azure.tenant_id, "management-tenant");
        assert!(matches!(
            &azure.credentials,
            AzureCredentials::WorkloadIdentity { client_id, .. }
                if client_id == "management-client"
        ));
    }

    #[tokio::test]
    async fn azure_failed_initial_setup_does_not_require_remote_identity_outputs() {
        let mut deployment = azure_deployment("initial-setup-failed");
        for resource in deployment
            .stack_state
            .as_mut()
            .expect("test stack state")
            .resources
            .values_mut()
        {
            resource.outputs = None;
        }

        let resolved = EnvironmentCredentialResolver::new()
            .resolve_from_env(&deployment, azure_env())
            .await
            .expect("failed initial setup should keep bootstrap credentials");
        let azure = resolved
            .azure_config()
            .expect("Azure config should resolve");

        assert!(matches!(
            &azure.credentials,
            AzureCredentials::WorkloadIdentity { client_id, .. }
                if client_id == "management-client"
        ));
    }

    #[tokio::test]
    async fn azure_after_initial_setup_hands_off_to_remote_managed_identity() {
        let resolved = EnvironmentCredentialResolver::new()
            .resolve_from_env(&azure_deployment("provisioning"), azure_env())
            .await
            .expect("remote credentials should resolve");
        let azure = resolved
            .azure_config()
            .expect("Azure config should resolve");

        assert_eq!(azure.subscription_id, "target-subscription");
        assert_eq!(azure.tenant_id, "target-tenant");
        assert!(matches!(
            &azure.credentials,
            AzureCredentials::WorkloadIdentity {
                client_id,
                federated_token_file,
                ..
            } if client_id == "target-uami-client"
                && federated_token_file == "/tmp/federated-token"
        ));
    }
}
