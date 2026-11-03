//! Importer for AWS KV (DynamoDB table).

use alien_core::{
    import::{data::AwsKvImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::kv::aws::AwsKvState;
use crate::kv::AwsKvController;

/// AWS DynamoDB key/value importer.
#[derive(Debug, Default)]
pub struct AwsKvImporter;

impl ResourceImporter for AwsKvImporter {
    type ImportData = AwsKvImportData;

    fn import(&self, data: AwsKvImportData, ctx: &ImportContext<'_>) -> Result<StackResourceState> {
        let controller = AwsKvController {
            state: AwsKvState::Ready,
            table_name: Some(data.table_name),
            table_arn: Some(data.table_arn),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
