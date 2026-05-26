use serde::{Deserialize, Serialize};

/// Azure ServiceBusNamespace ImportData — parent namespace for Queue
/// resources. Realized once per stack and shared across `Queue`s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureServiceBusNamespaceImportData {
    /// Subscription ID containing the namespace.
    pub subscription_id: String,
    /// Resource group containing the namespace.
    pub resource_group: String,
    /// Service Bus namespace name (globally unique).
    pub namespace_name: String,
    /// Endpoint hostname (`<namespace>.servicebus.windows.net`).
    pub endpoint: String,
}
