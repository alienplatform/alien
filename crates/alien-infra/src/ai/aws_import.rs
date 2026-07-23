//! Importer for AWS AI (Bedrock).
//!
//! AWS AI creates no cloud resource; the importer carries the region from the
//! import payload directly into the controller state.

use alien_core::{
    import::{data::AwsAiImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::ai::aws::AwsAiState;
use crate::ai::AwsAiController;

/// AWS Bedrock AI importer.
#[derive(Debug, Default)]
pub struct AwsAiImporter;

impl ResourceImporter for AwsAiImporter {
    type ImportData = AwsAiImportData;

    fn import(&self, data: AwsAiImportData, ctx: &ImportContext<'_>) -> Result<StackResourceState> {
        let controller = AwsAiController {
            state: AwsAiState::Ready,
            region: Some(data.region),
            finetune: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
