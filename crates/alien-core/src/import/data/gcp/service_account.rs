use serde::{Deserialize, Serialize};

/// GCP ServiceAccount ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpServiceAccountImportData {
    /// Project ID containing the service account.
    pub project_id: String,
    /// Service account email (`<name>@<project>.iam.gserviceaccount.com`).
    pub service_account_email: String,
    /// Stable unique id Google assigns to the service account.
    pub service_account_unique_id: String,
    /// Whether stack-level permissions were attached by the generated stack.
    pub stack_permissions_applied: bool,
}
