use super::{GcpRemoteBindingsController, GcpRemoteBindingsState};
use crate::{import::ResourceImporter, import_helpers::make_imported_state};
use alien_core::{
    import::{data::GcpRemoteBindingsImportData, ImportContext},
    Result, StackResourceState,
};

#[derive(Debug, Default)]
pub struct GcpRemoteBindingsImporter;
impl ResourceImporter for GcpRemoteBindingsImporter {
    type ImportData = GcpRemoteBindingsImportData;
    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        make_imported_state(
            GcpRemoteBindingsController {
                state: GcpRemoteBindingsState::Ready,
                service_account_email: Some(data.service_account_email),
                service_account_unique_id: Some(data.service_account_unique_id),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
