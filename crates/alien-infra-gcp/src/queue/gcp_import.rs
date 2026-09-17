//! Importer for GCP Queue (Pub/Sub topic + subscription).

use alien_core::{
    import::{data::GcpQueueImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::queue::gcp::{GcpQueueController, GcpQueueState};

/// GCP Pub/Sub queue importer.
#[derive(Debug, Default)]
pub struct GcpQueueImporter;

impl ResourceImporter for GcpQueueImporter {
    type ImportData = GcpQueueImportData;

    fn import(
        &self,
        data: GcpQueueImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // The controller stores the short ids; full resource names are
        // metadata that the runtime path can rebuild from project + id.
        let _ = (data.project_id, data.topic_name, data.subscription_name);
        let controller = GcpQueueController {
            state: GcpQueueState::Ready,
            topic_name: Some(data.topic_id),
            subscription_name: Some(data.subscription_id),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
