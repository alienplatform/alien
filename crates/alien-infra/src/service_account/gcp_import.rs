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

        let _ = data.project_id;
        let _ = data.stack_permissions_applied;
        let controller = GcpServiceAccountController {
            state: GcpServiceAccountState::Ready,
            service_account_email: Some(data.service_account_email),
            service_account_unique_id: Some(data.service_account_unique_id),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
