use serde::{Deserialize, Serialize};

/// GCP Agent Platform sandbox ImportData.
///
/// The sandbox is the environment template, which is release-owned under either lifecycle: setup
/// creates no template, so this carries only the region the stack applied in. The engine registers
/// its own id, and the template controller reads it from that dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpSandboxImportData {
    /// Region the setup stack applied in; selects the regional Agent Platform endpoint.
    pub region: String,
}
