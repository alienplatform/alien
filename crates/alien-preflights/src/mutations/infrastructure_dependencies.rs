//! Infrastructure Dependencies mutation that adds dependencies from user resources to infrastructure resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{DeploymentConfig, Platform, ResourceRef, Stack, StackState};
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
        _stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Always run for platforms that have infrastructure dependencies
        matches!(
            stack_state.platform,
            Platform::Azure | Platform::Gcp | Platform::Kubernetes
        )
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

        // Get global dependencies that all resources should have
        let global_deps = self.get_global_dependencies(platform);

        // Process each resource in the stack
        let resource_ids: Vec<_> = stack.resources.keys().cloned().collect();

        for resource_id in resource_ids {
            if let Some(entry) = stack.resources.get_mut(&resource_id) {
                let resource_type = entry.config.resource_type();

                // Add global dependencies before the infrastructure-resource
                // check. Azure infrastructure resources such as managed
                // identities, role definitions, storage accounts, and service
                // bus namespaces still live inside the deployment resource
                // group and must wait for it.
                for global_dep in &global_deps {
                    if global_dep.id() == resource_id {
                        continue;
                    }
                    if !entry.dependencies.contains(global_dep) {
                        entry.dependencies.push(global_dep.clone());
                        debug!(
                            "Added global dependency {:?} to resource '{}'",
                            global_dep, resource_id
                        );
                    }
                }

                // Skip resource-specific dependencies for infrastructure
                // resources themselves; the global dependencies above are
                // still required.
                if self.is_infrastructure_resource(&resource_id, Some(&resource_type)) {
                    continue;
                }

                // Add resource-specific dependencies
                let specific_deps =
                    self.get_resource_specific_dependencies(&resource_type, platform);
                for specific_dep in specific_deps {
                    if !entry.dependencies.contains(&specific_dep) {
                        entry.dependencies.push(specific_dep.clone());
                        debug!(
                            "Added resource-specific dependency {:?} to resource '{}'",
                            specific_dep, resource_id
                        );
                    }
                }
            }
        }

        Ok(stack)
    }
}

impl InfrastructureDependenciesMutation {
    /// Get global dependencies that all resources should have for a platform
    fn get_global_dependencies(&self, platform: Platform) -> Vec<ResourceRef> {
        match platform {
            Platform::Azure => {
                vec![ResourceRef::new(
                    alien_core::AzureResourceGroup::RESOURCE_TYPE,
                    "default-resource-group",
                )]
            }
            // Kubernetes: namespace is created by Helm, not as a dependency
            _ => Vec::new(),
        }
    }

    /// Get resource-specific dependencies for a resource type and platform
    fn get_resource_specific_dependencies(
        &self,
        resource_type: &alien_core::ResourceType,
        platform: Platform,
    ) -> Vec<ResourceRef> {
        match (platform, resource_type.as_ref()) {
            // Azure dependencies
            (Platform::Azure, "worker") => {
                vec![
                    ResourceRef::new(alien_core::ServiceActivation::RESOURCE_TYPE, "enable-app"),
                    ResourceRef::new(
                        alien_core::AzureContainerAppsEnvironment::RESOURCE_TYPE,
                        "default-container-env",
                    ),
                ]
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
                | "ns"
                | "enable-app"
                | "enable-storage"
                | "enable-keyvault"
                | "enable-container-registry"
                | "enable-cloud-run"
                | "enable-cloud-build"
                | "enable-cloud-storage"
                | "enable-iam"
                | "enable-cloud-resource-manager"
                | "enable-artifact-registry"
                | "enable-secret-manager"
                | "enable-firestore"
                | "enable-pubsub"
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
                    | "azure-container-apps-environment"
                    | "azure-storage-account"
                    | "kubernetes-namespace"
                    | "service-activation"
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        AzureResourceGroup, AzureStorageAccount, EnvironmentVariablesSnapshot, ExternalBindings,
        Resource, ResourceEntry, ResourceLifecycle, StackSettings, Storage,
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
}
