//! Importer for Azure ServiceAccount (UAMI).

use alien_core::{
    import::{data::AzureServiceAccountImportData, ImportContext},
    ResourceStatus, Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state_with_status;
use crate::service_account::AzureServiceAccountController;

/// Azure User-Assigned Managed Identity importer.
#[derive(Debug, Default)]
pub struct AzureServiceAccountImporter;

impl ResourceImporter for AzureServiceAccountImporter {
    type ImportData = AzureServiceAccountImportData;

    fn import(
        &self,
        data: AzureServiceAccountImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        use crate::service_account::azure::AzureServiceAccountState;

        let _ = (data.subscription_id, data.resource_group);
        let (state, status) = if data.stack_permissions_applied {
            (
                AzureServiceAccountState::WaitingForRbacPropagation,
                ResourceStatus::Provisioning,
            )
        } else {
            (AzureServiceAccountState::Ready, ResourceStatus::Running)
        };
        let controller = AzureServiceAccountController {
            state,
            identity_resource_id: Some(data.identity_id),
            identity_client_id: Some(data.client_id),
            identity_principal_id: Some(data.principal_id),
            // Custom-role and assignment IDs are reconstructed at heartbeat
            // time when permissions need to be re-applied. Importing them
            // would require leaking ARM IDs into the wire payload that the
            // generator doesn't produce; leaving them empty is safe because
            // the controller's update path is idempotent.
            custom_role_definition_ids: Vec::new(),
            role_assignment_ids: Vec::new(),
            stack_permissions_applied: data.stack_permissions_applied,
            managed_identity_wait_until_epoch_secs: None,
            role_assignment_wait_until_epoch_secs: None,
            _internal_stay_count: None,
        };
        make_imported_state_with_status(controller, ctx, status)
    }
}
