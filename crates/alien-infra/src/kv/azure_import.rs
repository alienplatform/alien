//! Importer for Azure KV.
//!
//! The current Azure KV controller is backed by Azure Table Storage with
//! the table inside a stack-shared storage account. The wire `ImportData`
//! describes a Cosmos DB-shaped payload — a planned refactor to align
//! the controller with the new TF / CFN emitters. Until then, the
//! importer maps `container_name` onto the controller's `table_name`
//! field and leaves storage-account outputs to be rebuilt from the
//! `default-storage-account` dependency at first reconcile.

use alien_core::{
    import::{data::AzureKvImportData, ImportContext},
    Result, StackResourceState,
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
        let _ = (
            data.subscription_id,
            data.account_name,
            data.database_name,
            data.endpoint,
        );
        let controller = AzureKvController {
            state: AzureKvState::Ready,
            table_name: Some(data.container_name),
            // Filled lazily from the `default-storage-account` dependency
            // when the heartbeat path runs.
            storage_account_outputs: None,
            resource_group_name: Some(data.resource_group),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
