use super::{AwsRemoteBindingsController, AwsRemoteBindingsState};
use crate::{import::ResourceImporter, import_helpers::make_imported_state};
use alien_core::{
    import::{data::AwsRemoteBindingsImportData, ImportContext},
    Result, StackResourceState,
};

#[derive(Debug, Default)]
pub struct AwsRemoteBindingsImporter;

impl ResourceImporter for AwsRemoteBindingsImporter {
    type ImportData = AwsRemoteBindingsImportData;
    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        make_imported_state(
            AwsRemoteBindingsController {
                state: AwsRemoteBindingsState::Ready,
                role_arn: Some(data.role_arn),
                role_name: Some(data.role_name),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
