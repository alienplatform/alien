//! Infrastructure Dependencies mutation that adds dependencies from user resources to infrastructure resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, Platform, RemoteStackManagement, ResourceRef, Stack, StackState,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Mutation that adds dependencies from user resources to infrastructure resources.
///
/// This ensures that user-defined resources properly depend on the infrastructure
/// resources they need, such as:
/// - All Azure resources depending on the resource group
/// - All Kubernetes resources depending on the namespace
/// - Specific resources depending on service activations, storage accounts, etc.
pub struct InfrastructureDependenciesMutation;

#[async_trait]
impl StackMutation for InfrastructureDependenciesMutation {
    fn description(&self) -> &'static str {
        "Add dependencies from user resources to infrastructure resources"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Always run for platforms that have infrastructure dependencies, and
        // for stacks with remote management so every resource waits for the
        // cross-account access bridge before create/delete work.
        matches!(
            stack_state.platform,
            Platform::Azure | Platform::Gcp | Platform::Kubernetes
        ) || remote_stack_management_id(stack).is_some()
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let platform = stack_state.platform;
        info!(
            "Adding infrastructure dependencies for platform {:?}",
            platform
        );

        // Process each resource in the stack
        let resource_ids: Vec<_> = stack.resources.keys().cloned().collect();

        for resource_id in resource_ids {
            let Some(entry) = stack.resources.get(&resource_id) else {
                continue;
            };
            let resource_type = entry.config.resource_type();
            let deps =
                self.get_dependencies_for_resource(&stack, &resource_id, &resource_type, platform);

            if let Some(entry) = stack.resources.get_mut(&resource_id) {
                for dependency in deps {
                    if dependency.id() == resource_id {
                        continue;
                    }
                    if !entry.dependencies.contains(&dependency) {
                        entry.dependencies.push(dependency.clone());
                        debug!(
                            "Added infrastructure dependency {:?} to resource '{}'",
                            dependency, resource_id
                        );
                    }
                }
            }
        }

        Ok(stack)
    }
}

impl InfrastructureDependenciesMutation {
    /// Get dependencies that should be added to a concrete resource.
    fn get_dependencies_for_resource(
        &self,
        stack: &Stack,
        resource_id: &str,
        resource_type: &alien_core::ResourceType,
        platform: Platform,
    ) -> Vec<ResourceRef> {
        let mut dependencies = Vec::new();
        let is_infrastructure_resource =
            self.is_infrastructure_resource(resource_id, Some(resource_type));

        if platform == Platform::Azure
            && resource_id != "default-resource-group"
            && stack.resources.contains_key("default-resource-group")
        {
            dependencies.push(ResourceRef::new(
                alien_core::AzureResourceGroup::RESOURCE_TYPE,
                "default-resource-group",
            ));
        }

        if !is_infrastructure_resource {
            if let Some(management_id) = remote_stack_management_id(stack) {
                dependencies.push(ResourceRef::new(
                    RemoteStackManagement::RESOURCE_TYPE,
                    management_id,
                ));
            }
        }

        if !is_infrastructure_resource {
            dependencies.extend(self.get_resource_specific_dependencies(
                stack,
                resource_id,
                resource_type,
                platform,
            ));
        }

        dependencies
    }

