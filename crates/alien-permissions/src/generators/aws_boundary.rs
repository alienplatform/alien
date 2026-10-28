//! The setup-owned ceiling for roles written by management.

use std::{borrow::Cow, collections::BTreeSet};

use alien_core::{AwsBindingSpec, PermissionSetReference, Stack};

use alien_error::AlienError;
use serde_json::{json, Value};

use crate::{
    get_permission_set, list_permission_set_ids, BindingTarget, ErrorData, PermissionContext,
    Result,
};

use super::{AwsIamStatement, AwsRuntimePermissionsGenerator};

const MANAGED_POLICY_LIMIT: usize = 6_144;

/// Render the boundary including inline grants from application permission profiles.
///
/// The resolver receives the profile name and scope (`"*"` or a resource id). It must
/// return the same context used to compile that identity's grants, including actual
/// resource names for resource scopes. It is called only for scopes with inline AWS
/// permissions. Management permissions are deliberately not part of this ceiling.
/// Named catalog grants are already included by [`aws_permissions_boundary`].
pub fn aws_stack_permissions_boundary(
    stack: &Stack,
    context: &PermissionContext,
    mut resolve_context: impl FnMut(&str, &str) -> Result<PermissionContext>,
) -> Result<Value> {
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut statements = Vec::new();
    for (profile_name, profile) in stack.permission_profiles() {
        for (scope, references) in &profile.0 {
            let inline_sets: Vec<_> = references
                .iter()
                .filter_map(|reference| match reference {
                    PermissionSetReference::Inline(set) if set.platforms.aws.is_some() => Some(set),
                    _ => None,
                })
                .collect();
            if inline_sets.is_empty() {
                continue;
            }
            let grant_context = resolve_context(profile_name, scope)?;
            let target = if scope == "*" {
                BindingTarget::Stack
            } else {
                BindingTarget::Resource
            };
            for set in inline_sets {
                statements.extend(
                    generator
                        .generate_policy(set, target, &grant_context)?
                        .statement,
                );
            }
        }
    }
    aws_permissions_boundary(context, &statements)
}

/// Render the stack permissions boundary from the runtime catalog and compiled inline grants.
///
/// `custom_statements` must contain only inline AWS grants from runtime profiles, compiled with
/// their actual resource contexts. Setup must approve changes to these grants. Catalog resource
/// bindings are widened to the stack prefix; unprefixed grants retain their actions and conditions.
/// Returns an error if interpolation fails or the document exceeds IAM's managed-policy limit.
pub fn aws_permissions_boundary(
    context: &PermissionContext,
    custom_statements: &[AwsIamStatement],
) -> Result<Value> {
    let prefix = context.stack_prefix.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::VariableNotFound {
            variable: "stackPrefix".to_string(),
        })
    })?;
    let mut catalog_context = context.clone();
    catalog_context.resource_name = Some(format!("{prefix}-*"));
    let mut scoped_resources = BTreeSet::new();
    let mut statements = Vec::new();
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut ids = list_permission_set_ids();
    ids.sort();
    for id in ids.into_iter().filter(|id| is_runtime_set(id)) {
        let set = get_permission_set(id).ok_or_else(|| {
            AlienError::new(ErrorData::InvalidPermissionSet {
                message: format!("Catalog permission set '{id}' is missing"),
            })
        })?;
        for entry in set
            .platforms
            .aws
            .iter()
            .flatten()
            .filter(|e| e.effect.is_allow())
        {
            for binding in [&entry.binding.stack, &entry.binding.resource]
                .into_iter()
                .flatten()
            {
                let mut entry = entry.clone();
                let mut binding = binding.clone();
                // Some resource bindings already include the prefix before resourceName.
                for resource in &mut binding.resources {
                    *resource =
                        resource.replace("${stackPrefix}-${resourceName}", "${resourceName}");
                }
                entry.binding.stack = Some(binding.clone());
                let mut single = set.clone();
                single.platforms.aws = Some(vec![entry]);
                let policy =
                    generator.generate_policy(&single, BindingTarget::Stack, &catalog_context)?;
                for mut statement in policy.statement {
                    let mut unprefixed = Vec::new();
                    for (template, resource) in binding.resources.iter().zip(statement.resource) {
                        if template.contains("${stackPrefix}")
                            || template.contains("${resourceName}")
                        {
                            scoped_resources.insert(resource);
                        } else {
                            unprefixed.push(resource);
                        }
                    }
                    statement.resource = unprefixed;
                    if !statement.resource.is_empty() || !statement.not_resource.is_empty() {
                        merge_statement(&mut statements, statement);
                    }
                }
            }
        }
    }
    for statement in custom_statements {
        merge_statement(&mut statements, statement.clone());
    }
    // A trailing wildcard also covers longer catalog patterns such as log streams and objects.
    let scoped_resources: Vec<_> = scoped_resources
        .iter()
        .filter(|resource| {
            !scoped_resources.iter().any(|other| {
                other != *resource
                    && other
                        .strip_suffix('*')
                        .is_some_and(|prefix| resource.starts_with(prefix))
            })
        })
        .collect();
    let mut rendered = vec![json!({
        "Effect": "Allow", "NotAction": "iam:*", "Resource": scoped_resources,
    })];
    for statement in statements {
        let mut value = json!({"Effect": statement.effect, "Action": statement.action});
        if !statement.resource.is_empty() {
            value["Resource"] = json!(statement.resource);
        }
        if !statement.not_resource.is_empty() {
            value["NotResource"] = json!(statement.not_resource);
        }
        if let Some(condition) = statement.condition {
            value["Condition"] = json!(condition);
        }
        rendered.push(value);
    }
    let policy = json!({"Version": "2012-10-17", "Statement": rendered});
    let size = policy
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .count();
    if size > MANAGED_POLICY_LIMIT {
        return Err(AlienError::new(ErrorData::PermissionsBoundaryTooLarge {
            size,
            limit: MANAGED_POLICY_LIMIT,
        }));
    }
    Ok(policy)
}

