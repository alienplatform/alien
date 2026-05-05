//! Importer for Azure Build (Container Registry build task).

use alien_core::{
    import::{data::AzureBuildImportData, ImportContext},
    Result, StackResourceState,
};

use crate::build::{AzureBuildController, AzureBuildState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// Azure Container Registry build importer.
#[derive(Debug, Default)]
pub struct AzureBuildImporter;

impl ResourceImporter for AzureBuildImporter {
    type ImportData = AzureBuildImportData;

    fn import(
        &self,
        data: AzureBuildImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = (data.subscription_id, data.registry_name, data.task_name);
        let controller = AzureBuildController {
            state: AzureBuildState::Ready,
            // The Azure build controller derives `managed_environment_id`
            // from the Container Apps Environment dependency at heartbeat
            // time; same for `managed_identity_id`. Importing skips the
            // initial Provisioning flow that resolves those.
            managed_environment_id: None,
            resource_group_name: Some(data.resource_group),
            build_env_vars: Some(data.build_env_vars),
            managed_identity_id: None,
            resource_prefix: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
