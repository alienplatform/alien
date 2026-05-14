//! Importer for GCP RemoteStackManagement (cross-project SA).

use alien_core::{
    import::{data::GcpRemoteStackManagementImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::remote_stack_management::{
    GcpRemoteStackManagementController, GcpRemoteStackManagementState,
};

/// GCP cross-project management service account importer.
#[derive(Debug, Default)]
pub struct GcpRemoteStackManagementImporter;

impl ResourceImporter for GcpRemoteStackManagementImporter {
    type ImportData = GcpRemoteStackManagementImportData;

    fn import(
        &self,
        data: GcpRemoteStackManagementImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = data.project_id;
        let controller = GcpRemoteStackManagementController {
            state: GcpRemoteStackManagementState::Ready,
            service_account_email: Some(data.service_account_email),
            service_account_unique_id: Some(data.service_account_unique_id),
            // The custom role + token-creator binding are created together
            // when management permissions are applied; otherwise both
            // booleans stay false (manager will refresh on first reconcile).
            custom_role_name: None,
            role_created: data.management_permissions_applied,
            role_bound: data.management_permissions_applied,
            impersonation_granted: data.management_permissions_applied,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
