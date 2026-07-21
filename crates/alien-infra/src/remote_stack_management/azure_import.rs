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
        let _ = data.management_permissions_applied;
        let controller = AzureRemoteStackManagementController {
            // Runtime and setup role definitions are deterministic. Entering
            // UpdateStart idempotently discovers conflicting setup assignment
            // IDs, records every owned ID, and prevents Running before exact
            // remote Storage grants have been reconciled.
            state: AzureRemoteStackManagementState::UpdateStart,
            uami_resource_id: Some(data.identity_id),
            uami_client_id: Some(data.client_id),
            uami_principal_id: Some(data.principal_id),
            tenant_id: Some(data.tenant_id),
            // The update flow reconstructs the FIC name and discovers concrete
            // role-assignment IDs while idempotently reconciling setup artifacts.
            fic_name: None,
            role_definition_id: None,
            resource_role_definition_ids: Default::default(),
            role_assignment_ids: Vec::new(),
            role_assignment_wait_until_epoch_secs: None,
            // Setup and runtime use deterministic role IDs. Unknown forces one
            // reconciliation before the import is considered current.
            applied_management_grant_fingerprint: None,
            _internal_stay_count: None,
        };
        make_imported_state_with_status(controller, ctx, ResourceStatus::Updating)
    }
}
