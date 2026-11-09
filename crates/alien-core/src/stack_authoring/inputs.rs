//! Stack inputs as a stack file declares them.
//!
//! An [`AuthoredInput`] is one input as written (`kind`, `label`, `min-length`, ...). Turning
//! it into a [`StackInputDefinition`] applies the same rules the TypeScript SDK's `inputs()`
//! applies, so a stack carries the same definitions whichever frontend described it.
//!
//! Errors name the offending field as `input.<id>.<key>`, with the key spelled the way the
//! definition spells it (`minLength`), so a frontend can find it in the file.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{input_default, input_number, input_validation, invalid, InputValue};
use crate::error::Result;
use crate::{
    Platform, StackInputCondition, StackInputDefinition, StackInputEnvironmentMapping,
    StackInputEnvironmentVariableType, StackInputGenerate, StackInputKind, StackInputProvider,
    StackInputValidation, STACK_INPUT_GENERATE_MAX_LENGTH, STACK_INPUT_GENERATE_MIN_LENGTH,
};

/// One environment variable an input is delivered as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredEnvMapping {
    /// Environment variable name: uppercase letters, digits and underscores.
    pub name: String,
    /// Resource IDs or `prefix*` patterns that get the variable. Every workload when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_resources: Option<Vec<String>>,
    /// Whether the variable is plain or secret. Defaults from the input kind.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub var_type: Option<StackInputEnvironmentVariableType>,
}

/// A stack input as written in a stack file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredInput {
    /// Input primitive kind.
    pub kind: StackInputKind,
    /// Who can provide the value. The developer when unset.
    #[serde(default = "default_provided_by")]
    pub provided_by: Vec<StackInputProvider>,
    /// Whether a value is needed before deployment can proceed. True when unset.
    #[serde(default = "default_required")]
    pub required: bool,
    /// Human-facing field label.
    pub label: String,
    /// Helper text shown where the value is asked for. Required when the input is.
    #[serde(default)]
    pub description: String,
    /// Example shown in the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Value used when none is provided. Secrets have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<InputValue>,
    /// Only applies when a boolean input has this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<StackInputCondition>,
    /// Platforms where the input applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<Platform>>,
    /// Environment variables the value is delivered as.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<AuthoredEnvMapping>,
    /// Let Alien generate the value. Secret, developer-provided inputs only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate: Option<StackInputGenerate>,
    /// Shortest string, for string and secret inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,
    /// Longest string, for string and secret inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    /// Whole-value pattern, for string and secret inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Semantic format such as `url`, for string and secret inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Smallest value, for number and integer inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<InputValue>,
    /// Largest value, for number and integer inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<InputValue>,
    /// Accepted values, for enum inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    /// Fewest items, for string-list inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    /// Most items, for string-list inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
}

fn default_provided_by() -> Vec<StackInputProvider> {
    vec![StackInputProvider::Developer]
}

fn default_required() -> bool {
    true
}

const TEXT: &[StackInputKind] = &[StackInputKind::String, StackInputKind::Secret];
const NUMERIC: &[StackInputKind] = &[StackInputKind::Number, StackInputKind::Integer];

/// Turns the inputs of a stack file, in file order, into definitions.
///
/// Validates each input, and then the `when` conditions across them.
pub fn normalize_inputs(
    inputs: impl IntoIterator<Item = (String, AuthoredInput)>,
) -> Result<Vec<StackInputDefinition>> {
    let definitions = inputs
        .into_iter()
        .map(|(id, input)| input.into_definition(&id))
        .collect::<Result<Vec<_>>>()?;
    validate_conditions(&definitions)?;
    Ok(definitions)
}

/// Whether `name` can be an environment variable an input is delivered as.
pub fn is_input_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Checks the name of an environment variable delivered from input `input_id`.
pub fn validate_input_env_name(input_id: &str, name: &str) -> Result<()> {
    if is_input_env_name(name) {
        return Ok(());
    }
    Err(invalid(
        &format!("input.{input_id}.env"),
        format!("env name '{name}' must use uppercase letters, digits, and underscores"),
    ))
}

