use serde::{Deserialize, Serialize};

use crate::Platform;

/// Who can provide a stack input value.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum StackInputProvider {
    /// Value is provided by the developer before a deployment link is created.
    Developer,
    /// Value is provided by the deployer during setup.
    Deployer,
}

/// Primitive stack input kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum StackInputKind {
    /// Plain string input.
    String,
    /// Secret string input.
    Secret,
    /// Floating point number input.
    Number,
    /// Integer input.
    Integer,
    /// Boolean input.
    Boolean,
    /// String enum input.
    Enum,
    /// List of strings.
    StringList,
}

/// How a resolved stack input is injected into runtime environment variables.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackInputEnvironmentMapping {
    /// Environment variable name.
    pub name: String,
    /// Target resource IDs or patterns. None means every env-capable resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_resources: Option<Vec<String>>,
    /// Whether this env var is plain or secret. Defaults from the input kind.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub var_type: Option<StackInputEnvironmentVariableType>,
}

/// Environment variable handling for a stack input mapping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum StackInputEnvironmentVariableType {
    /// Plain value injected directly.
    Plain,
    /// Secret value routed through secret handling.
    Secret,
}

/// Portable stack input validation constraints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackInputValidation {
    /// Minimum string length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,
    /// Maximum string length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    /// Portable whole-value regex pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Semantic format hint such as url.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Minimum number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    /// Maximum number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    /// Allowed string enum values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    /// Minimum string-list items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    /// Maximum string-list items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
}

/// Stack input default value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum StackInputDefaultValue {
    /// String default.
    String(String),
    /// Number default.
    Number(String),
    /// Boolean default.
    Boolean(bool),
    /// String list default.
    StringList(Vec<String>),
}

/// Stack input definition serialized into a release stack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackInputDefinition {
    /// Stable input ID used by CLI/API calls.
    pub id: String,
    /// Input primitive kind.
    pub kind: StackInputKind,
    /// Who can provide this value.
    pub provided_by: Vec<StackInputProvider>,
    /// Whether a resolved value is required before deployment can proceed.
    pub required: bool,
    /// Human-facing field label.
    pub label: String,
    /// Human-facing helper text.
    pub description: String,
    /// Example placeholder shown in UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Default value for optional/plain inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<StackInputDefaultValue>,
    /// Platforms where this input applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<Platform>>,
    /// Portable validation constraints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<StackInputValidation>,
    /// Runtime env-var mappings for v1 input resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<StackInputEnvironmentMapping>,
}
