//! Importer for Azure Queue (Service Bus queue).

use alien_core::{
    import::{data::AzureQueueImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::queue::azure::{AzureQueueController, AzureQueueState};

/// Azure Service Bus queue importer.
#[derive(Debug, Default)]
pub struct AzureQueueImporter;

impl ResourceImporter for AzureQueueImporter {
    type ImportData = AzureQueueImportData;

    fn import(
        &self,
        data: AzureQueueImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = (data.subscription_id, data.resource_group);
        let controller = AzureQueueController {
            state: AzureQueueState::Ready,
            namespace_name: Some(data.namespace_name),
            queue_name: Some(data.queue_name),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