/// Older setups have no boundary policy or adoption grant. Only omit the catalog's
/// boundary condition; explicit custom conditions must keep their original meaning.
pub(super) fn role_write_binding<'a>(
    binding: &'a AwsBindingSpec,
    actions: &'a [String],
    context: &PermissionContext,
) -> (Cow<'a, AwsBindingSpec>, Cow<'a, [String]>) {
    let uses_setup_boundary = binding
        .condition
        .as_ref()
        .and_then(|conditions| conditions.get("StringEquals"))
        .and_then(|values| values.get("iam:PermissionsBoundary"))
        .is_some_and(|value| value == "${awsPermissionsBoundaryArn}");
    if context.aws_permissions_boundary_arn.is_some() || !uses_setup_boundary {
        return (Cow::Borrowed(binding), Cow::Borrowed(actions));
    }

    let mut binding = binding.clone();
    if let Some(conditions) = &mut binding.condition {
        if let Some(values) = conditions.get_mut("StringEquals") {
            values.shift_remove("iam:PermissionsBoundary");
            if values.is_empty() {
                conditions.shift_remove("StringEquals");
            }
        }
        if conditions.is_empty() {
            binding.condition = None;
        }
    }
    let actions = actions
        .iter()
        .filter(|action| action.as_str() != "iam:PutRolePermissionsBoundary")
        .cloned()
        .collect();
    (Cow::Owned(binding), Cow::Owned(actions))
}

fn is_runtime_set(id: &str) -> bool {
    let (category, operation) = id.split_once('/').unwrap_or((id, ""));
    !matches!(category, "guard" | "remote-stack-management" | "observe")
        && !operation.starts_with("protect-")
        && !matches!(
            operation,
            "provision" | "management" | "heartbeat" | "telemetry"
        )
}

