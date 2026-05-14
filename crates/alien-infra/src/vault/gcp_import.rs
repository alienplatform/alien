//! Importer for GCP Vault (Secret Manager namespace).

use alien_core::{
    import::{data::GcpVaultImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::vault::{GcpVaultController, GcpVaultState};

/// GCP Secret Manager vault importer.
#[derive(Debug, Default)]
pub struct GcpVaultImporter;

impl ResourceImporter for GcpVaultImporter {
    type ImportData = GcpVaultImportData;

    fn import(
        &self,
        data: GcpVaultImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = GcpVaultController {
            state: GcpVaultState::Ready,
            project_id: Some(data.project_id),
            // Secret Manager is global on GCP; the controller's `location`
            // field is informational and stays unset at import time.
            location: None,
            vault_prefix: Some(data.secret_prefix),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