impl AuthoredInput {
    /// The definition this input becomes under `id`.
    pub fn into_definition(self, id: &str) -> Result<StackInputDefinition> {
        let field = |key: &str| format!("input.{id}.{key}");
        validate_input_id(id)?;

        if self.provided_by.is_empty() {
            return Err(invalid(
                &field("providedBy"),
                "must include at least one provider",
            ));
        }
        if self.label.trim().is_empty() {
            return Err(invalid(&field("label"), "must include a label"));
        }
        if self.required && self.description.trim().is_empty() {
            return Err(invalid(
                &field("description"),
                "must include a description when the input is required",
            ));
        }

        let validation = self.validation(id)?;
        validate_validation(id, &self.kind, &validation)?;

        if let Some(generate) = &self.generate {
            self.validate_generate(id, generate, &validation)?;
        }

        for mapping in &self.env {
            validate_input_env_name(id, &mapping.name)?;
            if mapping.target_resources.as_ref().is_some_and(Vec::is_empty) {
                return Err(invalid(
                    &field("env"),
                    format!(
                        "env mapping '{}' cannot use an empty target-resources list",
                        mapping.name
                    ),
                ));
            }
        }

        let default = self
            .default
            .as_ref()
            .map(|value| {
                input_default(&self.kind, value).map_err(|error| relocate(error, &field("default")))
            })
            .transpose()?;

        Ok(StackInputDefinition {
            id: id.to_string(),
            kind: self.kind.clone(),
            provided_by: self.provided_by,
            required: self.required,
            label: self.label,
            description: self.description,
            placeholder: self.placeholder,
            default,
            platforms: self.platforms,
            when: self.when,
            validation: input_validation(&self.kind, validation),
            generate: self.generate,
            env: self
                .env
                .into_iter()
                .map(|mapping| StackInputEnvironmentMapping {
                    name: mapping.name,
                    target_resources: mapping.target_resources,
                    var_type: mapping.var_type,
                })
                .collect(),
        })
    }

    /// The validation block, rejecting constraints the kind cannot have.
    fn validation(&self, id: &str) -> Result<StackInputValidation> {
        let only = |key: &str, present: bool, kinds: &[StackInputKind], what: &str| {
            if present && !kinds.contains(&self.kind) {
                return Err(invalid(
                    &format!("input.{id}.{key}"),
                    format!("applies to {what} inputs only"),
                ));
            }
            Ok(())
        };
        only(
            "minLength",
            self.min_length.is_some(),
            TEXT,
            "string and secret",
        )?;
        only(
            "maxLength",
            self.max_length.is_some(),
            TEXT,
            "string and secret",
        )?;
        only("pattern", self.pattern.is_some(), TEXT, "string and secret")?;
        only("format", self.format.is_some(), TEXT, "string and secret")?;
        only("min", self.min.is_some(), NUMERIC, "number and integer")?;
        only("max", self.max.is_some(), NUMERIC, "number and integer")?;
        only(
            "values",
            self.values.is_some(),
            &[StackInputKind::Enum],
            "enum",
        )?;
        only(
            "minItems",
            self.min_items.is_some(),
            &[StackInputKind::StringList],
            "string-list",
        )?;
        only(
            "maxItems",
            self.max_items.is_some(),
            &[StackInputKind::StringList],
            "string-list",
        )?;

        let number = |key: &str, value: &Option<InputValue>| {
            value
                .as_ref()
                .map(|value| {
                    input_number(key, &self.kind, value)
                        .map_err(|error| relocate(error, &format!("input.{id}.{key}")))
                })
                .transpose()
        };
        Ok(StackInputValidation {
            min_length: self.min_length,
            max_length: self.max_length,
            pattern: self.pattern.clone(),
            format: self.format.clone(),
            min: number("min", &self.min)?,
            max: number("max", &self.max)?,
            values: self.values.clone(),
            min_items: self.min_items,
            max_items: self.max_items,
        })
    }

