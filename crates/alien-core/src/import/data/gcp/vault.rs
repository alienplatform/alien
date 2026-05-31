use serde::{Deserialize, Serialize};

/// GCP Vault ImportData — Secret Manager namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpVaultImportData {
    /// Project ID containing Secret Manager.
    pub project_id: String,
    /// Prefix used for secret names owned by this vault.
    pub secret_prefix: String,
}
