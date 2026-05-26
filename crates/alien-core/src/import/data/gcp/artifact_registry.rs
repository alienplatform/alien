use serde::{Deserialize, Serialize};

/// GCP ArtifactRegistry ImportData — an Artifact Registry repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpArtifactRegistryImportData {
    /// Project ID hosting the registry.
    pub project_id: String,
    /// Region where the registry lives.
    pub region: String,
    /// Repository ID (the short name).
    pub repository_id: String,
    /// Full repository resource name
    /// (`projects/<id>/locations/<region>/repositories/<repo>`).
    pub repository_name: String,
    /// Docker registry endpoint
    /// (`<region>-docker.pkg.dev/<project>/<repo>`).
    pub registry_endpoint: String,
    /// Service account email with pull-only access.
    pub pull_service_account_email: String,
    /// Service account email with push and pull access.
    pub push_service_account_email: String,
}
