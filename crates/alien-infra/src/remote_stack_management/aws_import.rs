//! Importer for AWS RemoteStackManagement (cross-account management role).

use alien_core::{
    import::{data::AwsRemoteStackManagementImportData, ImportContext},
    ResourceStatus, Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state_with_status;
use crate::remote_stack_management::{
    AwsRemoteStackManagementController, AwsRemoteStackManagementState,
};

/// AWS cross-account management role importer.
#[derive(Debug, Default)]
pub struct AwsRemoteStackManagementImporter;

impl ResourceImporter for AwsRemoteStackManagementImporter {
    type ImportData = AwsRemoteStackManagementImportData;

    fn import(
        &self,
        data: AwsRemoteStackManagementImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = AwsRemoteStackManagementController {
            // Setup artifacts cannot persist the runtime grant fingerprint.
            // Enter the normal update flow so exact resource grants are
            // reconciled before the imported stack can report Running.
            state: AwsRemoteStackManagementState::UpdateStart,
            role_arn: Some(data.role_arn),
            role_name: Some(data.role_name),
            management_permissions_applied: data.management_permissions_applied,
            applied_management_grant_fingerprint: None,
            _internal_stay_count: None,
        };
        make_imported_state_with_status(controller, ctx, ResourceStatus::Updating)
    }
}
