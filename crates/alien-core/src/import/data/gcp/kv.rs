use serde::{Deserialize, Serialize};

/// GCP KV ImportData — a Firestore database in Datastore mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpKvImportData {
    /// Project ID containing the database.
    pub project_id: String,
    /// Firestore database ID (`(default)` when this is the default database).
    pub database_id: String,
    /// Firestore database location (region or multi-region code).
    pub location: String,
}
