//! Importer for GCP AI (Vertex AI).
//!
//! GCP AI creates no cloud resource; the importer carries project and location
//! from the import payload directly into the controller state.

use alien_core::{
    import::{data::GcpAiImportData, ImportContext},
    Result, StackResourceState,
};

use crate::ai::gcp::GcpAiState;
use crate::ai::GcpAiController;
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// GCP Vertex AI importer.
#[derive(Debug, Default)]
pub struct GcpAiImporter;

impl ResourceImporter for GcpAiImporter {
    type ImportData = GcpAiImportData;

    fn import(
        &self,
        data: GcpAiImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = GcpAiController {
            state: GcpAiState::Ready,
            project: Some(data.project_id),
            location: Some(data.location),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
