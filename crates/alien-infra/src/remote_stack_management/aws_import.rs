//! Importer for AWS RemoteStackManagement (cross-account management role).

use alien_core::{
    import::{data::AwsRemoteStackManagementImportData, ImportContext},
    ErrorData, Result, StackResourceState,
};

use alien_error::{AlienError, Context};
use alien_permissions::generators::aws_permissions_boundary_covers;

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
        let invalid_import = || ErrorData::ImportDataInvalid {
            resource_id: ctx.resource_id.to_string(),
            resource_type: ctx.resource.config.resource_type(),
            platform: ctx.platform,
        };
        match (
            &data.permissions_boundary_arn,
            &data.permissions_boundary_document,
        ) {
            (Some(_), Some(document)) => {
                // Use the same parser as the release coverage check; do not persist a document
                // that the check cannot interpret later.
                aws_permissions_boundary_covers(document, document).context(invalid_import())?;
            }
            (None, None) => {}
            _ => {
                return Err(AlienError::new(alien_permissions::ErrorData::InvalidPermissionSet {
                    message: "Setup must provide both permissionsBoundaryArn and permissionsBoundaryDocument, or neither; rerun setup".to_string(),
                })).context(invalid_import());
            }
        }
        let controller = AwsRemoteStackManagementController {
            state: AwsRemoteStackManagementState::Ready,
            role_arn: Some(data.role_arn),
            role_name: Some(data.role_name),
            management_permissions_applied: data.management_permissions_applied,
            permissions_boundary_arn: data.permissions_boundary_arn,
            permissions_boundary_document: data.permissions_boundary_document,
            // Setup was applied by an external engine, which refreshes it itself.
            management_permissions_revision: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
