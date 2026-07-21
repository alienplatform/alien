//! Importer for GCP RemoteStackManagement (cross-project SA).

use alien_core::{
    import::{data::GcpRemoteStackManagementImportData, ImportContext},
    ResourceStatus, Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state_with_status;
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
            // Force one ownership-establishing reconciliation before Running.
            state: GcpRemoteStackManagementState::UpdateStart,
            service_account_email: Some(data.service_account_email),
            service_account_unique_id: Some(data.service_account_unique_id),
            role_bound: data.management_permissions_applied,
            impersonation_granted: data.management_permissions_applied,
            applied_management_grant_fingerprint: None,
            remote_storage_bucket_names: Vec::new(),
            _internal_stay_count: None,
        };
        make_imported_state_with_status(controller, ctx, ResourceStatus::Updating)
    }
}
