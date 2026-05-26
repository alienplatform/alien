use serde::{Deserialize, Serialize};

/// Azure Queue ImportData — a Service Bus queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureQueueImportData {
    /// Subscription ID containing the namespace.
    pub subscription_id: String,
    /// Resource group containing the namespace.
    pub resource_group: String,
    /// Service Bus namespace name.
    pub namespace_name: String,
    /// Queue name within the namespace.
    pub queue_name: String,
}
