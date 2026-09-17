//! Importer for Azure ArtifactRegistry (Azure Container Registry).

use alien_core::{
    import::{data::AzureArtifactRegistryImportData, ImportContext},
    Result, StackResourceState,
};

use crate::artifact_registry::{AzureArtifactRegistryController, AzureArtifactRegistryState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// Azure Container Registry importer.
///
/// `pull_principal_id` / `push_principal_id` describe the *current* runtime
/// access bindings; the controller does not persist them. They flow through
/// the `ServiceAccount` resource graph at heartbeat time.
#[derive(Debug, Default)]
pub struct AzureArtifactRegistryImporter;

impl ResourceImporter for AzureArtifactRegistryImporter {
    type ImportData = AzureArtifactRegistryImportData;

    fn import(
        &self,
        data: AzureArtifactRegistryImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = (data.pull_principal_id, data.push_principal_id);
        let controller = AzureArtifactRegistryController {
            state: AzureArtifactRegistryState::Ready,
            registry_name: Some(data.registry_name),
            resource_group_name: Some(data.resource_group),
            login_server: Some(data.login_server),
            subscription_id: Some(data.subscription_id),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
