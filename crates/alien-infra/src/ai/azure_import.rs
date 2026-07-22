//! Importer for Azure AI (AIServices / Azure AI Foundry).

use alien_core::{
    import::{data::AzureAiImportData, ImportContext},
    Result, StackResourceState,
};

use crate::ai::azure::AzureAiState;
use crate::ai::AzureAiController;
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// Azure AI importer.
#[derive(Debug, Default)]
pub struct AzureAiImporter;

impl ResourceImporter for AzureAiImporter {
    type ImportData = AzureAiImportData;

    fn import(
        &self,
        data: AzureAiImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = AzureAiController {
            state: AzureAiState::Ready,
            account_name: Some(data.account_name),
            endpoint: Some(data.endpoint),
            resource_group: Some(data.resource_group),
            location: Some(data.location),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
