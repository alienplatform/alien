use serde::{Deserialize, Serialize};

/// GCP Queue ImportData — a Pub/Sub topic plus subscription created in
/// the customer's project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpQueueImportData {
    /// Project ID containing the topic.
    pub project_id: String,
    /// Pub/Sub topic ID (short name).
    pub topic_id: String,
    /// Pub/Sub topic full resource name (`projects/<id>/topics/<name>`).
    pub topic_name: String,
    /// Default subscription ID created alongside the topic.
    pub subscription_id: String,
    /// Subscription full resource name.
    pub subscription_name: String,
}
