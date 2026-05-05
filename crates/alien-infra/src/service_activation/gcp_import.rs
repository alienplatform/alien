//! Importer for GCP ServiceActivation (`google_project_service`).

use alien_core::{
    import::{data::GcpServiceActivationImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::service_activation::{GcpServiceActivationController, GcpServiceActivationState};

/// GCP service activation importer.
#[derive(Debug, Default)]
pub struct GcpServiceActivationImporter;

impl ResourceImporter for GcpServiceActivationImporter {
    type ImportData = GcpServiceActivationImportData;

    fn import(
        &self,
        data: GcpServiceActivationImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = data.project_id;
        let controller = GcpServiceActivationController {
            state: GcpServiceActivationState::Ready,
            service_name: Some(data.service_name),
            service_activated: data.activated,
            operation_name: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
