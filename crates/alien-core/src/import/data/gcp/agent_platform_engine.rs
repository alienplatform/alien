use serde::{Deserialize, Serialize};

/// GCP Agent Platform reasoning-engine ImportData.
///
/// Emitted only for a Frozen engine, which the setup stack creates. Vertex assigns the id, so it
/// is read off the created resource rather than derived: the controller and the template beneath
/// it address the engine by exactly this segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpAgentPlatformEngineImportData {
    /// The engine's resource name, or the bare id it ends with.
    pub engine_id: String,
}
