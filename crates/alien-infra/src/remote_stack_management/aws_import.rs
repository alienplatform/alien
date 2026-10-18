//! Importer for AWS RemoteStackManagement (cross-account management role).

use alien_core::{
    Result, StackResourceState,
    import::{ImportContext, data::AwsRemoteStackManagementImportData},
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
        if let Some(function_arn) = &data.worker_gateway_cleanup_function_arn {
            if !data.management_permissions_applied {
                return Err(alien_core::ErrorData::GenericError {
                    message: "Worker gateway cleanup requires setup-applied management permissions"
                        .to_string(),
                }
                .into());
            }
            alien_core::aws_worker_gateway::validate_cleanup_function_arn(
                function_arn,
                &data.role_arn,
                &data.role_name,
                ctx.region,
            )?;
        }
        let controller = AwsRemoteStackManagementController {
            state: AwsRemoteStackManagementState::Ready,
            role_arn: Some(data.role_arn),
            role_name: Some(data.role_name),
            management_permissions_applied: data.management_permissions_applied,
            management_permissions_revision: None,
            installed_management_grants: data
                .installed_management_grants
                .filter(|_| data.management_permissions_applied),
            worker_gateway_cleanup_function_arn: data.worker_gateway_cleanup_function_arn,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