    fn validate_generate(
        &self,
        id: &str,
        generate: &StackInputGenerate,
        validation: &StackInputValidation,
    ) -> Result<()> {
        let field = format!("input.{id}.generate");
        if self.kind != StackInputKind::Secret {
            return Err(invalid(&field, "is only supported on secret inputs"));
        }
        if self.default.is_some() {
            return Err(invalid(&field, "cannot be combined with a default"));
        }
        if self.provided_by != [StackInputProvider::Developer] {
            return Err(invalid(
                &field,
                "a generated input must be provided by the developer only; Alien supplies \
                 the value, so the deployer is never asked for it",
            ));
        }
        let length = generate.length;
        if !(STACK_INPUT_GENERATE_MIN_LENGTH..=STACK_INPUT_GENERATE_MAX_LENGTH).contains(&length) {
            return Err(invalid(
                &field,
                format!(
                    "length must be an integer between {STACK_INPUT_GENERATE_MIN_LENGTH} and \
                     {STACK_INPUT_GENERATE_MAX_LENGTH}"
                ),
            ));
        }
        if validation.pattern.is_some() || validation.format.is_some() {
            return Err(invalid(
                &field,
                "cannot be combined with pattern or format; generated values are alphanumeric",
            ));
        }
        if validation.min_length.is_some_and(|min| length < min)
            || validation.max_length.is_some_and(|max| length > max)
        {
            return Err(invalid(
                &field,
                format!("length {length} is outside the input's min-length and max-length"),
            ));
        }
        Ok(())
    }
}

/// The same error, reported against `field`.
fn relocate(
    error: alien_error::AlienError<crate::ErrorData>,
    field: &str,
) -> alien_error::AlienError<crate::ErrorData> {
    match error.error {
        Some(crate::ErrorData::StackDefinitionInvalid { reason, .. }) => invalid(field, reason),
        _ => error,
    }
}

fn validate_input_id(id: &str) -> Result<()> {
    let field = format!("input.{id}");
    let mut chars = id.chars();
    let valid = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(invalid(
            &field,
            "the ID must start with a letter or underscore and contain only letters, digits, \
             and underscores",
        ));
    }
    if !id.chars().any(|c| c.is_ascii_lowercase()) {
        return Err(invalid(
            &field,
            "the ID must not be all caps; that style is for environment variables",
        ));
    }
    Ok(())
}

fn validate_validation(
    id: &str,
    kind: &StackInputKind,
    validation: &StackInputValidation,
) -> Result<()> {
    let field = |key: &str| format!("input.{id}.{key}");
    if let (Some(min), Some(max)) = (validation.min_length, validation.max_length) {
        if min > max {
            return Err(invalid(
                &field("minLength"),
                "must be less than or equal to max-length",
            ));
        }
    }
    if let (Some(min), Some(max)) = (validation.min_items, validation.max_items) {
        if min > max {
            return Err(invalid(
                &field("minItems"),
                "must be less than or equal to max-items",
            ));
        }
    }
    if *kind == StackInputKind::Enum
        && validation
            .values
            .as_ref()
            .is_none_or(|values| values.is_empty())
    {
        return Err(invalid(
            &field("values"),
            "an enum input needs at least one value",
        ));
    }
    if let Some(pattern) = &validation.pattern {
        validate_portable_pattern(&field("pattern"), pattern)?;
    }
    Ok(())
}

fn validate_portable_pattern(field: &str, pattern: &str) -> Result<()> {
    if pattern.is_empty() {
        return Err(invalid(field, "must not be empty"));
    }
    if pattern.len() > 1 && pattern.starts_with('/') && pattern.ends_with('/') {
        return Err(invalid(field, "must not use regex delimiters"));
    }
    let bytes = pattern.as_bytes();
    let backreference_or_property = bytes
        .windows(2)
        .any(|pair| pair[0] == b'\\' && matches!(pair[1], b'1'..=b'9' | b'p' | b'P' | b'k'));
    let nested_class = pattern.match_indices('[').any(|(start, _)| {
        pattern[start + 1..]
            .chars()
            .take_while(|c| *c != ']')
            .any(|c| c == '[')
    });
    if backreference_or_property || pattern.contains("(?") || nested_class {
        return Err(invalid(
            field,
            "is not portable across TypeScript, CloudFormation, and Terraform",
        ));
    }
    Ok(())
}

