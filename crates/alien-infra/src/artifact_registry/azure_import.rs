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
/// Pull references remain setup-owned. Import records them for authoritative
/// identity and role-assignment checks before a workload attaches the identity.
#[derive(Debug, Default)]
pub struct AzureArtifactRegistryImporter;

impl ResourceImporter for AzureArtifactRegistryImporter {
    type ImportData = AzureArtifactRegistryImportData;

    fn import(
        &self,
        data: AzureArtifactRegistryImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = data.push_principal_id;
        let controller = AzureArtifactRegistryController {
            state: AzureArtifactRegistryState::Ready,
            registry_name: Some(data.registry_name),
            resource_group_name: Some(data.resource_group),
            login_server: Some(data.login_server),
            subscription_id: Some(data.subscription_id),
            pull_identity_resource_id: data.pull_identity_resource_id,
            pull_principal_id: Some(data.pull_principal_id),
            pull_role_assignment_id: data.pull_role_assignment_id,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