fn merge_statement(statements: &mut Vec<AwsIamStatement>, mut statement: AwsIamStatement) {
    statement.resource.sort();
    statement.resource.dedup();
    statement.not_resource.sort();
    statement.not_resource.dedup();
    if let Some(existing) = statements.iter_mut().find(|existing| {
        existing.effect == statement.effect
            && existing.resource == statement.resource
            && existing.not_resource == statement.not_resource
            && existing.condition == statement.condition
    }) {
        existing.action.extend(statement.action);
        existing.action.sort();
        existing.action.dedup();
    } else {
        statement.action.sort();
        statement.action.dedup();
        statements.push(statement);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> PermissionContext {
        PermissionContext::new()
            .with_stack_prefix("s".repeat(40))
            .with_aws_region("ap-southeast-7")
            .with_aws_account_id("123456789012")
            .with_managing_account_id("210987654321")
    }

    fn matches(pattern: &str, value: &str) -> bool {
        let pattern = regex::escape(pattern)
            .replace(r"\*", ".*")
            .replace(r"\?", ".");
        regex::Regex::new(&format!("^{pattern}$"))
            .unwrap()
            .is_match(value)
    }

    #[test]
    fn full_catalog_fits_and_covers_runtime_actions_and_resources() {
        let context = context();
        let policy = aws_permissions_boundary(&context, &[]).unwrap();
        assert!(policy.to_string().len() <= MANAGED_POLICY_LIMIT);
        eprintln!(
            "Full runtime boundary: {} characters",
            policy.to_string().len()
        );
        let context = context.with_resource_name(format!("{}-example", "s".repeat(40)));
        let generator = AwsRuntimePermissionsGenerator::new();
        for id in list_permission_set_ids()
            .into_iter()
            .filter(|id| is_runtime_set(id))
        {
            let set = get_permission_set(id).unwrap();
            for entry in set
                .platforms
                .aws
                .iter()
                .flatten()
                .filter(|e| e.effect.is_allow())
            {
                for (target, binding) in [
                    (BindingTarget::Stack, &entry.binding.stack),
                    (BindingTarget::Resource, &entry.binding.resource),
                ] {
                    if binding.is_none() {
                        continue;
                    }
                    let mut single = set.clone();
                    single.platforms.aws = Some(vec![entry.clone()]);
                    for statement in generator
                        .generate_policy(&single, target, &context)
                        .unwrap()
                        .statement
                    {
                        assert!(
                            statement.not_resource.is_empty(),
                            "Review new runtime NotResource grant: {id}"
                        );
                        for action in &statement.action {
                            for resource in &statement.resource {
                                assert!(
                                    policy["Statement"].as_array().unwrap().iter().any(
                                        |boundary| {
                                            let action_covered = if boundary["NotAction"] == "iam:*"
                                            {
                                                !action.to_ascii_lowercase().starts_with("iam:")
                                                    && action != "*"
                                            } else {
                                                boundary["Action"].as_array().unwrap().iter().any(
                                                    |allowed| {
                                                        matches(
                                                            &allowed
                                                                .as_str()
                                                                .unwrap()
                                                                .to_ascii_lowercase(),
                                                            &action.to_ascii_lowercase(),
                                                        )
                                                    },
                                                )
                                            };
                                            let condition_covered = boundary
                                                .get("Condition")
                                                .is_none()
                                                || boundary["Condition"]
                                                    == serde_json::to_value(&statement.condition)
                                                        .unwrap();
                                            action_covered
                                                && condition_covered
                                                && boundary["Resource"]
                                                    .as_array()
                                                    .unwrap()
                                                    .iter()
                                                    .any(|allowed| {
                                                        matches(allowed.as_str().unwrap(), resource)
                                                    })
                                        }
                                    ),
                                    "Boundary does not cover {id}: {action} on {resource}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn custom_statement(resource: String) -> AwsIamStatement {
        AwsIamStatement {
            sid: "Custom".to_string(),
            effect: "Allow".to_string(),
            action: vec!["s3:GetObject".to_string()],
            resource: vec![resource],
            not_resource: vec![],
            condition: Some(
                serde_json::from_value(
                    json!({"StringEquals": {"s3:ExistingObjectTag/access": "approved"}}),
                )
                .unwrap(),
            ),
        }
    }

    #[test]
    fn custom_permissions_keep_conditions_and_merge_matching_resources() {
        let first = custom_statement("arn:aws:s3:::external-bucket/*".to_string());
        let mut second = first.clone();
        second.action = vec!["s3:GetObjectVersion".to_string()];
        let boundary = aws_permissions_boundary(&context(), &[first.clone(), second]).unwrap();
        let grants: Vec<_> = boundary["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["Resource"] == json!(first.resource))
            .collect();
        assert_eq!(grants.len(), 1);
        assert_eq!(
            grants[0]["Action"],
            json!(["s3:GetObject", "s3:GetObjectVersion"])
        );
        assert_eq!(grants[0]["Condition"], json!(first.condition));
    }

    #[test]
    fn oversized_boundary_fails_with_actionable_error() {
        let error = aws_permissions_boundary(
            &context(),
            &[custom_statement(format!(
                "arn:aws:s3:::{}/*",
                "x".repeat(MANAGED_POLICY_LIMIT)
            ))],
        )
        .unwrap_err();
        assert_eq!(error.code, "PERMISSIONS_BOUNDARY_TOO_LARGE");
    }

    fn inline_profile(resource: &str) -> alien_core::PermissionProfile {
        let set = serde_json::from_value(json!({
            "id": "custom/read",
            "description": "Read approved objects",
            "platforms": {"aws": [{
                "grant": {"actions": ["s3:GetObject"]},
                "binding": {
                    "stack": {"resources": [resource]},
                    "resource": {
                        "resources": ["arn:aws:s3:::${resourceName}/*"],
                        "condition": {"StringEquals": {"s3:ExistingObjectTag/profile": "${resourceId}"}}
                    }
                }
            }]}
        })).unwrap();
        alien_core::PermissionProfile(indexmap::indexmap! {
            "*".to_string() => vec![PermissionSetReference::Inline(set)]
        })
    }

    #[test]
    fn stack_boundary_compiles_inline_grants_with_their_identity_and_resource_contexts() {
        let mut scoped = inline_profile("unused");
        let sets = scoped.0.shift_remove("*").unwrap();
        scoped.0.insert("uploads".to_string(), sets);
        let mut other_cloud = inline_profile("unused");
        let PermissionSetReference::Inline(set) = &mut other_cloud.0["*"][0] else {
            unreachable!()
        };
        set.platforms.aws = None;
        let stack = Stack::new("test".to_string())
            .permission("other-cloud", other_cloud)
            .permission("reader", inline_profile("arn:aws:s3:::external-global/*"))
            .permission("writer", scoped)
            .permission(
                "catalog",
                alien_core::PermissionProfile::new().global(["storage/data-read"]),
            )
            .management(alien_core::ManagementPermissions::Extend(inline_profile(
                "arn:aws:s3:::management-only/*",
            )))
            .build();
        let mut resolved = Vec::new();
        let boundary = aws_stack_permissions_boundary(&stack, &context(), |profile, scope| {
            resolved.push((profile.to_string(), scope.to_string()));
            Ok(context()
                .with_resource_name("actual-existing-bucket")
                .with_resource_id(scope))
        })
        .unwrap();
        assert_eq!(
            resolved,
            [
                ("reader".to_string(), "*".to_string()),
                ("writer".to_string(), "uploads".to_string())
            ]
        );
        let statements = boundary["Statement"].as_array().unwrap();
        assert!(statements.iter().any(|s| s["Resource"]
            == json!(["arn:aws:s3:::external-global/*"])
            && s["Action"] == json!(["s3:GetObject"])));
        assert!(statements.iter().any(|s| s["Resource"]
            == json!(["arn:aws:s3:::actual-existing-bucket/*"])
            && s["Action"] == json!(["s3:GetObject"])
            && s["Condition"]
                == json!({"StringEquals": {"s3:ExistingObjectTag/profile": "uploads"}})));
        assert!(!boundary.to_string().contains("management-only"));
    }

    #[test]
    fn stack_boundary_preserves_inline_overrides_and_deduplicates_shared_grants() {
        let mut profile = inline_profile("arn:aws:s3:::custom-override/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        // An inline definition remains custom even when its id matches a catalog set.
        set.id = "storage/data-read".to_string();
        let one = Stack::new("test".to_string())
            .permission("one", profile.clone())
            .build();
        let two = Stack::new("test".to_string())
            .permission("one", profile.clone())
            .permission("two", profile)
            .build();
        let render = |stack: &Stack| {
            aws_stack_permissions_boundary(stack, &context(), |_, _| Ok(context())).unwrap()
        };
        let boundary = render(&one);
        assert_eq!(boundary, render(&two));
        assert!(boundary["Statement"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["Resource"] == json!(["arn:aws:s3:::custom-override/*"])));
    }

    #[test]
    fn stack_boundary_propagates_resolution_compilation_and_size_failures() {
        let stack = Stack::new("test".to_string())
            .permission("one", inline_profile("${missingVariable}"))
            .build();
        let error = aws_stack_permissions_boundary(&stack, &context(), |_, _| {
            Err(AlienError::new(ErrorData::VariableNotFound {
                variable: "actualResourceName".to_string(),
            }))
        })
        .unwrap_err();
        assert_eq!(error.code, "VARIABLE_NOT_FOUND");
        assert!(error.to_string().contains("actualResourceName"));
        let error =
            aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context())).unwrap_err();
        assert_eq!(error.code, "VARIABLE_NOT_FOUND");
        assert!(error.to_string().contains("missingVariable"));
        let stack = Stack::new("test".to_string())
            .permission("one", inline_profile(&"x".repeat(MANAGED_POLICY_LIMIT)))
            .build();
        assert_eq!(
            aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context()))
                .unwrap_err()
                .code,
            "PERMISSIONS_BOUNDARY_TOO_LARGE"
        );
    }

    #[test]
    fn control_plane_sets_are_excluded() {
        for id in [
            "worker/provision",
            "worker/management",
            "storage/heartbeat",
            "storage/telemetry",
            "guard/example",
            "sandbox/protect-setup-roles",
            "remote-stack-management/execute",
            "observe/logs",
        ] {
            assert!(!is_runtime_set(id), "{id}");
        }
        for id in [
            "worker/execute",
            "storage/data-read",
            "sandbox/remote-execute",
        ] {
            assert!(is_runtime_set(id), "{id}");
        }
    }
}
