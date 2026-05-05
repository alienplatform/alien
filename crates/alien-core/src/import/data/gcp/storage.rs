use serde::{Deserialize, Serialize};

/// GCP Storage ImportData — a Cloud Storage bucket created in the
/// customer's project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpStorageImportData {
    /// Cloud Storage bucket name.
    pub bucket_name: String,
    /// Bucket self-link.
    pub bucket_self_link: String,
    /// Project ID containing the bucket.
    pub project_id: String,
    /// Bucket location (region or multi-region code).
    pub location: String,
}
