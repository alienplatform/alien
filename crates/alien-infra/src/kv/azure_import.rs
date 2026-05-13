//! Importer for Azure KV.
//!
//! The Azure KV controller is backed by Azure Table Storage with the table
//! inside the stack-shared storage account. Terraform imports the same
//! storage-account/table shape so imported state can produce runtime
//! binding params immediately.

use alien_core::{
    import::{data::AzureKvImportData, ImportContext},
    AzureStorageAccountOutputs, Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::kv::azure::AzureKvState;
use crate::kv::AzureKvController;

/// Azure key/value importer.
#[derive(Debug, Default)]
pub struct AzureKvImporter;

impl ResourceImporter for AzureKvImporter {
    type ImportData = AzureKvImportData;

    fn import(
        &self,
        data: AzureKvImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let storage_account_outputs = AzureStorageAccountOutputs {
            resource_id: format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}",
                data.subscription_id, data.resource_group, data.storage_account_name
            ),
            primary_blob_endpoint: azure_storage_endpoint(&data.storage_account_name, "blob"),
            primary_file_endpoint: azure_storage_endpoint(&data.storage_account_name, "file"),
            primary_queue_endpoint: azure_storage_endpoint(&data.storage_account_name, "queue"),
            primary_table_endpoint: data.table_endpoint,
            primary_access_key: String::new(),
            connection_string: String::new(),
            account_name: data.storage_account_name,
        };

        let controller = AzureKvController {
            state: AzureKvState::Ready,
            table_name: Some(data.table_name),
            storage_account_outputs: Some(storage_account_outputs),
            resource_group_name: Some(data.resource_group),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}

fn azure_storage_endpoint(account_name: &str, service: &str) -> String {
    format!("https://{account_name}.{service}.core.windows.net/")
}
