use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// AWS Build ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsBuildImportData {
    /// CodeBuild project name.
    pub project_name: String,
    /// CodeBuild project ARN.
    pub project_arn: String,
    /// Environment variables stamped into the CodeBuild project.
    pub build_env_vars: HashMap<String, String>,
}
