//! Importer for GCP ArtifactRegistry.

use alien_core::{
    import::{data::GcpArtifactRegistryImportData, ImportContext},
    Result, StackResourceState,
};

use crate::artifact_registry::{GcpArtifactRegistryController, GcpArtifactRegistryState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// GCP Artifact Registry importer.
#[derive(Debug, Default)]
pub struct GcpArtifactRegistryImporter;

impl ResourceImporter for GcpArtifactRegistryImporter {
    type ImportData = GcpArtifactRegistryImportData;

    fn import(
        &self,
        data: GcpArtifactRegistryImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // `repository_name` (full path) and `registry_endpoint` are
        // deterministically derived from project + region + repository_id,
        // so we only persist the canonical fields.
        let _ = (data.repository_name, data.registry_endpoint);
        let controller = GcpArtifactRegistryController {
            state: GcpArtifactRegistryState::Ready,
            project_id: Some(data.project_id),
            location: Some(data.region),
            repository_name: Some(data.repository_id),
            pull_service_account_email: Some(data.pull_service_account_email),
            push_service_account_email: Some(data.push_service_account_email),
            repository_operation_name: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
