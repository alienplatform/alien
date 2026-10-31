//! RemoteStackManagement mutation that adds cross-account management resources.

use crate::{
    error::{ErrorData, Result},
    StackMutation,
};
use alien_core::{
    DeploymentConfig, DeploymentModel, KubernetesClusterOwnership, Platform, RemoteStackManagement,
    ResourceEntry, ResourceLifecycle, Stack, StackSettings, StackState,
};
use alien_error::AlienError;
use async_trait::async_trait;
use tracing::{debug, info};

/// Mutation that adds RemoteStackManagement resource for cross-account platforms.
///
/// This enables external management of the stack from another cloud account on
/// AWS, GCP, and Azure platforms. For other platforms, cross-account management
/// is not applicable.
pub struct RemoteStackManagementMutation;

fn is_cloud_backed_kubernetes_pull(
    platform: Platform,
    base_platform: Option<Platform>,
    stack_settings: &StackSettings,
) -> bool {
    platform == Platform::Kubernetes
        && matches!(
            base_platform.unwrap_or(platform),
            Platform::Aws | Platform::Gcp | Platform::Azure
        )
        && stack_settings.deployment_model == DeploymentModel::Pull
}

/// Whether the deployment is a cloud-backed Kubernetes pull deployment whose
/// operator is installed into an existing cluster. That operator already has its
/// workload identity, so the stack gets no setup-owned management identity to
/// provision or import.
fn operator_in_existing_cluster(
    platform: Platform,
    base_platform: Option<Platform>,
    stack_settings: &StackSettings,
) -> bool {
    is_cloud_backed_kubernetes_pull(platform, base_platform, stack_settings)
        && stack_settings
            .kubernetes
            .as_ref()
            .and_then(|settings| settings.cluster.as_ref())
            .is_some_and(|cluster| cluster.ownership != KubernetesClusterOwnership::Managed)
}

/// Whether a deployment's stack has a setup-owned management resource that is
/// rendered from the manager's management identity: cross-account cloud stacks
/// (and test), and Kubernetes stacks on a cloud, except an Operator installed
/// into an existing cluster. Without the identity such a stack renders without
/// the resource, so the manager must not render it then.
pub fn stack_renders_management_identity(
    platform: Platform,
    base_platform: Option<Platform>,
    stack_settings: &StackSettings,
) -> bool {
    // Kubernetes setup can also need a cloud management identity for Helm pull
    // mode; in that case the cloud is carried as the base platform.
    let cross_account = matches!(
        platform,
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Test
    ) || (platform == Platform::Kubernetes
        && matches!(
            base_platform.unwrap_or(platform),
            Platform::Aws | Platform::Gcp | Platform::Azure
        ));
    cross_account && !operator_in_existing_cluster(platform, base_platform, stack_settings)
}

#[async_trait]
impl StackMutation for RemoteStackManagementMutation {
    fn description(&self) -> &'static str {
        "Add RemoteStackManagement resource for cross-account access"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        let platform = stack_state.platform;

        if !stack_renders_management_identity(
            platform,
            config.base_platform,
            &config.stack_settings,
        ) {
            return false;
        }

        let cloud_backed_kubernetes_pull =
            is_cloud_backed_kubernetes_pull(platform, config.base_platform, &config.stack_settings);

        // Push-mode cloud stacks need an external manager management config.
        // Cloud-backed Kubernetes pull stacks still need this setup-owned
        // identity, but the agent uses it from inside the cluster.
        // Runtime credentials do not carry installer management configuration.
        // Keep the installed identity in automatic targets so reconciliation
        // cannot interpret its absence as a request to revoke its own access.
        let installed_automatic_management = config.stack_settings.setup_management
            == alien_core::SetupManagement::Automatic
            && stack_state.resources.values().any(|resource| {
                resource.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE
                    && resource.status != alien_core::ResourceStatus::Deleted
            });
        if config.management_config.is_none()
            && !cloud_backed_kubernetes_pull
            && !installed_automatic_management
        {
            return false;
        }

