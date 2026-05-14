//! Importer for Azure RemoteStackManagement (UAMI + FIC).

use alien_core::{
    import::{data::AzureRemoteStackManagementImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
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
        let _ = data.management_permissions_applied;
        let controller = AzureRemoteStackManagementController {
            state: AzureRemoteStackManagementState::Ready,
            uami_resource_id: Some(data.identity_id),
            uami_client_id: Some(data.client_id),
            uami_principal_id: Some(data.principal_id),
            tenant_id: Some(data.tenant_id),
            // FIC name and role-assignment IDs are reconstructed by the
            // heartbeat path from `ctx.management_config` and the FIC template
            // emitter.
            fic_name: None,
            role_definition_id: None,
            role_assignment_ids: Vec::new(),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
