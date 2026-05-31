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
        // Cloud Build triggers are the setup artifact's durable build
        // configuration. The controller-provisioned path stores the Alien
        // build id here because it creates builds on demand; imported stacks
        // store the trigger name so outputs point at the concrete setup
        // resource.
        let _ = data.trigger_id;
        let controller = GcpBuildController {
            state: GcpBuildState::Ready,
            project_id: Some(data.project_id),
            location: Some(data.region),
            build_config_id: Some(data.trigger_name),
            build_env_vars: Some(data.build_env_vars),
            service_account: Some(data.service_account_email),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
