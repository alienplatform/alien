//! Azure infrastructure controllers.

macro_rules! provider_module {
    ($name:ident) => {
        pub mod $name {
            pub mod azure;
            pub mod azure_import;
            pub use azure::*;
            pub use azure_import::*;
        }
    };
}

provider_module!(ai);
provider_module!(artifact_registry);
provider_module!(kv);
provider_module!(network);
provider_module!(queue);
provider_module!(remote_bindings);
provider_module!(remote_stack_management);
provider_module!(sandbox);
provider_module!(service_account);
provider_module!(service_activation);
provider_module!(vault);

pub mod build {
    pub mod azure;
    pub mod azure_import;
    #[cfg(test)]
    pub mod fixtures;
    pub use azure::*;
    pub use azure_import::*;
}

pub mod storage {
    pub mod azure;
    pub mod azure_import;
    #[cfg(test)]
    pub mod fixtures;
    pub use azure::*;
    pub use azure_import::*;
}

pub mod key {
    pub mod azure;
    pub use azure::*;
}

pub mod worker {
    pub mod azure;
    pub mod azure_import;
    #[cfg(test)]
    pub mod fixtures;
    pub mod readiness_probe;
    pub use azure::*;
    pub use azure_import::*;
}
pub use worker::*;

pub mod infra_requirements {
    mod azure_container_apps_environment;
    mod azure_resource_group;
    pub mod azure_service_bus_namespace;
    mod azure_storage_account;
    pub mod azure_utils;

    pub use azure_container_apps_environment::*;
    pub use azure_resource_group::*;
    pub use azure_service_bus_namespace::*;
    pub use azure_storage_account::*;
}

pub use infra_requirements::azure_utils;

mod azure_permissions_helper;
pub use azure_permissions_helper::AzurePermissionsHelper;

pub mod core {
    pub use crate::{
        AzurePermissionsHelper, AzureServiceProvider as PlatformServiceProvider,
        ResourcePermissionsHelper,
    };
    #[cfg(test)]
    pub use alien_infra::{
        controller_test, deserialize_controller, MockPlatformServiceProvider, ResourceRegistry,
        StackExecutor,
    };
    pub use alien_infra_core::*;
}

pub mod error {
    pub use alien_infra_core::{ErrorData, Result};
}

pub mod import {
    pub use alien_infra_core::ResourceImporter;
}

pub mod import_helpers {
    pub use alien_infra_core::{make_imported_state, make_imported_state_with_status};
}

pub use error::Result;

#[cfg(test)]
pub use alien_infra::MockPlatformServiceProvider;

use std::sync::Arc;

use alien_azure_clients::{
    authorization::{AuthorizationApi, Scope},
    blob_containers::BlobContainerApi,
    cognitive_services::CognitiveServicesAccountsApi,
    container_apps::ContainerAppsApi,
    containerregistry::ContainerRegistryApi,
    event_grid::EventGridApi,
    keyvault::{KeyVaultKeysApi, KeyVaultManagementApi},
    long_running_operation::LongRunningOperationApi,
    managed_identity::ManagedIdentityApi,
    network::NetworkApi,
    resources::ResourcesApi,
    sandbox_groups::SandboxGroupsApi,
    service_bus::ServiceBusManagementApi,
    storage_accounts::StorageAccountsApi,
    tables::TableManagementApi,
    AzureClientConfig,
};
use alien_core::KubernetesCluster;
use alien_infra_core::ResourceControllerContext;
use alien_permissions::PermissionContext;

/// Provider-specific client factory used by Azure controllers.
///
/// This intentionally lives beside the controllers so the shared controller
/// contract does not depend on the Azure client graph.
#[async_trait::async_trait]
pub trait AzureServiceProvider: Send + Sync {
    fn get_azure_authorization_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AuthorizationApi>>;
    fn get_azure_blob_container_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn BlobContainerApi>>;
    fn get_azure_cognitive_services_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn CognitiveServicesAccountsApi>>;
    fn get_azure_container_apps_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ContainerAppsApi>>;
    fn get_azure_container_registry_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ContainerRegistryApi>>;
    fn get_azure_event_grid_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn EventGridApi>>;
    fn get_azure_key_vault_keys_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn KeyVaultKeysApi>>;
    fn get_azure_key_vault_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn KeyVaultManagementApi>>;
    fn get_azure_long_running_operation_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn LongRunningOperationApi>>;
    fn get_azure_managed_identity_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedIdentityApi>>;
    fn get_azure_network_client(&self, config: &AzureClientConfig) -> Result<Arc<dyn NetworkApi>>;
    fn get_azure_resources_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ResourcesApi>>;
    fn get_azure_sandbox_groups_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn SandboxGroupsApi>>;
    fn get_azure_service_bus_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ServiceBusManagementApi>>;
    fn get_azure_storage_accounts_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn StorageAccountsApi>>;
    fn get_azure_table_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn TableManagementApi>>;
    async fn get_azure_caller_principal_id(&self, config: &AzureClientConfig) -> Result<String>;
}

