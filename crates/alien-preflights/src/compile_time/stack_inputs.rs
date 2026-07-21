use std::collections::HashSet;

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{
    Container, Daemon, Platform, Stack, StackInputDefaultValue, StackInputDefinition,
    StackInputKind, StackInputValidation, Worker,
};

/// Validates stack input definitions before release/package generation.
pub struct StackInputsDefinitionCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for StackInputsDefinitionCheck {
    fn description(&self) -> &'static str {
        "Stack inputs must be portable, well-described, and resolvable to env-capable resources"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        !stack.inputs().is_empty()
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let mut ids = HashSet::new();

        for input in stack.inputs() {
            validate_input(input, stack, &mut ids, &mut errors);
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

fn validate_input(
    input: &StackInputDefinition,
    stack: &Stack,
    ids: &mut HashSet<String>,
    errors: &mut Vec<String>,
) {
    if let Err(reason) = validate_input_id(&input.id) {
        errors.push(format!("Stack input '{}': {}", input.id, reason));
    }

    let normalized_id = input.id.to_ascii_lowercase();
    if !ids.insert(normalized_id) {
        errors.push(format!(
            "Stack input '{}': input ids must be unique after case normalization",
            input.id
        ));
    }

    if input.provided_by.is_empty() {
        errors.push(format!(
            "Stack input '{}': providedBy must include at least one provider",
            input.id
        ));
    }

    if input.label.trim().is_empty() {
        errors.push(format!("Stack input '{}': label is required", input.id));
    }

    if input.required && input.description.trim().is_empty() {
        errors.push(format!(
            "Stack input '{}': description is required for required inputs",
            input.id
        ));
    }

    if input.kind == StackInputKind::Secret && input.default.is_some() {
        errors.push(format!(
            "Stack input '{}': secret inputs cannot declare defaults",
            input.id
        ));
    }

    if let Some(default) = &input.default {
        validate_default_matches_kind(input, default, errors);
    }

    if let Some(validation) = &input.validation {
        validate_constraints(input, validation, errors);
    }

    for mapping in &input.env {
        validate_env_name(&input.id, &mapping.name, errors);

        if let Some(target_resources) = &mapping.target_resources {
            if target_resources.is_empty() {
                errors.push(format!(
                    "Stack input '{}': env mapping '{}' has an empty targetResources list; omit targetResources to target every env-capable resource",
                    input.id, mapping.name
                ));
            }

            for target in target_resources {
                if target.contains('*') {
                    continue;
                }

                match stack.resources().find(|(id, _)| id.as_str() == target.as_str()) {
                    Some((_, entry)) if is_env_capable_resource(&entry.config.resource_type()) => {}
                    Some((_, entry)) => errors.push(format!(
                        "Stack input '{}': env mapping '{}' targets resource '{}' of type '{}', which does not support runtime environment variables",
                        input.id,
                        mapping.name,
                        target,
                        entry.config.resource_type()
                    )),
                    None => errors.push(format!(
                        "Stack input '{}': env mapping '{}' targets unknown resource '{}'",
                        input.id, mapping.name, target
                    )),
                }
            }
        }
    }
}

fn validate_input_id(id: &str) -> std::result::Result<(), &'static str> {
    if id.is_empty() {
        return Err("id must not be empty");
    }

    let first = id.as_bytes()[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return Err("id must start with a letter or underscore");
    }

    if !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("id must contain only letters, digits, and underscores");
    }

    if id.bytes().all(|b| !b.is_ascii_lowercase()) {
        return Err("id must not be all-caps env-var style");
    }

    Ok(())
}

fn validate_default_matches_kind(
    input: &StackInputDefinition,
    default: &StackInputDefaultValue,
    errors: &mut Vec<String>,
) {
    let valid = matches!(
        (&input.kind, default),
        (StackInputKind::String, StackInputDefaultValue::String(_))
            | (StackInputKind::Secret, StackInputDefaultValue::String(_))
            | (StackInputKind::Enum, StackInputDefaultValue::String(_))
            | (StackInputKind::Number, StackInputDefaultValue::Number(_))
            | (StackInputKind::Integer, StackInputDefaultValue::Number(_))
            | (StackInputKind::Boolean, StackInputDefaultValue::Boolean(_))
            | (
                StackInputKind::StringList,
                StackInputDefaultValue::StringList(_)
            )
    );

    if !valid {
        errors.push(format!(
            "Stack input '{}': default value does not match input kind",
            input.id
        ));
    }
}

fn validate_constraints(
    input: &StackInputDefinition,
    validation: &StackInputValidation,
    errors: &mut Vec<String>,
) {
    if let (Some(min), Some(max)) = (validation.min_length, validation.max_length) {
        if min > max {
            errors.push(format!(
                "Stack input '{}': minLength must be less than or equal to maxLength",
                input.id
            ));
        }
    }

    if let (Some(min), Some(max)) = (validation.min_items, validation.max_items) {
        if min > max {
            errors.push(format!(
                "Stack input '{}': minItems must be less than or equal to maxItems",
                input.id
            ));
        }
    }

    if matches!(input.kind, StackInputKind::Enum) {
        match validation.values.as_ref() {
            Some(values) if !values.is_empty() => {}
            _ => errors.push(format!(
                "Stack input '{}': enum inputs require at least one value",
                input.id
            )),
        }
    }

    if let Some(pattern) = &validation.pattern {
        if let Err(reason) = validate_portable_pattern(pattern) {
            errors.push(format!(
                "Stack input '{}': pattern '{}' is not portable: {}",
                input.id, pattern, reason
            ));
        }
    }
}

