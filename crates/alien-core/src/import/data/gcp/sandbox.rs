use serde::{Deserialize, Serialize};

/// GCP Agent Platform sandbox ImportData.
///
/// The sandbox is the environment template, which is release-owned under either lifecycle: setup
/// creates no template, so this carries the region the stack applied in and, when preview is declared, the
/// signing identity setup created. The engine registers its own id, and the template controller
/// reads it from that dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpSandboxImportData {
    /// Region the setup stack applied in; selects the regional Agent Platform endpoint.
    pub region: String,
    /// Service account setup created to sign the edge credential for preview. Present only when
    /// the sandbox declares preview ports; the controller carries it into the binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_service_account: Option<String>,
}
