//! Stack mutations that modify the stack to ensure successful deployment.
//! These mutations run at deployment time but do NOT query cloud state.

mod activation;
pub mod azure_container_apps_environment;
pub mod azure_memory_adjustment;
pub mod azure_resource_group;
pub mod azure_service_activation;
pub mod azure_service_bus_namespace;
pub mod azure_storage_account;
pub mod compute_cluster;
pub mod container_resources;
pub mod extension_identity;
pub mod gcp_agent_platform_engine;
pub mod gcp_service_activation;
pub mod infrastructure_dependencies;
pub mod kubernetes_cluster;
pub mod management_permission_profile;
pub mod network;
pub mod remote_bindings;
pub mod remote_stack_management;
pub mod resource_link_permissions;
pub mod sandbox_bundle_storage;
pub mod sandbox_gateway;
pub mod sandbox_gateway_links;
pub(crate) mod sandbox_gateway_permissions;
pub mod secrets_vault;
pub mod service_account;
pub mod service_account_dependencies;

use crate::error::{ErrorData, Result};
use activation::Activation;
use alien_core::{DeploymentConfig, Platform, Stack, StackState};
use alien_error::AlienError;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn runs_on_platform_or_base(
    stack_state: &StackState,
    config: &DeploymentConfig,
    platform: Platform,
) -> bool {
    stack_state.platform == platform
        || (stack_state.platform == Platform::Kubernetes && config.base_platform == Some(platform))
}

/// The prefix of the activation ids for services only extension types need. Alien's own
/// activations are named `enable-*`, so the two never meet.
const EXTENSION_ACTIVATION_PREFIX: &str = "extension-service-";

/// The activations the stack needs on `platform`, by id: `alien`, the ones Alien's own types
/// need, and each service an extension type's descriptor lists.
///
/// A service one of `platform`'s declared activations enables keeps Alien's id even when only an
/// extension asks for it, so the activation keeps its id when an Alien type later needs the
/// service too. Service names compare without case, as Azure's do.
pub(crate) fn required_activations(
    stack: &Stack,
    platform: Platform,
    alien: BTreeSet<Activation>,
    mutation_name: &str,
) -> Result<BTreeMap<String, String>> {
    let known = match platform {
        Platform::Gcp => activation::gcp::ALL,
        Platform::Azure => activation::azure::ALL,
        _ => &[],
    };
    let mut activations: BTreeMap<String, String> = alien
        .into_iter()
        .map(|activation| {
            (
                activation.id().to_string(),
                activation.service().to_string(),
            )
        })
        .collect();
    for (_, entry) in stack.resources() {
        let resource_type = entry.config.resource_type();
        let Some(descriptor) = stack.descriptor(&resource_type) else {
            continue;
        };
        for service in descriptor.services.get(&platform).into_iter().flatten() {
            let (id, service) = match known
                .iter()
                .find(|alien| alien.service().eq_ignore_ascii_case(service))
            {
                Some(alien) => (alien.id().to_string(), alien.service().to_string()),
                None => (extension_activation_id(service), service.clone()),
            };
            match activations.get(&id) {
                Some(taken) if taken.eq_ignore_ascii_case(&service) => {}
                Some(taken) => {
                    return Err(AlienError::new(ErrorData::StackMutationFailed {
                        mutation_name: mutation_name.to_string(),
                        message: format!(
                            "Resource type '{resource_type}' needs the service '{service}', whose activation id '{id}' already enables '{taken}'"
                        ),
                        resource_id: Some(id),
                    }));
                }
                None => {
                    activations.insert(id, service);
                }
            }
        }
    }
    Ok(activations)
}

fn extension_activation_id(service: &str) -> String {
    let name: String = service
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("{EXTENSION_ACTIVATION_PREFIX}{name}")
}

pub use azure_container_apps_environment::AzureContainerAppsEnvironmentMutation;
pub use azure_memory_adjustment::AzureMemoryAdjustmentMutation;
pub use azure_resource_group::AzureResourceGroupMutation;
pub use azure_service_activation::AzureServiceActivationMutation;
pub use azure_service_bus_namespace::AzureServiceBusNamespaceMutation;
pub use azure_storage_account::AzureStorageAccountMutation;
pub use compute_cluster::ComputeClusterMutation;
pub use container_resources::ContainerResourcesMutation;
pub use extension_identity::ExtensionIdentityMutation;
pub use gcp_agent_platform_engine::GcpAgentPlatformEngineMutation;
pub use gcp_service_activation::GcpServiceActivationMutation;
pub use infrastructure_dependencies::InfrastructureDependenciesMutation;
pub use kubernetes_cluster::KubernetesClusterMutation;
pub use management_permission_profile::ManagementPermissionProfileMutation;
pub use network::NetworkMutation;
pub use remote_bindings::RemoteBindingsMutation;
pub use remote_stack_management::RemoteStackManagementMutation;
pub use resource_link_permissions::ResourceLinkPermissionsMutation;
pub use sandbox_bundle_storage::SandboxBundleStorageMutation;
pub use sandbox_gateway::SandboxGatewayMutation;
pub use sandbox_gateway_links::SandboxGatewayLinksMutation;
pub use secrets_vault::{DeployerSecretsVaultMutation, SecretsVaultMutation};
pub use service_account::ServiceAccountMutation;
pub use service_account_dependencies::ServiceAccountDependenciesMutation;
