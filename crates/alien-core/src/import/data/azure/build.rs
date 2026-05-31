use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Azure Build ImportData — an Azure Container Registry build task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureBuildImportData {
    /// Subscription ID hosting the build task.
    pub subscription_id: String,
    /// Resource group hosting the registry.
    pub resource_group: String,
    /// Azure Container Registry name.
    pub registry_name: String,
    /// Build task name.
    pub task_name: String,
    /// Environment variables stamped into the task's variables block.
    pub build_env_vars: HashMap<String, String>,
}
