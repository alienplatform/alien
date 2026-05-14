//! Importer for Azure Storage (Blob container).

use alien_core::{
    import::{
        data::{
            AzureContainerAppsEnvironmentImportData, AzureResourceGroupImportData,
            AzureServiceBusNamespaceImportData, AzureStorageAccountImportData,
            AzureStorageImportData,
        },
        ImportContext,
    },
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::infra_requirements::azure_utils::{
    azure_resource_group_resource_id, azure_service_bus_namespace_resource_id,
};
use crate::infra_requirements::{
    AzureContainerAppsEnvironmentController, AzureContainerAppsEnvironmentState,
    AzureResourceGroupController, AzureResourceGroupState, AzureServiceBusNamespaceController,
    AzureServiceBusNamespaceState, AzureStorageAccountController, AzureStorageAccountState,
};
use crate::storage::{AzureStorageController, AzureStorageState};

/// Azure Blob storage importer.
#[derive(Debug, Default)]
pub struct AzureStorageImporter;

impl ResourceImporter for AzureStorageImporter {
    type ImportData = AzureStorageImportData;

    fn import(
        &self,
        data: AzureStorageImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = (data.subscription_id, data.resource_group);
        let controller = AzureStorageController {
            state: AzureStorageState::Ready,
            container_name: Some(data.container_name),
            storage_account_name: Some(data.storage_account_name),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}

/// Azure ResourceGroup auxiliary importer (preflight-injected).
#[derive(Debug, Default)]
pub struct AzureResourceGroupImporter;

impl ResourceImporter for AzureResourceGroupImporter {
    type ImportData = AzureResourceGroupImportData;

    fn import(
        &self,
        data: AzureResourceGroupImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let resource_id =
            azure_resource_group_resource_id(&data.subscription_id, &data.resource_group);
        let controller = AzureResourceGroupController {
            state: AzureResourceGroupState::Ready,
            resource_group_name: Some(data.resource_group),
            resource_id: Some(resource_id),
            location: Some(data.location),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}

/// Azure StorageAccount auxiliary importer.
#[derive(Debug, Default)]
pub struct AzureStorageAccountImporter;

impl ResourceImporter for AzureStorageAccountImporter {
    type ImportData = AzureStorageAccountImportData;

    fn import(
        &self,
        data: AzureStorageAccountImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // Access keys / connection strings stay out of the wire payload —
        // they are sensitive runtime metadata. The heartbeat path fetches
        // them on demand from the storage account.
        let _ = data.subscription_id;
        let _ = data.resource_group;
        let _ = data.queue_endpoint;
        let controller = AzureStorageAccountController {
            state: AzureStorageAccountState::Ready,
            account_name: Some(data.storage_account_name),
            resource_id: None,
            primary_access_key: None,
            connection_string: None,
            primary_blob_endpoint: Some(data.blob_endpoint),
            primary_file_endpoint: None,
            primary_queue_endpoint: None,
            primary_table_endpoint: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}

/// Azure ContainerAppsEnvironment auxiliary importer.
#[derive(Debug, Default)]
pub struct AzureContainerAppsEnvironmentImporter;

impl ResourceImporter for AzureContainerAppsEnvironmentImporter {
    type ImportData = AzureContainerAppsEnvironmentImportData;

    fn import(
        &self,
        data: AzureContainerAppsEnvironmentImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = data.subscription_id;
        let controller = AzureContainerAppsEnvironmentController {
            state: AzureContainerAppsEnvironmentState::Ready,
            environment_name: Some(data.environment_name),
            resource_id: Some(data.resource_id),
            resource_group_name: Some(data.resource_group),
            default_domain: Some(data.default_domain),
            static_ip: None,
            long_running_operation: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}

/// Azure ServiceBusNamespace auxiliary importer.
#[derive(Debug, Default)]
pub struct AzureServiceBusNamespaceImporter;

impl ResourceImporter for AzureServiceBusNamespaceImporter {
    type ImportData = AzureServiceBusNamespaceImportData;

    fn import(
        &self,
        data: AzureServiceBusNamespaceImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let resource_id = azure_service_bus_namespace_resource_id(
            &data.subscription_id,
            &data.resource_group,
            &data.namespace_name,
        );
        let controller = AzureServiceBusNamespaceController {
            state: AzureServiceBusNamespaceState::Ready,
            namespace_name: Some(data.namespace_name),
            resource_group_name: Some(data.resource_group),
            resource_id: Some(resource_id),
            fqdn: Some(data.endpoint.clone()),
            endpoint: Some(data.endpoint),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
