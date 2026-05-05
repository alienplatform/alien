//! Importer for GCP Function (Cloud Run service).

use alien_core::{
    import::{data::GcpFunctionImportData, ImportContext},
    Function, Ingress, Result, StackResourceState,
};

use crate::function::{GcpFunctionController, GcpFunctionState};
use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;

/// GCP Cloud Run function importer.
///
/// The bulk of the controller's load-balancer / TLS state (`backend_service_name`,
/// `target_https_proxy_name`, etc.) is reconstructed at heartbeat time from
/// the resource's deployment-config metadata; the import payload only carries
/// stable identifiers (service name, URL, trigger names).
#[derive(Debug, Default)]
pub struct GcpFunctionImporter;

impl ResourceImporter for GcpFunctionImporter {
    type ImportData = GcpFunctionImportData;

    fn import(
        &self,
        data: GcpFunctionImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // GCP storage triggers fan out across Eventarc → Pub/Sub topic → GCS
        // notification. The import payload carries the Eventarc trigger
        // names; the topic and notification IDs are reconstructed at first
        // reconcile so we leave the corresponding fields empty.
        let _ = data.eventarc_trigger_names;
        let is_public = ctx
            .resource
            .config
            .downcast_ref::<Function>()
            .map(|function| function.ingress == Ingress::Public)
            .unwrap_or_else(|| data.url.is_some());

        let controller = GcpFunctionController {
            state: if is_public {
                GcpFunctionState::WaitingForCertificate
            } else {
                GcpFunctionState::Ready
            },
            service_name: Some(data.service_name),
            url: data.url,
            operation_name: None,
            push_subscriptions: data.pubsub_subscription_names,
            storage_notification_topics: Vec::new(),
            gcs_notification_ids: Vec::new(),
            scheduler_job_names: data.scheduler_job_names,
            fqdn: None,
            certificate_id: None,
            ssl_certificate_name: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            serverless_neg_name: None,
            backend_service_name: None,
            url_map_name: None,
            target_https_proxy_name: None,
            global_address_name: None,
            forwarding_rule_name: None,
            project_id: Some(data.project_id),
            region: Some(data.region),
            commands_topic_name: None,
            commands_subscription_name: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
