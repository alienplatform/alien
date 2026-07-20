//! Importer for Azure RemoteStackManagement (UAMI + FIC).

use alien_core::{
    import::{data::AzureRemoteStackManagementImportData, ImportContext},
    ResourceStatus, Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state_with_status;
use crate::remote_stack_management::{
    AzureRemoteStackManagementController, AzureRemoteStackManagementState,
};

/// Azure cross-subscription management identity importer.
#[derive(Debug, Default)]
pub struct AzureRemoteStackManagementImporter;

impl ResourceImporter for AzureRemoteStackManagementImporter {
    type ImportData = AzureRemoteStackManagementImportData;

    fn import(
        &self,
        data: AzureRemoteStackManagementImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = data.subscription_id;
        let _ = data.resource_group;
        let (state, status) = if data.management_permissions_applied {
            (
                AzureRemoteStackManagementState::WaitingForRbacPropagation,
                ResourceStatus::Provisioning,
            )
        } else {
            (
                AzureRemoteStackManagementState::Ready,
                ResourceStatus::Running,
            )
        };
        let controller = AzureRemoteStackManagementController {
            state,
            uami_resource_id: Some(data.identity_id),
            uami_client_id: Some(data.client_id),
            uami_principal_id: Some(data.principal_id),
            tenant_id: Some(data.tenant_id),
            // FIC name and role-assignment IDs are reconstructed by the
            // heartbeat path from `ctx.management_config` and the FIC template
            // emitter.
            fic_name: None,
            role_definition_id: None,
            resource_role_definition_ids: Default::default(),
            role_assignment_ids: Vec::new(),
            role_assignment_wait_until_epoch_secs: None,
            _internal_stay_count: None,
        };
        make_imported_state_with_status(controller, ctx, status)
    }
}