    /// Get resource-specific dependencies for a resource type and platform
    fn get_resource_specific_dependencies(
        &self,
        stack: &Stack,
        resource_id: &str,
        resource_type: &alien_core::ResourceType,
        platform: Platform,
    ) -> Vec<ResourceRef> {
        match (platform, resource_type.as_ref()) {
            // Azure dependencies
            (Platform::Azure, "worker") => {
                let mut dependencies = vec![
                    ResourceRef::new(alien_core::ServiceActivation::RESOURCE_TYPE, "enable-app"),
                    ResourceRef::new(
                        alien_core::AzureContainerAppsEnvironment::RESOURCE_TYPE,
                        "default-container-env",
                    ),
                ];

                if stack
                    .resources
                    .get(resource_id)
                    .and_then(|entry| entry.config.downcast_ref::<alien_core::Worker>())
                    .is_some_and(|worker| worker.commands_enabled)
                {
                    dependencies.push(ResourceRef::new(
                        alien_core::ServiceActivation::RESOURCE_TYPE,
                        "enable-servicebus",
                    ));
                    dependencies.push(ResourceRef::new(
                        alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
                        "default-service-bus-namespace",
                    ));
                }

                dependencies
            }
            (Platform::Azure, "build") => {
                vec![
                    ResourceRef::new(alien_core::ServiceActivation::RESOURCE_TYPE, "enable-app"),
                    ResourceRef::new(
                        alien_core::AzureContainerAppsEnvironment::RESOURCE_TYPE,
                        "default-container-env",
                    ),
                ]
            }
            (Platform::Azure, "storage") => {
                vec![
                    ResourceRef::new(
                        alien_core::ServiceActivation::RESOURCE_TYPE,
                        "enable-storage",
                    ),
                    ResourceRef::new(
                        alien_core::AzureStorageAccount::RESOURCE_TYPE,
                        "default-storage-account",
                    ),
                ]
            }
            (Platform::Azure, "vault") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-keyvault",
                )]
            }
            (Platform::Azure, "kv") => {
                vec![
                    ResourceRef::new(
                        alien_core::ServiceActivation::RESOURCE_TYPE,
                        "enable-storage",
                    ),
                    ResourceRef::new(
                        alien_core::AzureStorageAccount::RESOURCE_TYPE,
                        "default-storage-account",
                    ),
                ]
            }
            (Platform::Azure, "artifact-registry") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-container-registry",
                )]
            }
            (Platform::Azure, "queue") => {
                vec![
                    ResourceRef::new(
                        alien_core::ServiceActivation::RESOURCE_TYPE,
                        "enable-servicebus",
                    ),
                    ResourceRef::new(
                        alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
                        "default-service-bus-namespace",
                    ),
                ]
            }

            // GCP dependencies
            (Platform::Gcp, "worker") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-cloud-run",
                )]
            }
            (Platform::Gcp, "build") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-cloud-build",
                )]
            }
            (Platform::Gcp, "storage") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-cloud-storage",
                )]
            }
            (Platform::Gcp, "role") => {
                vec![
                    ResourceRef::new(alien_core::ServiceActivation::RESOURCE_TYPE, "enable-iam"),
                    ResourceRef::new(
                        alien_core::ServiceActivation::RESOURCE_TYPE,
                        "enable-cloud-resource-manager",
                    ),
                ]
            }
            (Platform::Gcp, "artifact-registry") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-artifact-registry",
                )]
            }
            (Platform::Gcp, "vault") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-secret-manager",
                )]
            }
            (Platform::Gcp, "kv") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-firestore",
                )]
            }
            (Platform::Gcp, "queue") => {
                vec![ResourceRef::new(
                    alien_core::ServiceActivation::RESOURCE_TYPE,
                    "enable-pubsub",
                )]
            }

            _ => Vec::new(),
        }
    }

    /// Check if a resource is an infrastructure resource that shouldn't get dependencies added
    fn is_infrastructure_resource(
        &self,
        resource_id: &str,
        resource_type: Option<&alien_core::ResourceType>,
    ) -> bool {
        // Check by resource ID patterns
        if matches!(
            resource_id,
            "default-resource-group"
                | "default-container-env"
                | "default-storage-account"
                | "default-service-bus-namespace"
                | "default-network"
                | "ns"
                | "enable-app"
                | "enable-storage"
                | "enable-keyvault"
                | "enable-container-registry"
                | "enable-container-service"
                | "enable-network"
                | "enable-cloud-run"
                | "enable-cloud-build"
                | "enable-cloud-storage"
                | "enable-iam"
                | "enable-cloud-resource-manager"
                | "enable-artifact-registry"
                | "enable-secret-manager"
                | "enable-firestore"
                | "enable-pubsub"
                | "enable-container"
                | "enable-compute-engine"
                | "enable-iam-credentials"
                | "remote-stack-management"
                | "management"
        ) {
            return true;
        }

        // Check by resource type
        if let Some(resource_type) = resource_type {
            if matches!(
                resource_type.as_ref(),
                "azure-resource-group"
                    | "azure_resource_group"
                    | "azure-container-apps-environment"
                    | "azure_container_apps_environment"
                    | "azure-storage-account"
                    | "azure_storage_account"
                    | "azure-service-bus-namespace"
                    | "azure_service_bus_namespace"
                    | "kubernetes-namespace"
                    | "kubernetes_namespace"
                    | "kubernetes-cluster"
                    | "kubernetes_cluster"
                    | "network"
                    | "service-activation"
                    | "service_activation"
                    | "remote-stack-management"
                    | "permission-profile"
                    | "service-account"
            ) {
                return true;
            }
        }

        false
    }
}

fn remote_stack_management_id(stack: &Stack) -> Option<&str> {
    stack
        .resources
        .iter()
        .find(|(_, entry)| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
        .map(|(resource_id, _)| resource_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        AzureResourceGroup, AzureStorageAccount, EnvironmentVariablesSnapshot, ExternalBindings,
        KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
        KubernetesHeartbeatMode, Resource, ResourceEntry, ResourceLifecycle, StackSettings,
        Storage,
    };
    use indexmap::IndexMap;

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn azure_infrastructure_resources_depend_on_resource_group() {
        let mut resources = IndexMap::new();
        resources.insert(
            "default-resource-group".to_string(),
            ResourceEntry {
                config: Resource::new(
                    AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "default-storage-account".to_string(),
            ResourceEntry {
                config: Resource::new(
                    AzureStorageAccount::new("default-storage-account".to_string()).build(),
                ),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "app-storage".to_string(),
            ResourceEntry {
                config: Resource::new(Storage::new("app-storage".to_string()).build()),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };
        let stack_state = StackState::new(Platform::Azure);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result = InfrastructureDependenciesMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();
        let resource_group =
            ResourceRef::new(AzureResourceGroup::RESOURCE_TYPE, "default-resource-group");
        let storage_account = ResourceRef::new(
            AzureStorageAccount::RESOURCE_TYPE,
            "default-storage-account",
        );

        assert!(!result
            .resources
            .get("default-resource-group")
            .unwrap()
            .dependencies
            .contains(&resource_group));
        assert!(result
            .resources
            .get("default-storage-account")
            .unwrap()
            .dependencies
            .contains(&resource_group));
        let app_storage_deps = &result.resources.get("app-storage").unwrap().dependencies;
        assert!(app_storage_deps.contains(&resource_group));
        assert!(app_storage_deps.contains(&storage_account));
    }

    #[tokio::test]
    async fn kubernetes_cluster_does_not_depend_on_remote_management() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                KubernetesCluster::new("kubernetes".to_string())
                    .provider(KubernetesClusterProvider::Eks)
                    .ownership(KubernetesClusterOwnership::Managed)
                    .namespace("default".to_string())
                    .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("app-storage".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result = InfrastructureDependenciesMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();
        let remote_management =
            ResourceRef::new(RemoteStackManagement::RESOURCE_TYPE, "management");

        assert!(!result
            .resources
            .get("kubernetes")
            .unwrap()
            .dependencies
            .contains(&remote_management));
        assert!(result
            .resources
            .get("app-storage")
            .unwrap()
            .dependencies
            .contains(&remote_management));
    }
}