#[cfg(test)]
macro_rules! impl_test_azure_service_provider {
    ($(($method:ident, $api:ty)),* $(,)?) => {
        #[async_trait::async_trait]
        impl AzureServiceProvider for alien_infra::MockPlatformServiceProvider {
            $(
                fn $method(&self, config: &AzureClientConfig) -> Result<Arc<$api>> {
                    alien_infra::PlatformServiceProvider::$method(self, config)
                }
            )*

            async fn get_azure_caller_principal_id(
                &self,
                config: &AzureClientConfig,
            ) -> Result<String> {
                alien_infra::PlatformServiceProvider::get_azure_caller_principal_id(self, config)
                    .await
            }
        }
    };
}

#[cfg(test)]
impl_test_azure_service_provider!(
    (get_azure_authorization_client, dyn AuthorizationApi),
    (get_azure_blob_container_client, dyn BlobContainerApi),
    (
        get_azure_cognitive_services_client,
        dyn CognitiveServicesAccountsApi
    ),
    (get_azure_container_apps_client, dyn ContainerAppsApi),
    (
        get_azure_container_registry_client,
        dyn ContainerRegistryApi
    ),
    (get_azure_event_grid_client, dyn EventGridApi),
    (get_azure_key_vault_keys_client, dyn KeyVaultKeysApi),
    (
        get_azure_key_vault_management_client,
        dyn KeyVaultManagementApi
    ),
    (
        get_azure_long_running_operation_client,
        dyn LongRunningOperationApi
    ),
    (get_azure_managed_identity_client, dyn ManagedIdentityApi),
    (get_azure_network_client, dyn NetworkApi),
    (get_azure_resources_client, dyn ResourcesApi),
    (get_azure_sandbox_groups_client, dyn SandboxGroupsApi),
    (
        get_azure_service_bus_management_client,
        dyn ServiceBusManagementApi
    ),
    (get_azure_storage_accounts_client, dyn StorageAccountsApi),
    (get_azure_table_management_client, dyn TableManagementApi),
);

#[async_trait::async_trait]
pub trait AzureResourcePermissionsService: Send + Sync {
    fn azure_kubernetes_cluster_permission_context(
        &self,
        ctx: &ResourceControllerContext<'_>,
        cluster: &KubernetesCluster,
    ) -> Result<PermissionContext>;

    async fn apply_azure_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_scope: Scope,
        resource_type: &str,
        permission_type: &str,
    ) -> Result<()>;

    fn build_azure_permission_context(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext>;
}

#[cfg(test)]
struct TestAzureResourcePermissionsService;

#[cfg(test)]
#[async_trait::async_trait]
impl AzureResourcePermissionsService for TestAzureResourcePermissionsService {
    fn azure_kubernetes_cluster_permission_context(
        &self,
        ctx: &ResourceControllerContext<'_>,
        cluster: &KubernetesCluster,
    ) -> Result<PermissionContext> {
        alien_infra::ResourcePermissionsHelper::azure_kubernetes_cluster_permission_context(
            ctx, cluster,
        )
    }

    async fn apply_azure_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_scope: Scope,
        resource_type: &str,
        permission_type: &str,
    ) -> Result<()> {
        alien_infra::ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
            ctx,
            resource_id,
            resource_name,
            resource_scope,
            resource_type,
            permission_type,
        )
        .await
    }

    fn build_azure_permission_context(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext> {
        alien_infra::ResourcePermissionsHelper::build_azure_permission_context(ctx, resource_name)
    }
}

#[cfg(test)]
pub trait AzureControllerTestBuilderExt {
    fn azure_service_provider(
        self,
        provider: Arc<alien_infra::MockPlatformServiceProvider>,
    ) -> Self;
}

#[cfg(test)]
impl AzureControllerTestBuilderExt
    for alien_infra::controller_test::SingleControllerExecutorBuilder
{
    fn azure_service_provider(
        self,
        provider: Arc<alien_infra::MockPlatformServiceProvider>,
    ) -> Self {
        self.service_provider(provider.clone())
            .service::<dyn AzureServiceProvider>(provider)
            .service::<dyn AzureResourcePermissionsService>(Arc::new(
                TestAzureResourcePermissionsService,
            ))
    }
}

pub struct ResourcePermissionsHelper;

impl ResourcePermissionsHelper {
    pub fn azure_kubernetes_cluster_permission_context(
        ctx: &ResourceControllerContext<'_>,
        cluster: &KubernetesCluster,
    ) -> Result<PermissionContext> {
        ctx.services
            .require::<dyn AzureResourcePermissionsService>()?
            .azure_kubernetes_cluster_permission_context(ctx, cluster)
    }

    pub async fn apply_azure_resource_scoped_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_scope: Scope,
        resource_type: &str,
        permission_type: &str,
    ) -> Result<()> {
        ctx.services
            .require::<dyn AzureResourcePermissionsService>()?
            .apply_azure_resource_scoped_permissions(
                ctx,
                resource_id,
                resource_name,
                resource_scope,
                resource_type,
                permission_type,
            )
            .await
    }

    pub fn build_azure_permission_context(
        ctx: &ResourceControllerContext<'_>,
        resource_name: &str,
    ) -> Result<PermissionContext> {
        ctx.services
            .require::<dyn AzureResourcePermissionsService>()?
            .build_azure_permission_context(ctx, resource_name)
    }
}
