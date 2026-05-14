//! Importer for Azure ServiceActivation (resource provider registration).

use alien_core::{
    import::{data::AzureServiceActivationImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::service_activation::{AzureServiceActivationController, AzureServiceActivationState};

/// Azure resource-provider activation importer.
#[derive(Debug, Default)]
pub struct AzureServiceActivationImporter;

impl ResourceImporter for AzureServiceActivationImporter {
    type ImportData = AzureServiceActivationImportData;

    fn import(
        &self,
        data: AzureServiceActivationImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = data.subscription_id;
        let controller = AzureServiceActivationController {
            state: AzureServiceActivationState::Ready,
            service_name: Some(data.provider_namespace),
            service_activated: data.registered,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
