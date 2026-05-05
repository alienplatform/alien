//! Importer for GCP ServiceAccount.

use alien_core::{
    import::{data::GcpServiceAccountImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::service_account::GcpServiceAccountController;

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
        use crate::service_account::gcp::GcpServiceAccountState;

        // `stack_permissions_applied` lights up `role_created` since the
        // GCP controller binds its custom role unconditionally during
        // create — the two flags carry the same signal.
        let _ = data.project_id;
        let controller = GcpServiceAccountController {
            state: GcpServiceAccountState::Ready,
            service_account_email: Some(data.service_account_email),
            service_account_unique_id: Some(data.service_account_unique_id),
            custom_role_name: None,
            role_created: data.stack_permissions_applied,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
