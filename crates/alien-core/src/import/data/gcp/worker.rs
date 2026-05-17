use serde::{Deserialize, Serialize};

/// GCP Worker ImportData — a Cloud Run service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpWorkerImportData {
    /// Project ID hosting the service.
    pub project_id: String,
    /// Region where the service runs.
    pub region: String,
    /// Cloud Run service name.
    pub service_name: String,
    /// Public HTTPS URL when public ingress is enabled.
    pub url: Option<String>,
    /// Pub/Sub push-subscription names created for queue triggers.
    pub pubsub_subscription_names: Vec<String>,
    /// Cloud Scheduler job names created for schedule triggers.
    pub scheduler_job_names: Vec<String>,
    /// Eventarc trigger names created for storage triggers.
    pub eventarc_trigger_names: Vec<String>,
    /// Pub/Sub topic short name created for commands delivery.
    pub commands_topic_name: Option<String>,
    /// Pub/Sub push-subscription name created for commands delivery.
    pub commands_subscription_name: Option<String>,
}
