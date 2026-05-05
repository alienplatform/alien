//! Importer for AWS ArtifactRegistry (ECR + IAM access roles).

use alien_core::{
    import::{data::AwsArtifactRegistryImportData, ImportContext},
    Result, StackResourceState,
};

use crate::artifact_registry::{AwsArtifactRegistryController, AwsArtifactRegistryState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// AWS ECR importer.
#[derive(Debug, Default)]
pub struct AwsArtifactRegistryImporter;

impl ResourceImporter for AwsArtifactRegistryImporter {
    type ImportData = AwsArtifactRegistryImportData;

    fn import(
        &self,
        data: AwsArtifactRegistryImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // `registry_id` and `registry_endpoint` are deterministically derived
        // from `account_id` + `region` at output time, so the controller
        // doesn't store them.
        let _ = (data.registry_id, data.registry_endpoint);
        let controller = AwsArtifactRegistryController {
            state: AwsArtifactRegistryState::Ready,
            account_id: Some(data.account_id),
            region: Some(data.region),
            pull_role_arn: Some(data.pull_role_arn),
            push_role_arn: Some(data.push_role_arn),
            repository_prefix: Some(data.repository_prefix),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
