//! Importer for GCP ServiceAccount.

use alien_core::{
    Result, StackResourceState,
    import::{ImportContext, data::GcpServiceAccountImportData},
};

use crate::core::GcpCustomRoleNaming;
use crate::import::ResourceImporter;
use crate::import_helpers::{imported_gcp_role_naming, make_imported_state};
use crate::service_account::{GcpServiceAccountController, gcp::GcpServiceAccountState};

/// GCP service account importer.
#[derive(Debug, Default)]
pub struct GcpServiceAccountImporter;

impl ResourceImporter for GcpServiceAccountImporter {
    type ImportData = GcpServiceAccountImportData;

    fn import(
        &self,
        data: GcpServiceAccountImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let naming = imported_gcp_role_naming(data.custom_role_namespace, ctx)?
            .unwrap_or(GcpCustomRoleNaming::HashedLongPrefix);
        let _ = data.project_id;
        let _ = data.stack_permissions_applied;
        let controller = GcpServiceAccountController {
            state: GcpServiceAccountState::Ready,
            service_account_email: Some(data.service_account_email),
            service_account_unique_id: Some(data.service_account_unique_id),
            custom_role_naming: Some(naming),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
