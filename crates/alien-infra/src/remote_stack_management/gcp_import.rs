//! Importer for GCP RemoteStackManagement (cross-project SA).

use alien_core::{
    import::{data::GcpRemoteStackManagementImportData, ImportContext},
    Result, StackResourceState,
};

use crate::core::GcpCustomRoleNaming;
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::remote_stack_management::{
    GcpRemoteStackManagementController, GcpRemoteStackManagementState,
};

/// GCP cross-project management service account importer.
#[derive(Debug, Default)]
pub struct GcpRemoteStackManagementImporter;

impl ResourceImporter for GcpRemoteStackManagementImporter {
    type ImportData = GcpRemoteStackManagementImportData;

    fn import(
        &self,
        data: GcpRemoteStackManagementImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        data.federation.validate()?;
        if data.federation.service_account_email != data.service_account_email
            || data
                .project_number
                .as_ref()
                .is_some_and(|number| number != &data.federation.project_number)
        {
            return Err(alien_error::AlienError::new(
                alien_core::ErrorData::GcpFederationInvalid {
                    field: "federation".to_string(),
                    reason: "must identify the imported management account and project".to_string(),
                },
            ));
        }
        let federation_setup =
            crate::gcp_federation::GcpFederationSetup::from_access(&data.federation)?;
        let _ = data.project_id;
        let controller = GcpRemoteStackManagementController {
            state: GcpRemoteStackManagementState::Ready,
            service_account_email: Some(data.service_account_email),
            service_account_unique_id: Some(data.service_account_unique_id),
            role_bound: data.management_permissions_applied,
            federation_granted: data.management_permissions_applied,
            federation_setup,
            federation_access: Some(data.federation),
            custom_role_naming: Some(GcpCustomRoleNaming::HashedLongPrefix),
            // Setup was applied by an external engine, which refreshes it itself.
            management_permissions_revision: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
