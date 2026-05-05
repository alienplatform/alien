//! Importer for AWS Vault (Parameter Store SecureString namespace).

use alien_core::{
    import::{data::AwsVaultImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::vault::{AwsVaultController, AwsVaultState};

/// AWS Parameter Store vault importer.
#[derive(Debug, Default)]
pub struct AwsVaultImporter;

impl ResourceImporter for AwsVaultImporter {
    type ImportData = AwsVaultImportData;

    fn import(
        &self,
        data: AwsVaultImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = AwsVaultController {
            state: AwsVaultState::Ready,
            account_id: Some(data.account_id),
            region: Some(data.region),
            vault_prefix: Some(data.parameter_prefix),
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
