use serde::{Deserialize, Serialize};

/// Azure KV ImportData — a Cosmos DB account in NoSQL mode plus its
/// container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureKvImportData {
    /// Subscription ID containing the Cosmos DB account.
    pub subscription_id: String,
    /// Resource group containing the Cosmos DB account.
    pub resource_group: String,
    /// Cosmos DB account name.
    pub account_name: String,
    /// Database name.
    pub database_name: String,
    /// Container name within the database.
    pub container_name: String,
    /// Primary endpoint URL.
    pub endpoint: String,
}
