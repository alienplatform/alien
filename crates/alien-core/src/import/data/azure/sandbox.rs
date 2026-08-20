use serde::{Deserialize, Serialize};

/// Azure Sandbox ImportData.
///
/// Carries the sandbox group from the setup emitter to the runtime controller. All three fields
/// are required to address it: the ADC data plane endpoint is **per-region**, so a group without
/// its region cannot be reached at all, and the data plane path is scoped by resource group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureSandboxImportData {
    /// Sandbox group name.
    pub sandbox_group: String,
    /// Region the group lives in; selects the ADC endpoint.
    pub region: String,
    /// Resource group containing the sandbox group.
    pub resource_group: String,
}
