pub mod azure;
pub use azure::*;

// Re-export commonly used types for convenience
pub use azure::{
    AzureClientConfig, AzureClientConfigExt, AzureCredentials, AzureImpersonationConfig,
};

// Re-export all client APIs
pub use azure::authorization::{AuthorizationApi, AzureAuthorizationClient};
pub use azure::blob_containers::{AzureBlobContainerClient, BlobContainerApi};
pub use azure::compute::{AzureVmssClient, VirtualMachineScaleSetsApi};
pub use azure::container_apps::{AzureContainerAppsClient, ContainerAppsApi};
pub use azure::containerregistry::{AzureContainerRegistryClient, ContainerRegistryApi};
pub use azure::disks::{AzureManagedDisksClient, ManagedDisksApi};
pub use azure::keyvault::{
    AzureKeyVaultManagementClient, AzureKeyVaultSecretsClient, KeyVaultManagementApi,
    KeyVaultSecretsApi,
};
pub use azure::load_balancers::{AzureLoadBalancerClient, LoadBalancerApi};
pub use azure::long_running_operation::{LongRunningOperationApi, LongRunningOperationClient};
pub use azure::managed_identity::{AzureManagedIdentityClient, ManagedIdentityApi};
pub use azure::managed_services::{AzureManagedServicesClient, ManagedServicesApi};
pub use azure::network::{AzureNetworkClient, NetworkApi};
pub use azure::resources::{AzureResourcesClient, ResourcesApi};
pub use azure::service_bus::{
    AzureServiceBusDataPlaneClient, AzureServiceBusManagementClient, ServiceBusDataPlaneApi,
    ServiceBusManagementApi,
};
pub use azure::storage_accounts::{AzureStorageAccountsClient, StorageAccountsApi};
pub use azure::tables::{
    AzureTableManagementClient, AzureTableStorageClient, TableManagementApi, TableStorageApi,
};
pub use azure::token_cache::AzureTokenCache;

// Re-export error types from alien-client-core
pub use alien_client_core::{Error, ErrorData, Result};
