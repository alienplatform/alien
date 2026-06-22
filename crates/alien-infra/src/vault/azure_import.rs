//! Importer for Azure Vault (Key Vault).

use alien_core::{
    import::{data::AzureVaultImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::vault::{AzureVaultController, AzureVaultState};

/// Azure Key Vault importer.
#[derive(Debug, Default)]
pub struct AzureVaultImporter;

impl ResourceImporter for AzureVaultImporter {
    type ImportData = AzureVaultImportData;

    fn import(
        &self,
        data: AzureVaultImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = data.subscription_id;
        let controller = AzureVaultController {
            state: AzureVaultState::Ready,
            vault_name: Some(data.vault_name),
            resource_group_name: Some(data.resource_group),
            vault_uri: Some(data.vault_uri),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
