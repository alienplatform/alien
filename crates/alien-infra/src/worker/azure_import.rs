//! Importer for Azure Worker (Container App).

use alien_core::{
    import::{data::AzureWorkerImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::worker::{AzureWorkerController, AzureWorkerState};

/// Azure Container App worker importer.
///
/// `ImportData` carries only the stable identifiers (subscription, resource
/// group, container app name, public fqdn). Domain/certificate state, Dapr
/// components, commands infrastructure, and the ARM resource id are
/// rebuilt by the controller's heartbeat path on first reconcile.
#[derive(Debug, Default)]
pub struct AzureWorkerImporter;

impl ResourceImporter for AzureWorkerImporter {
    type ImportData = AzureWorkerImportData;

    fn import(
        &self,
        data: AzureWorkerImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let _ = (data.subscription_id, data.resource_group);
        let controller = AzureWorkerController {
            state: AzureWorkerState::Ready,
            container_app_name: Some(data.container_app_name),
            resource_id: None,
            url: data.fqdn.as_ref().map(|f| format!("https://{}", f)),
            // The ingress host (CNAME target) isn't in ImportData; the heartbeat handler rebuilds it.
            container_app_url: None,
            pending_operation_url: None,
            pending_operation_retry_after: None,
            dapr_components: Vec::new(),
            fqdn: data.fqdn,
            certificate_id: None,
            keyvault_cert_id: None,
            container_apps_certificate_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            commands_namespace_name: None,
            commands_queue_name: None,
            commands_dapr_component: None,
            commands_sender_role_assignment_id: None,
            commands_receiver_role_assignment_id: None,
            commands_infrastructure_auth_wait_until_epoch_secs: None,
            container_apps_environment_wake_wait_until_epoch_secs: None,
            container_apps_environment_wake_retry_after_epoch_secs: None,
            pre_container_app_rbac_wait_until_epoch_secs: None,
            ready_rbac_wait_until_epoch_secs: None,
            update_rbac_wait_required: false,
            update_dapr_components_deleted: false,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