/// Checks the `when` conditions of `definitions`, which must all be unconditional booleans
/// that exist in the same stack.
fn validate_conditions(definitions: &[StackInputDefinition]) -> Result<()> {
    let by_id: HashMap<&str, &StackInputDefinition> = definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect();
    for definition in definitions {
        let Some(condition) = &definition.when else {
            continue;
        };
        let reference = by_id.get(condition.input.as_str());
        let reason = if condition.input == definition.id {
            Some("cannot reference itself")
        } else if reference.is_none() {
            Some("references an unknown input")
        } else if reference.is_some_and(|reference| reference.kind != StackInputKind::Boolean) {
            Some("must reference a boolean input")
        } else if reference.is_some_and(|reference| reference.when.is_some()) {
            Some("must reference an unconditional input; chains and cycles are unsupported")
        } else if definition.generate.is_some() {
            Some("cannot be combined with generate")
        } else if definition
            .provided_by
            .contains(&StackInputProvider::Deployer)
            && reference.is_some_and(|reference| {
                !reference
                    .provided_by
                    .contains(&StackInputProvider::Deployer)
            })
        {
            Some("on a deployer input must reference a deployer-provided boolean")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(invalid(
                &format!("input.{}.when", definition.id),
                format!("'{}' {reason}", condition.input),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorData, StackInputDefaultValue};
    use alien_error::AlienError;

    fn input(kind: StackInputKind) -> AuthoredInput {
        AuthoredInput {
            kind,
            provided_by: vec![StackInputProvider::Deployer],
            required: true,
            label: "Input".to_string(),
            description: "Input value".to_string(),
            placeholder: None,
            default: None,
            when: None,
            platforms: None,
            env: Vec::new(),
            generate: None,
            min_length: None,
            max_length: None,
            pattern: None,
            format: None,
            min: None,
            max: None,
            values: None,
            min_items: None,
            max_items: None,
        }
    }

    fn flag(provided_by: StackInputProvider) -> AuthoredInput {
        AuthoredInput {
            provided_by: vec![provided_by],
            ..input(StackInputKind::Boolean)
        }
    }

    fn when(input: &str) -> Option<StackInputCondition> {
        Some(StackInputCondition {
            input: input.to_string(),
            equals: true,
        })
    }

    fn normalize(
        inputs: Vec<(&str, AuthoredInput)>,
    ) -> std::result::Result<Vec<StackInputDefinition>, AlienError<ErrorData>> {
        normalize_inputs(
            inputs
                .into_iter()
                .map(|(id, input)| (id.to_string(), input)),
        )
    }

    fn rejected(inputs: Vec<(&str, AuthoredInput)>) -> (String, String) {
        match normalize(inputs).expect_err("should be rejected").error {
            Some(ErrorData::StackDefinitionInvalid { field, reason }) => (field, reason),
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn a_boolean_gates_a_string_and_keeps_its_condition() {
        let definitions = normalize(vec![
            ("enabled", flag(StackInputProvider::Deployer)),
            (
                "bucket",
                AuthoredInput {
                    when: when("enabled"),
                    env: vec![AuthoredEnvMapping {
                        name: "BUCKET".to_string(),
                        target_resources: None,
                        var_type: None,
                    }],
                    ..input(StackInputKind::String)
                },
            ),
        ])
        .unwrap();
        assert_eq!(definitions.len(), 2);
        assert_eq!(definitions[0].id, "enabled");
        assert_eq!(
            definitions[0].validation, None,
            "a boolean has no validation block"
        );
        assert_eq!(definitions[1].when, when("enabled"));
        assert_eq!(
            definitions[1].validation,
            Some(StackInputValidation::default()),
            "other kinds always carry one, even when empty"
        );
        assert_eq!(definitions[1].env[0].name, "BUCKET");
    }

    #[test]
    fn a_developer_controller_is_rejected_for_a_deployer_input_only() {
        let (field, reason) = rejected(vec![
            ("enabled", flag(StackInputProvider::Developer)),
            (
                "bucket",
                AuthoredInput {
                    when: when("enabled"),
                    ..input(StackInputKind::String)
                },
            ),
        ]);
        assert_eq!(field, "input.bucket.when");
        assert!(reason.contains("deployer-provided"), "{reason}");

        normalize(vec![
            ("enabled", flag(StackInputProvider::Developer)),
            (
                "bucket",
                AuthoredInput {
                    provided_by: vec![StackInputProvider::Developer],
                    when: when("enabled"),
                    ..input(StackInputKind::String)
                },
            ),
        ])
        .expect("a developer input may follow a developer boolean");
    }

    #[test]
    fn a_condition_cannot_be_combined_with_generate() {
        let (field, reason) = rejected(vec![
            ("enabled", flag(StackInputProvider::Developer)),
            (
                "token",
                AuthoredInput {
                    provided_by: vec![StackInputProvider::Developer],
                    when: when("enabled"),
                    generate: Some(StackInputGenerate { length: 64 }),
                    ..input(StackInputKind::Secret)
                },
            ),
        ]);
        assert_eq!(field, "input.token.when");
        assert!(reason.contains("generate"), "{reason}");
    }

    #[test]
    fn a_condition_must_reference_an_existing_other_input() {
        for reference in ["missing", "bucket"] {
            let (field, reason) = rejected(vec![(
                "bucket",
                AuthoredInput {
                    when: when(reference),
                    ..input(StackInputKind::String)
                },
            )]);
            assert_eq!(field, "input.bucket.when");
            assert!(
                reason.contains("itself") || reason.contains("unknown"),
                "{reason}"
            );
        }
    }

    #[test]
    fn a_condition_must_reference_an_unconditional_boolean() {
        let (_, reason) = rejected(vec![
            ("enabled", input(StackInputKind::String)),
            (
                "bucket",
                AuthoredInput {
                    when: when("enabled"),
                    ..input(StackInputKind::String)
                },
            ),
        ]);
        assert!(reason.contains("boolean"), "{reason}");

        let (_, reason) = rejected(vec![
            ("outer", flag(StackInputProvider::Deployer)),
            (
                "enabled",
                AuthoredInput {
                    when: when("outer"),
                    ..flag(StackInputProvider::Deployer)
                },
            ),
            (
                "bucket",
                AuthoredInput {
                    when: when("enabled"),
                    ..input(StackInputKind::String)
                },
            ),
        ]);
        assert!(reason.contains("unconditional"), "{reason}");
    }

    #[test]
    fn ids_are_identifiers_with_a_lowercase_letter() {
        for id in ["1st", "has-dash", "API_KEY", ""] {
            let (field, _) = rejected(vec![(id, input(StackInputKind::String))]);
            assert_eq!(field, format!("input.{id}"));
        }
        normalize(vec![
            ("_x", input(StackInputKind::String)),
            ("apiKey2", input(StackInputKind::String)),
        ])
        .unwrap();
    }

    #[test]
    fn labels_descriptions_and_providers_are_required() {
        let (field, _) = rejected(vec![(
            "a",
            AuthoredInput {
                label: " ".to_string(),
                ..input(StackInputKind::String)
            },
        )]);
        assert_eq!(field, "input.a.label");
        let (field, _) = rejected(vec![(
            "a",
            AuthoredInput {
                description: String::new(),
                ..input(StackInputKind::String)
            },
        )]);
        assert_eq!(field, "input.a.description");
        let optional = normalize(vec![(
            "a",
            AuthoredInput {
                required: false,
                description: String::new(),
                ..input(StackInputKind::String)
            },
        )])
        .unwrap();
        assert_eq!(optional[0].description, "");
        let (field, _) = rejected(vec![(
            "a",
            AuthoredInput {
                provided_by: vec![],
                ..input(StackInputKind::String)
            },
        )]);
        assert_eq!(field, "input.a.providedBy");
    }

    #[test]
    fn constraints_belong_to_their_kind() {
        let cases: Vec<(&str, AuthoredInput)> = vec![
            (
                "input.a.min",
                AuthoredInput {
                    min: Some(InputValue::Integer(1)),
                    ..input(StackInputKind::String)
                },
            ),
            (
                "input.a.minLength",
                AuthoredInput {
                    min_length: Some(1),
                    ..input(StackInputKind::Integer)
                },
            ),
            (
                "input.a.values",
                AuthoredInput {
                    values: Some(vec!["x".into()]),
                    ..input(StackInputKind::String)
                },
            ),
            (
                "input.a.maxItems",
                AuthoredInput {
                    max_items: Some(2),
                    ..input(StackInputKind::String)
                },
            ),
            (
                "input.a.values",
                AuthoredInput {
                    values: Some(vec![]),
                    ..input(StackInputKind::Enum)
                },
            ),
            ("input.a.values", input(StackInputKind::Enum)),
            (
                "input.a.minLength",
                AuthoredInput {
                    min_length: Some(5),
                    max_length: Some(4),
                    ..input(StackInputKind::String)
                },
            ),
            (
                "input.a.minItems",
                AuthoredInput {
                    min_items: Some(3),
                    max_items: Some(2),
                    ..input(StackInputKind::StringList)
                },
            ),
        ];
        for (expected, case) in cases {
            let (field, reason) = rejected(vec![("a", case)]);
            assert_eq!(field, expected, "{reason}");
        }
    }

    #[test]
    fn numbers_print_the_way_javascript_prints_them() {
        let definitions = normalize(vec![
            (
                "rate",
                AuthoredInput {
                    min: Some(InputValue::Integer(0)),
                    max: Some(InputValue::Float(1.0)),
                    default: Some(InputValue::Float(0.25)),
                    ..input(StackInputKind::Number)
                },
            ),
            (
                "size",
                AuthoredInput {
                    min: Some(InputValue::Float(1.0)),
                    default: Some(InputValue::Integer(100)),
                    ..input(StackInputKind::Integer)
                },
            ),
        ])
        .unwrap();
        let rate = definitions[0].validation.as_ref().unwrap();
        assert_eq!(rate.min.as_deref(), Some("0"));
        assert_eq!(rate.max.as_deref(), Some("1"));
        assert_eq!(
            definitions[0].default,
            Some(StackInputDefaultValue::Number("0.25".to_string()))
        );
        assert_eq!(
            definitions[1].validation.as_ref().unwrap().min.as_deref(),
            Some("1")
        );

        let (field, reason) = rejected(vec![(
            "size",
            AuthoredInput {
                max: Some(InputValue::Float(1.5)),
                ..input(StackInputKind::Integer)
            },
        )]);
        assert_eq!(field, "input.size.max");
        assert!(reason.contains("whole number"), "{reason}");
    }

    #[test]
    fn defaults_are_typed_by_kind_and_secrets_have_none() {
        let (field, _) = rejected(vec![(
            "token",
            AuthoredInput {
                default: Some(InputValue::String("x".into())),
                ..input(StackInputKind::Secret)
            },
        )]);
        assert_eq!(field, "input.token.default");
        let (field, reason) = rejected(vec![(
            "flag",
            AuthoredInput {
                default: Some(InputValue::String("yes".into())),
                ..input(StackInputKind::Boolean)
            },
        )]);
        assert_eq!(field, "input.flag.default");
        assert!(reason.contains("a boolean"), "{reason}");
    }

    #[test]
    fn generated_secrets_follow_their_rules() {
        let generated = || AuthoredInput {
            provided_by: vec![StackInputProvider::Developer],
            generate: Some(StackInputGenerate { length: 32 }),
            ..input(StackInputKind::Secret)
        };
        let definitions = normalize(vec![("token", generated())]).unwrap();
        assert_eq!(
            definitions[0].generate,
            Some(StackInputGenerate { length: 32 })
        );

        let cases: Vec<AuthoredInput> = vec![
            AuthoredInput {
                kind: StackInputKind::String,
                ..generated()
            },
            AuthoredInput {
                provided_by: vec![StackInputProvider::Developer, StackInputProvider::Deployer],
                ..generated()
            },
            AuthoredInput {
                generate: Some(StackInputGenerate { length: 15 }),
                ..generated()
            },
            AuthoredInput {
                generate: Some(StackInputGenerate { length: 257 }),
                ..generated()
            },
            AuthoredInput {
                pattern: Some("^[a-z]+$".into()),
                ..generated()
            },
            AuthoredInput {
                format: Some("url".into()),
                ..generated()
            },
            AuthoredInput {
                min_length: Some(33),
                ..generated()
            },
            AuthoredInput {
                max_length: Some(31),
                ..generated()
            },
        ];
        for case in cases {
            let (field, reason) = rejected(vec![("token", case)]);
            assert_eq!(field, "input.token.generate", "{reason}");
        }
    }

    #[test]
    fn env_names_and_targets_are_checked() {
        let with_env = |env: Vec<AuthoredEnvMapping>| {
            vec![(
                "a",
                AuthoredInput {
                    env,
                    ..input(StackInputKind::String)
                },
            )]
        };
        let mapping = |name: &str, targets: Option<Vec<String>>| AuthoredEnvMapping {
            name: name.to_string(),
            target_resources: targets,
            var_type: None,
        };
        let (field, reason) = rejected(with_env(vec![mapping("lower", None)]));
        assert_eq!(field, "input.a.env");
        assert!(reason.contains("'lower'"), "{reason}");
        let (field, _) = rejected(with_env(vec![mapping("OK", Some(vec![]))]));
        assert_eq!(field, "input.a.env");
        normalize(with_env(vec![
            mapping("_OK_1", None),
            mapping("ALSO", Some(vec!["work*".into()])),
        ]))
        .unwrap();
    }

    #[test]
    fn patterns_must_be_portable() {
        for pattern in ["", "/abc/", r"(a)\1", r"\p{L}", "(?i)a", "[a[b]]", r"\k<n>"] {
            let (field, _) = rejected(vec![(
                "a",
                AuthoredInput {
                    pattern: Some(pattern.to_string()),
                    ..input(StackInputKind::String)
                },
            )]);
            assert_eq!(field, "input.a.pattern", "{pattern}");
        }
        for pattern in ["^[a-z0-9-]+$", r"^\d{3}$", "/", "[a-z][0-9]"] {
            normalize(vec![(
                "a",
                AuthoredInput {
                    pattern: Some(pattern.to_string()),
                    ..input(StackInputKind::String)
                },
            )])
            .unwrap_or_else(|error| panic!("{pattern}: {error}"));
        }
    }

    #[test]
    fn an_input_deserializes_from_its_canonical_json() {
        let input: AuthoredInput = serde_json::from_value(serde_json::json!({
            "kind": "stringList",
            "label": "Images",
            "required": false,
            "default": ["a", "b"],
            "maxItems": 8,
            "platforms": ["machines"],
            "env": [{ "name": "IMAGES", "targetResources": ["w*"], "type": "plain" }]
        }))
        .unwrap();
        assert_eq!(input.provided_by, [StackInputProvider::Developer]);
        let definition = input.into_definition("images").unwrap();
        assert_eq!(
            definition.default,
            Some(StackInputDefaultValue::StringList(vec![
                "a".into(),
                "b".into()
            ]))
        );
        assert_eq!(definition.platforms, Some(vec![Platform::Machines]));

        let error = serde_json::from_value::<AuthoredInput>(serde_json::json!({
            "kind": "string", "label": "x", "minlength": 1
        }))
        .expect_err("unknown key");
        assert!(error.to_string().contains("minlength"), "{error}");
    }
}
