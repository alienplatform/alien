//! Importer for GCP RemoteStackManagement (cross-project SA).

use alien_core::{
    Result, StackResourceState,
    import::{ImportContext, data::GcpRemoteStackManagementImportData},
};

use crate::core::GcpCustomRoleNaming;
use crate::import::ResourceImporter;
use crate::import_helpers::{imported_gcp_role_naming, make_imported_state};
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
        let naming = imported_gcp_role_naming(data.custom_role_namespace, ctx)?
            .unwrap_or(GcpCustomRoleNaming::HashedLongPrefix);
        let _ = data.project_id;
        let controller = GcpRemoteStackManagementController {
            state: GcpRemoteStackManagementState::Ready,
            service_account_email: Some(data.service_account_email),
            service_account_unique_id: Some(data.service_account_unique_id),
            role_bound: data.management_permissions_applied,
            impersonation_granted: data.management_permissions_applied,
            custom_role_naming: Some(naming),
            management_permissions_revision: None,
            installed_management_grants: data
                .installed_management_grants
                .filter(|_| data.management_permissions_applied),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
