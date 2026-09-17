//! Importer for AWS RemoteStackManagement (cross-account management role).

use alien_core::{
    import::{data::AwsRemoteStackManagementImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
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
            state: AwsRemoteStackManagementState::Ready,
            role_arn: Some(data.role_arn),
            role_name: Some(data.role_name),
            management_permissions_applied: data.management_permissions_applied,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
