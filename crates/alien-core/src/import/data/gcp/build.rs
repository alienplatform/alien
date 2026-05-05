use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// GCP Build ImportData — Cloud Build trigger + worker pool reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpBuildImportData {
    /// Project ID hosting the trigger.
    pub project_id: String,
    /// Region where the trigger runs.
    pub region: String,
    /// Cloud Build trigger ID.
    pub trigger_id: String,
    /// Cloud Build trigger name.
    pub trigger_name: String,
    /// Environment variables stamped into the trigger's substitutions.
    pub build_env_vars: HashMap<String, String>,
}
