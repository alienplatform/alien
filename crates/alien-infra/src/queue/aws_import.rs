//! Importer for AWS Queue (SQS).

use alien_core::{
    import::{data::AwsQueueImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::queue::aws::{AwsQueueController, AwsQueueState};

/// AWS SQS queue importer.
#[derive(Debug, Default)]
pub struct AwsQueueImporter;

impl ResourceImporter for AwsQueueImporter {
    type ImportData = AwsQueueImportData;

    fn import(
        &self,
        data: AwsQueueImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // queue_arn is intentionally not stored on the controller — it's
        // reconstructed from queue_url at heartbeat time.
        let _ = data.queue_arn;
        let controller = AwsQueueController {
            state: AwsQueueState::Ready,
            queue_url: Some(data.queue_url),
            queue_name: Some(data.queue_name),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
