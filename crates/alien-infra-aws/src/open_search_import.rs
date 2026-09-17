//! Importer for AWS OpenSearch (`experimental/aws-opensearch`).
//!
//! Maps the typed [`AwsOpenSearchImportData`] payload emitted by the
//! CloudFormation generator's `emit_import_ref` into an
//! [`AwsOpenSearchController`] pinned at its terminal `Ready` state. Like
//! every importer this is a pure data mapping — no AOSS calls, no liveness
//! verification; the outputs claim exactly what setup handed over.

use alien_core::{
    import::{data::AwsOpenSearchImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::open_search::{AwsOpenSearchController, AwsOpenSearchState};

/// AWS OpenSearch Serverless importer.
#[derive(Debug, Default)]
pub struct AwsOpenSearchImporter;

impl ResourceImporter for AwsOpenSearchImporter {
    type ImportData = AwsOpenSearchImportData;

    fn import(
        &self,
        data: AwsOpenSearchImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = AwsOpenSearchController {
            state: AwsOpenSearchState::Ready,
            collection_name: Some(data.collection_name),
            collection_id: Some(data.collection_id),
            collection_arn: Some(data.collection_arn),
            endpoint: Some(data.endpoint),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
