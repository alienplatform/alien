use serde::{Deserialize, Serialize};

/// GCP ServiceActivation ImportData — a `google_project_service`
/// enabling a single GCP API on the customer's project.
///
/// Added by the `GcpServiceActivationMutation` preflight when the
/// stack contains resources that need a specific API enabled
/// (Cloud Run, Pub/Sub, Firestore, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpServiceActivationImportData {
    /// Project ID the service was enabled on.
    pub project_id: String,
    /// Fully-qualified service name (e.g. `run.googleapis.com`).
    pub service_name: String,
    /// True once the service is reported enabled.
    pub activated: bool,
}
