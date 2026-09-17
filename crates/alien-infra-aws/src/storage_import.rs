//! Importer for AWS Storage (S3 bucket).
//!
//! Maps the typed [`AwsStorageImportData`] payload emitted by the
//! CloudFormation / Terraform generator into a [`AwsStorageController`]
//! pinned at its terminal `Ready` state.

use alien_core::{
    import::{data::AwsStorageImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::storage::{AwsStorageController, AwsStorageState};

/// AWS S3 storage importer.
#[derive(Debug, Default)]
pub struct AwsStorageImporter;

impl ResourceImporter for AwsStorageImporter {
    type ImportData = AwsStorageImportData;

    fn import(
        &self,
        data: AwsStorageImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = AwsStorageController {
            state: AwsStorageState::Ready,
            bucket_name: Some(data.bucket_name),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
