use super::{AzureRemoteBindingsController, AzureRemoteBindingsState};
use crate::{import::ResourceImporter, import_helpers::make_imported_state};
use alien_core::{
    import::{data::AzureRemoteBindingsImportData, ImportContext},
    Result, StackResourceState,
};

#[derive(Debug, Default)]
pub struct AzureRemoteBindingsImporter;
impl ResourceImporter for AzureRemoteBindingsImporter {
    type ImportData = AzureRemoteBindingsImportData;
    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        make_imported_state(
            AzureRemoteBindingsController {
                state: AzureRemoteBindingsState::Ready,
                identity_resource_id: Some(data.identity_id),
                client_id: Some(data.client_id),
                principal_id: Some(data.principal_id),
                tenant_id: Some(data.tenant_id),
                fic_name: None,
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
