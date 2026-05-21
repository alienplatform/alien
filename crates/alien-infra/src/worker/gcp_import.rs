//! Importer for GCP Worker (Cloud Run service).

use alien_core::{
    import::{data::GcpWorkerImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::worker::{GcpWorkerController, GcpWorkerState};

/// GCP Cloud Run worker importer.
///
/// The bulk of the controller's load-balancer / TLS state (`backend_service_name`,
/// `target_https_proxy_name`, etc.) is reconstructed at heartbeat time from
/// the resource's deployment-config metadata; the import payload only carries
/// stable identifiers (service name, URL, trigger names).
#[derive(Debug, Default)]
pub struct GcpWorkerImporter;

impl ResourceImporter for GcpWorkerImporter {
    type ImportData = GcpWorkerImportData;

    fn import(
        &self,
        data: GcpWorkerImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // GCP storage triggers fan out across Eventarc → Pub/Sub topic → GCS
        // notification. The import payload carries the Eventarc trigger
        // names; the topic and notification IDs are reconstructed at first
        // reconcile so we leave the corresponding fields empty.
        let _ = data.eventarc_trigger_names;
        let controller = GcpWorkerController {
            state: GcpWorkerState::Ready,
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
            global_address_ip: None,
            forwarding_rule_name: None,
            project_id: Some(data.project_id),
            region: Some(data.region),
            commands_topic_name: data.commands_topic_name,
            commands_subscription_name: data.commands_subscription_name,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
