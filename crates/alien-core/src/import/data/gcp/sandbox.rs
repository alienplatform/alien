use serde::{Deserialize, Serialize};

/// GCP Sandbox ImportData.
///
/// A Cloud Run sandbox has no durable parent: it is a nested gVisor sandbox started by a launcher
/// binary Cloud Run injects into the container, so there is no group, image or endpoint for setup
/// to hand over. What the runtime needs is the launcher's path, and it is carried here rather than
/// hardcoded in the provider so a change to where Cloud Run mounts it is a data change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpSandboxImportData {
    /// Path to the sandbox CLI Cloud Run injects when the container sets `sandboxLauncher`.
    pub launcher_path: String,
    /// Whether sessions may reach the network. Taken from the declaration rather than left to the
    /// application: the launcher decides egress per sandbox at create time.
    pub allow_egress: bool,
}
