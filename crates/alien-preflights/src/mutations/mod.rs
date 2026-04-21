//! Stack mutations that modify the stack to ensure successful deployment.
//! These mutations run at deployment time but do NOT query cloud state.

pub mod azure_container_apps_environment;
pub mod azure_memory_adjustment;
pub mod azure_resource_group;
pub mod azure_service_activation;
pub mod azure_service_bus_namespace;
pub mod azure_storage_account;
pub mod container_cluster;
pub mod gcp_service_activation;
pub mod infrastructure_dependencies;
pub mod management_permission_profile;
pub mod network;
pub mod remote_stack_management;
pub mod secrets_vault;
pub mod service_account;
pub mod service_account_dependencies;

pub use azure_container_apps_environment::AzureContainerAppsEnvironmentMutation;
pub use azure_memory_adjustment::AzureMemoryAdjustmentMutation;
pub use azure_resource_group::AzureResourceGroupMutation;
pub use azure_service_activation::AzureServiceActivationMutation;
pub use azure_service_bus_namespace::AzureServiceBusNamespaceMutation;
pub use azure_storage_account::AzureStorageAccountMutation;
pub use container_cluster::ContainerClusterMutation;
pub use gcp_service_activation::GcpServiceActivationMutation;
pub use infrastructure_dependencies::InfrastructureDependenciesMutation;
pub use management_permission_profile::ManagementPermissionProfileMutation;
pub use network::NetworkMutation;
pub use remote_stack_management::RemoteStackManagementMutation;
pub use secrets_vault::SecretsVaultMutation;
pub use service_account::ServiceAccountMutation;
pub use service_account_dependencies::ServiceAccountDependenciesMutation;