fn validate_env_name(input_id: &str, name: &str, errors: &mut Vec<String>) {
    if name.is_empty() {
        errors.push(format!(
            "Stack input '{}': env name must not be empty",
            input_id
        ));
        return;
    }

    let first = name.as_bytes()[0];
    if !first.is_ascii_uppercase() && first != b'_' {
        errors.push(format!(
            "Stack input '{}': env name '{}' must start with an uppercase letter or underscore",
            input_id, name
        ));
    }

    if !name
        .bytes()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        errors.push(format!(
            "Stack input '{}': env name '{}' must contain only uppercase letters, digits, and underscores",
            input_id, name
        ));
    }
}

fn is_env_capable_resource(resource_type: &alien_core::ResourceType) -> bool {
    *resource_type == Worker::RESOURCE_TYPE
        || *resource_type == Container::RESOURCE_TYPE
        || *resource_type == Daemon::RESOURCE_TYPE
}

fn validate_portable_pattern(pattern: &str) -> std::result::Result<(), &'static str> {
    if pattern.is_empty() {
        return Err("pattern must not be empty");
    }

    if pattern.starts_with('/') && pattern.ends_with('/') && pattern.len() > 1 {
        return Err("regex delimiters are not allowed");
    }

    let bytes = pattern.as_bytes();
    let mut index = 0;
    let mut class_depth = 0;
    let mut group_depth = 0;

    while index < bytes.len() {
        let current = bytes[index];

        if current == b'\\' {
            let Some(next) = bytes.get(index + 1).copied() else {
                return Err("trailing escape is not allowed");
            };
            if matches!(next, b'1'..=b'9' | b'p' | b'P' | b'k') {
                return Err("backreferences and unicode classes are not portable");
            }
            index += 2;
            continue;
        }

        if class_depth > 0 {
            if current == b'[' {
                return Err("nested character classes are not portable");
            }
            if current == b']' {
                class_depth -= 1;
            }
            index += 1;
            continue;
        }

        match current {
            b'[' => class_depth += 1,
            b']' => return Err("unmatched character class close"),
            b'(' => {
                if matches!(bytes.get(index + 1), Some(b'?')) {
                    return Err("lookaround, named captures, inline flags, and non-capturing groups are not portable in v1");
                }
                group_depth += 1;
            }
            b')' => {
                if group_depth == 0 {
                    return Err("unmatched group close");
                }
                group_depth -= 1;
            }
            _ => {}
        }

        index += 1;
    }

    if class_depth != 0 {
        return Err("unclosed character class");
    }

    if group_depth != 0 {
        return Err("unclosed group");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        permissions::PermissionsConfig, Resource, ResourceEntry, ResourceLifecycle,
        StackInputEnvironmentMapping, StackInputEnvironmentVariableType, StackInputProvider,
        Worker, WorkerCode,
    };
    use indexmap::IndexMap;

    fn test_stack(inputs: Vec<StackInputDefinition>) -> Stack {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "example.com/api:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: vec![],
                remote_access: false,
                enabled_when: None,
            },
        );

        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig::default(),
            supported_platforms: None,
            inputs,
        }
    }

    fn string_input(id: &str) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind: StackInputKind::String,
            provided_by: vec![StackInputProvider::Deployer],
            required: true,
            label: "Tenant slug".to_string(),
            description: "Lowercase tenant identifier.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: Some(StackInputValidation {
                min_length: Some(3),
                max_length: Some(64),
                pattern: Some("[a-z0-9][a-z0-9-]{2,62}".to_string()),
                format: None,
                min: None,
                max: None,
                values: None,
                min_items: None,
                max_items: None,
            }),
            env: vec![StackInputEnvironmentMapping {
                name: "TENANT_SLUG".to_string(),
                target_resources: Some(vec!["api".to_string()]),
                var_type: Some(StackInputEnvironmentVariableType::Plain),
            }],
        }
    }

    #[tokio::test]
    async fn accepts_valid_stack_input() {
        let check = StackInputsDefinitionCheck;
        let result = check
            .check(&test_stack(vec![string_input("tenantSlug")]), Platform::Aws)
            .await
            .expect("check should run");

        assert!(result.success, "errors: {:?}", result.errors);
    }

    #[tokio::test]
    async fn rejects_non_portable_pattern() {
        let mut input = string_input("tenantSlug");
        input.validation.as_mut().unwrap().pattern = Some("(?=abc).*".to_string());

        let check = StackInputsDefinitionCheck;
        let result = check
            .check(&test_stack(vec![input]), Platform::Aws)
            .await
            .expect("check should run");

        assert!(!result.success);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("not portable")));
    }

    #[tokio::test]
    async fn rejects_unknown_target_resource() {
        let mut input = string_input("tenantSlug");
        input.env[0].target_resources = Some(vec!["missing".to_string()]);

        let check = StackInputsDefinitionCheck;
        let result = check
            .check(&test_stack(vec![input]), Platform::Aws)
            .await
            .expect("check should run");

        assert!(!result.success);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("targets unknown resource")));
    }
}
