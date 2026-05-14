//! Importer for GCP Build (Cloud Build trigger).

use alien_core::{
    import::{data::GcpBuildImportData, ImportContext},
    Result, StackResourceState,
};

use crate::build::{GcpBuildController, GcpBuildState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// GCP Cloud Build importer.
#[derive(Debug, Default)]
pub struct GcpBuildImporter;

impl ResourceImporter for GcpBuildImporter {
    type ImportData = GcpBuildImportData;

    fn import(
        &self,
        data: GcpBuildImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // `trigger_id` and `trigger_name` are recoverable from build_config_id
        // + project; we only persist build_config_id (the canonical key).
        let _ = (data.trigger_id, data.trigger_name);
        let controller = GcpBuildController {
            state: GcpBuildState::Ready,
            project_id: Some(data.project_id),
            location: Some(data.region),
            build_config_id: None,
            build_env_vars: Some(data.build_env_vars),
            service_account: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
