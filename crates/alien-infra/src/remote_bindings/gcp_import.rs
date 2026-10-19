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
        data.federation.validate()?;
        if data.federation.service_account_email != data.service_account_email {
            return Err(alien_error::AlienError::new(
                alien_core::ErrorData::GcpFederationInvalid {
                    field: "federation".to_string(),
                    reason: "must identify the imported published-resource account".to_string(),
                },
            ));
        }
        let federation_setup =
            crate::gcp_federation::GcpFederationSetup::from_access(&data.federation)?;
        make_imported_state(
            GcpRemoteBindingsController {
                state: GcpRemoteBindingsState::Ready,
                service_account_email: Some(data.service_account_email),
                service_account_unique_id: Some(data.service_account_unique_id),
                federation_setup,
                federation_access: Some(data.federation),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
