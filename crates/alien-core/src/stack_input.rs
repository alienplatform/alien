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

impl StackInputDefinition {
    /// A deployer-provided boolean input, the shape `.enabled(input)` gates
    /// on. With a default the input is optional; without one it is required,
    /// since an unanswered gate would leave the resource's existence
    /// undecided. This is the Rust-side seam gated-stack tests build inputs
    /// with.
    #[doc(hidden)]
    pub fn deployer_boolean(
        id: &str,
        label: &str,
        description: &str,
        default: Option<bool>,
    ) -> Self {
        Self {
            id: id.to_string(),
            kind: StackInputKind::Boolean,
            provided_by: vec![StackInputProvider::Deployer],
            required: default.is_none(),
            label: label.to_string(),
            description: description.to_string(),
            placeholder: None,
            default: default.map(StackInputDefaultValue::Boolean),
            platforms: None,
            validation: None,
            env: Vec::new(),
        }
    }
}

/// Finds the boolean deployer input a gate references, for render-time
/// re-validation. The compile-time preflight enforces the same two rules;
/// generators repeat them so a caller that renders without preflights cannot
/// ship a template whose gate variable is undeclared or non-boolean.
pub fn find_boolean_gate_input<'a>(
    inputs: &'a [StackInputDefinition],
    input_id: &str,
) -> Result<&'a StackInputDefinition, GateInputIssue> {
    let input = inputs
        .iter()
        .find(|input| input.id == input_id)
        .ok_or(GateInputIssue::Undeclared)?;
    if input.kind != StackInputKind::Boolean {
        return Err(GateInputIssue::NotBoolean(input.kind.clone()));
    }
    Ok(input)
}

/// Why a gate input failed render-time validation; the caller owns the
/// backend-specific error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateInputIssue {
    /// The stack declares no input with the gate's id.
    Undeclared,
    /// The input exists but is not a boolean.
    NotBoolean(StackInputKind),
}
