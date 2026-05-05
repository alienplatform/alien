//! Importer for AWS ServiceAccount (IAM role).

use alien_core::{
    import::{data::AwsServiceAccountImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::service_account::AwsServiceAccountController;

/// AWS IAM role importer.
#[derive(Debug, Default)]
pub struct AwsServiceAccountImporter;

impl ResourceImporter for AwsServiceAccountImporter {
    type ImportData = AwsServiceAccountImportData;

    fn import(
        &self,
        data: AwsServiceAccountImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        use crate::service_account::aws::AwsServiceAccountState;

        let controller = AwsServiceAccountController {
            state: AwsServiceAccountState::Ready,
            role_arn: Some(data.role_arn),
            role_name: Some(data.role_name),
            stack_permissions_applied: data.stack_permissions_applied,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