        !stack
            .resources
            .values()
            .any(|entry| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let platform = stack_state.platform;
        info!(
            "Adding RemoteStackManagement resource for platform {:?}",
            platform
        );

        let remote_mgmt_id = "management";

        if let Some(existing) = stack.resources.get(remote_mgmt_id) {
            return Err(AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: format!(
                    "resource ID '{remote_mgmt_id}' is reserved for stack management, but is already used by resource type '{}'",
                    existing.config.resource_type()
                ),
                resource_id: Some(remote_mgmt_id.to_string()),
            }));
        }

        // Create the RemoteStackManagement resource
        let remote_stack_management =
            RemoteStackManagement::new(remote_mgmt_id.to_string()).build();

        // Add it to the stack as a frozen resource (created once during setup)
        let remote_mgmt_entry = ResourceEntry {
            enabled_when: None,
            config: alien_core::Resource::new(remote_stack_management),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(), // No dependencies on other resources
            display_name: Some("Management identity".to_string()),
            description: Some(
                "Grants ongoing management access within the deployment boundary.".to_string(),
            ),
            remote_access: false,
        };

        stack
            .resources
            .insert(remote_mgmt_id.to_string(), remote_mgmt_entry);

        debug!("Added RemoteStackManagement resource '{}'", remote_mgmt_id);

        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        EnvironmentVariablesSnapshot, ExternalBindings, KubernetesClusterSettings,
        KubernetesSettings, ServiceAccount, StackSettings, Storage,
    };

    fn empty_stack() -> Stack {
        Stack::new("test".to_string()).build()
    }

    fn config(
        base_platform: Option<Platform>,
        deployment_model: DeploymentModel,
    ) -> DeploymentConfig {
        DeploymentConfig {
            gcp_federation_scope: None,
            deployment_name: None,
            stack_settings: StackSettings {
                deployment_model,
                ..StackSettings::default()
            },
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
            stored_secret_input_ids: None,
            input_values: Default::default(),
            allow_frozen_changes: false,
            setup_run: false,
            compute_backend: None,
            external_bindings: ExternalBindings::default(),
            base_platform,
            label_domain: None,
            observe_label_selector: None,
            observe_all_namespaces: false,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
            volume_restores: Vec::new(),
        }
    }

    #[test]
    fn cloud_backed_kubernetes_pull_runs_without_external_management_config() {
        let mutation = RemoteStackManagementMutation;
        let stack = empty_stack();
        let config = config(Some(Platform::Aws), DeploymentModel::Pull);

        assert!(mutation.should_run(&stack, &StackState::new(Platform::Kubernetes), &config));
    }

    #[test]
    fn existing_cloud_cluster_does_not_provision_remote_management() {
        let mutation = RemoteStackManagementMutation;
        let stack = empty_stack();
        let mut config = config(Some(Platform::Aws), DeploymentModel::Pull);
        config.stack_settings.kubernetes = Some(KubernetesSettings {
            cluster: Some(KubernetesClusterSettings {
                ownership: KubernetesClusterOwnership::Existing,
                namespace: Some("application".to_string()),
                cloud: None,
            }),
            exposure: None,
        });

        assert!(!mutation.should_run(&stack, &StackState::new(Platform::Kubernetes), &config));
    }

    #[test]
    fn cloud_backed_kubernetes_push_still_requires_external_management_config() {
        let mutation = RemoteStackManagementMutation;
        let stack = empty_stack();
        let config = config(Some(Platform::Aws), DeploymentModel::Push);

        assert!(!mutation.should_run(&stack, &StackState::new(Platform::Kubernetes), &config));
    }

    #[tokio::test]
    async fn reserved_management_id_is_never_overwritten() {
        let stack = Stack::new("test".to_string())
            .add(
                ServiceAccount::new("management".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let mutation = RemoteStackManagementMutation;
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Test,
            Platform::Kubernetes,
        ] {
            let mut config = config(Some(Platform::Aws), DeploymentModel::Pull);
            config.management_config = Some(alien_core::ManagementConfig::Kubernetes);
            let state = StackState::new(platform);
            assert!(mutation.should_run(&stack, &state, &config));
            let error = mutation
                .mutate(stack.clone(), &state, &config)
                .await
                .expect_err("a user service account must not be replaced");
            assert_eq!(error.code, "STACK_MUTATION_FAILED");
            assert!(error.message.contains("resource ID 'management' is reserved for stack management, but is already used by resource type 'service-account'"));
        }
    }

    #[test]
    fn management_name_is_not_reserved_when_mutation_does_not_run() {
        let stack = Stack::new("test".to_string())
            .add(
                ServiceAccount::new("management".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let config = config(None, DeploymentModel::Push);
        for platform in [Platform::Local, Platform::Kubernetes, Platform::Aws] {
            assert!(!RemoteStackManagementMutation.should_run(
                &stack,
                &StackState::new(platform),
                &config
            ));
        }
    }

    #[tokio::test]
    async fn access_only_stack_gets_both_setup_identities() {
        let stack = Stack::new("byo-bucket".to_string())
            .add_with_remote_access(
                Storage::new("exports".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let mut config = config(None, DeploymentModel::Push);
        config.management_config = Some(alien_core::ManagementConfig::Kubernetes);
        let state = StackState::new(Platform::Test);

        let stack = crate::mutations::RemoteBindingsMutation
            .mutate(stack, &state, &config)
            .await
            .expect("access identity mutation");
        let stack = RemoteStackManagementMutation
            .mutate(stack, &state, &config)
            .await
            .expect("management identity mutation");

        assert!(stack.resources.values().any(|entry| {
            entry.config.resource_type() == alien_core::RemoteBindings::RESOURCE_TYPE
        }));
        assert!(stack
            .resources
            .values()
            .any(|entry| { entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE }));
    }
}
