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

/// Shortest value Alien generates for a secret input.
pub const STACK_INPUT_GENERATE_MIN_LENGTH: u32 = 16;
/// Longest value Alien generates for a secret input.
pub const STACK_INPUT_GENERATE_MAX_LENGTH: u32 = 256;

/// Asks Alien to generate a secret input's value.
///
/// The value is an alphanumeric string (`A-Z`, `a-z`, `0-9`), so it is safe in
/// connection strings, command lines and environment variables. It is generated
/// once, when the deployment's input values are first resolved, and then kept
/// with the deployment's other input values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackInputGenerate {
    /// Number of characters to generate.
    pub length: u32,
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
    /// Alien generates the value when none is provided. Secret,
    /// developer-provided inputs only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate: Option<StackInputGenerate>,
    /// Runtime env-var mappings for v1 input resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<StackInputEnvironmentMapping>,
}

impl StackInputDefinition {
    /// Whether Alien generates this input's value when none is provided. A
    /// generated input never needs a value from the developer or deployer.
    pub fn is_generated(&self) -> bool {
        self.generate.is_some()
    }

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
            generate: None,
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

/// Environment variables produced by stack inputs that declare `env`
/// mappings, from the deployment's input values (or each input's default).
///
/// Only inputs that apply to `platform` count: an input that lists
/// `platforms` without it produces nothing. List values become
/// comma-separated strings. Secret inputs produce secret variables unless the
/// mapping sets a type. Two mappings resolving to the same variable name are
/// rejected.
pub fn resolve_stack_input_environment_variables(
    inputs: &[StackInputDefinition],
    values: &std::collections::HashMap<String, serde_json::Value>,
    platform: Platform,
) -> crate::Result<Vec<crate::EnvironmentVariable>> {
    let mut variables: Vec<crate::EnvironmentVariable> = Vec::new();
    let applies = |input: &&StackInputDefinition| match &input.platforms {
        Some(platforms) if !platforms.is_empty() => platforms.contains(&platform),
        _ => true,
    };
    for input in inputs
        .iter()
        .filter(|input| !input.env.is_empty())
        .filter(applies)
    {
        let value = match values.get(&input.id) {
            Some(serde_json::Value::Null) | None => match &input.default {
                Some(default) => default_environment_string(default),
                None => continue,
            },
            Some(value) => environment_string(value),
        };
        for mapping in &input.env {
            if variables
                .iter()
                .any(|variable| variable.name == mapping.name)
            {
                return Err(alien_error::AlienError::new(
                    crate::ErrorData::GenericError {
                        message: format!(
                            "Stack inputs map more than one value to environment variable '{}'",
                            mapping.name
                        ),
                    },
                ));
            }
            let secret = match mapping.var_type {
                Some(StackInputEnvironmentVariableType::Secret) => true,
                Some(StackInputEnvironmentVariableType::Plain) => false,
                None => input.kind == StackInputKind::Secret,
            };
            variables.push(crate::EnvironmentVariable {
                name: mapping.name.clone(),
                value: value.clone(),
                var_type: if secret {
                    crate::EnvironmentVariableType::Secret
                } else {
                    crate::EnvironmentVariableType::Plain
                },
                target_resources: mapping.target_resources.clone(),
            });
        }
    }
    Ok(variables)
}

fn environment_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(environment_string)
            .collect::<Vec<_>>()
            .join(","),
        other => other.to_string(),
    }
}

fn default_environment_string(default: &StackInputDefaultValue) -> String {
    match default {
        StackInputDefaultValue::String(value) | StackInputDefaultValue::Number(value) => {
            value.clone()
        }
        StackInputDefaultValue::Boolean(value) => value.to_string(),
        StackInputDefaultValue::StringList(values) => values.join(","),
    }
}

#[cfg(test)]
mod environment_tests {
    use std::collections::HashMap;

    use super::*;
    use crate::EnvironmentVariableType;

    fn input(id: &str, kind: StackInputKind, env: &str) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind,
            provided_by: vec![StackInputProvider::Developer],
            required: false,
            label: id.to_string(),
            description: id.to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            env: vec![StackInputEnvironmentMapping {
                name: env.to_string(),
                target_resources: Some(vec!["api".to_string()]),
                var_type: None,
            }],
        }
    }

    #[test]
    fn maps_values_defaults_and_secrecy() {
        let mut region = input("region", StackInputKind::String, "REGION");
        region.default = Some(StackInputDefaultValue::String("eu-west-1".to_string()));
        let inputs = vec![
            input("token", StackInputKind::Secret, "ACCESS_TOKEN"),
            input("zones", StackInputKind::StringList, "ZONES"),
            region,
            input("unset", StackInputKind::String, "UNSET"),
        ];
        let values = HashMap::from([
            ("token".to_string(), serde_json::json!("s3cr3t")),
            ("zones".to_string(), serde_json::json!(["a", "b"])),
        ]);

        let variables =
            resolve_stack_input_environment_variables(&inputs, &values, Platform::Kubernetes)
                .unwrap();

        let by_name: HashMap<_, _> = variables.iter().map(|v| (v.name.as_str(), v)).collect();
        assert_eq!(
            variables.len(),
            3,
            "unset inputs without defaults produce nothing"
        );
        assert_eq!(by_name["ACCESS_TOKEN"].value, "s3cr3t");
        assert_eq!(
            by_name["ACCESS_TOKEN"].var_type,
            EnvironmentVariableType::Secret
        );
        assert_eq!(
            by_name["ACCESS_TOKEN"].target_resources,
            Some(vec!["api".to_string()])
        );
        assert_eq!(by_name["ZONES"].value, "a,b");
        assert_eq!(by_name["ZONES"].var_type, EnvironmentVariableType::Plain);
        assert_eq!(by_name["REGION"].value, "eu-west-1");
    }

    #[test]
    fn rejects_two_inputs_mapped_to_one_variable() {
        let inputs = vec![
            input("a", StackInputKind::String, "SHARED"),
            input("b", StackInputKind::String, "SHARED"),
        ];
        let values = HashMap::from([
            ("a".to_string(), serde_json::json!("1")),
            ("b".to_string(), serde_json::json!("2")),
        ]);
        let error =
            resolve_stack_input_environment_variables(&inputs, &values, Platform::Kubernetes)
                .expect_err("duplicate names must be rejected");
        assert!(error.message.contains("SHARED"));
    }

    #[test]
    fn inputs_for_other_platforms_produce_nothing() {
        // An AWS-only input with a default must not reach a Kubernetes
        // workload, and may share a variable name with a Kubernetes one.
        let mut aws_only = input("aws-region", StackInputKind::String, "REGION");
        aws_only.platforms = Some(vec![Platform::Aws]);
        aws_only.default = Some(StackInputDefaultValue::String("us-east-1".to_string()));
        let mut kubernetes_only = input("zone", StackInputKind::String, "REGION");
        kubernetes_only.platforms = Some(vec![Platform::Kubernetes]);
        let values = HashMap::from([("zone".to_string(), serde_json::json!("rack-7"))]);

        let variables = resolve_stack_input_environment_variables(
            &[aws_only, kubernetes_only],
            &values,
            Platform::Kubernetes,
        )
        .unwrap();

        assert_eq!(variables.len(), 1);
        assert_eq!(variables[0].name, "REGION");
        assert_eq!(variables[0].value, "rack-7");
    }
}
