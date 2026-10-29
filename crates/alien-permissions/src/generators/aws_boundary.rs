//! The setup-owned ceiling for roles written by management.

use std::{borrow::Cow, collections::BTreeSet};

use alien_core::{
    AwsBindingSpec, PermissionSet, PermissionSetReference, Stack, ALIEN_STACK_TAG_KEY,
};

use alien_error::AlienError;
use serde_json::{json, Value};

use crate::{
    aws_resource_policies::without_refs_the_stack_policy_grants, get_permission_set,
    list_permission_set_ids, BindingTarget, ErrorData, PermissionContext, Result,
};

use super::{AwsIamStatement, AwsRuntimePermissionsGenerator};

const MANAGED_POLICY_LIMIT: usize = 6_144;

/// Render the boundary including inline and ARN-selected application grants.
///
/// The resolver receives the profile name and scope (`"*"` or a resource id). It must
/// return the same context used to compile that identity's grants, including actual
/// resource names for resource scopes. It is called only for scopes with inline AWS
/// permissions or catalog bindings that select complete resource ARNs. Management
/// permissions are deliberately not part of this ceiling. Other named catalog grants
/// are already included by [`aws_permissions_boundary`].
/// Global selected sets also contribute their resource binding to matching resource
/// types, just as workload policy writers do. Those calls resolve the physical
/// resource context, even when the profile does not explicitly name the resource.
pub fn aws_stack_permissions_boundary(
    stack: &Stack,
    context: &PermissionContext,
    mut resolve_context: impl FnMut(&str, &str) -> Result<PermissionContext>,
) -> Result<Value> {
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut statements = Vec::new();
    for (profile_name, profile) in stack.permission_profiles() {
        for (scope, references) in &profile.0 {
            let resolved_sets: Vec<_> = references
                .iter()
                .filter_map(resolved_boundary_set)
                .collect();
            if resolved_sets.is_empty() {
                continue;
            }
            let grant_context = resolve_context(profile_name, scope)?;
            let target = if scope == "*" {
                BindingTarget::Stack
            } else {
                BindingTarget::Resource
            };
            for set in resolved_sets {
                statements.extend(
                    generator
                        .generate_policy(set, target, &grant_context)?
                        .statement,
                );
            }
        }
        for (resource_id, entry) in stack.resources() {
            let type_prefix = format!("{}/", entry.config.resource_type());
            let references: Vec<_> = profile
                .0
                .get("*")
                .into_iter()
                .flatten()
                .filter(|reference| {
                    reference.id().starts_with(&type_prefix)
                        && resolved_boundary_set(reference).is_some()
                })
                .cloned()
                .collect();
            if references.is_empty() {
                continue;
            }
            let grant_context = resolve_context(profile_name, resource_id)?;
            let references = without_refs_the_stack_policy_grants(
                stack,
                profile_name,
                profile,
                resource_id,
                references,
                &generator,
                &grant_context,
            )?;
            for set in references.iter().filter_map(resolved_boundary_set) {
                statements.extend(
                    generator
                        .generate_policy(set, BindingTarget::Resource, &grant_context)?
                        .statement,
                );
            }
        }
    }
    aws_permissions_boundary(context, &statements)
}

/// Render the stack permissions boundary from the runtime catalog and compiled inline grants.
///
/// `custom_statements` contains runtime inline grants and catalog grants selecting complete
/// resource ARNs, compiled with their actual resource contexts. Use [`aws_stack_permissions_boundary`]
/// to collect these from a stack. Setup must approve changes to these grants. Only name-based
/// catalog bindings are widened to the stack prefix; complete ARN selectors cannot be widened.
/// Existing stack ownership tags provide a non-IAM ceiling for unprefixed resources.
/// Other unprefixed grants retain their actions and conditions.
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
    let ownership_key = format!("aws:ResourceTag/{ALIEN_STACK_TAG_KEY}");
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
                // Complete ARN selectors (such as KMS keys) require the stack resolver.
                binding
                    .resources
                    .retain(|resource| resource != "${resourceName}");
                if binding.resources.is_empty() && binding.not_resources.is_empty() {
                    continue;
                }
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
                    // Existing ownership covers non-IAM runtime operations even when
                    // identity policies impose additional service-specific conditions.
                    if statement
                        .condition
                        .as_ref()
                        .and_then(|conditions| conditions.get("StringEquals"))
                        .and_then(|values| values.get(&ownership_key))
                        .is_some_and(|value| value == prefix)
                        && statement.action.iter().all(|action| {
                            action.split_once(':').is_some_and(|(service, _)| {
                                !service.eq_ignore_ascii_case("iam")
                                    && !service.contains(['*', '?'])
                            })
                        })
                    {
                        continue;
                    }
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
    let mut rendered = vec![
        json!({
            "Effect": "Allow", "NotAction": "iam:*", "Resource": scoped_resources,
        }),
        json!({
            "Effect": "Allow", "NotAction": "iam:*", "Resource": "*",
            "Condition": {"StringEquals": {ownership_key: prefix}},
        }),
    ];
    rendered.extend(statements.iter().map(render_statement));
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

fn resolved_boundary_set(reference: &PermissionSetReference) -> Option<&PermissionSet> {
    match reference {
        PermissionSetReference::Inline(set) if set.platforms.aws.is_some() => Some(set),
        PermissionSetReference::Name(id) if is_runtime_set(id) => {
            let set = get_permission_set(id)?;
            set.platforms
                .aws
                .as_ref()?
                .iter()
                .any(|entry| {
                    [&entry.binding.stack, &entry.binding.resource]
                        .into_iter()
                        .flatten()
                        .any(|binding| {
                            binding
                                .resources
                                .iter()
                                .any(|resource| resource == "${resourceName}")
                        })
                })
                .then_some(set)
        }
        _ => None,
    }
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

fn render_statement(statement: &AwsIamStatement) -> Value {
    let mut value = json!({"Effect": statement.effect, "Action": statement.action});
    if !statement.resource.is_empty() {
        value["Resource"] = json!(statement.resource);
    }
    if !statement.not_resource.is_empty() {
        value["NotResource"] = json!(statement.not_resource);
    }
    if let Some(condition) = &statement.condition {
        value["Condition"] = json!(condition);
    }
    for key in ["Action", "Resource", "NotResource"] {
        if let Some(values) = value[key].as_array().filter(|values| values.len() == 1) {
            value[key] = values[0].clone();
        }
    }
    value
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

    #[test]
    fn catalog_and_individual_arn_grants_fit_and_cover_runtime_permissions() {
        let context = context();
        let policy = aws_permissions_boundary(&context, &[]).unwrap();
        assert!(policy.to_string().len() <= MANAGED_POLICY_LIMIT);
        eprintln!(
            "Name-scoped runtime catalog boundary: {} characters",
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
                    let key_context = context.clone().with_resource_name("arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab");
                    let arn_selected =
                        resolved_boundary_set(&PermissionSetReference::from_name(id)).is_some();
                    let actual_context = if arn_selected { &key_context } else { &context };
                    let required = generator
                        .generate_policy(&single, target, actual_context)
                        .unwrap();
                    let boundary = if arn_selected {
                        aws_permissions_boundary(&context, &required.statement).unwrap()
                    } else {
                        policy.clone()
                    };
                    assert!(
                        super::super::aws_permissions_boundary_covers(
                            &boundary,
                            &serde_json::to_value(required).unwrap(),
                        )
                        .unwrap(),
                        "Boundary does not cover {id} ({target:?})"
                    );
                }
            }
        }
    }

    #[test]
    fn ownership_ceiling_requires_existing_tags_and_excludes_iam() {
        let context = context();
        let boundary = aws_permissions_boundary(&context, &[]).unwrap();
        let key = format!("aws:ResourceTag/{ALIEN_STACK_TAG_KEY}");
        for (action, condition, covered) in [
            (
                "ec2:ModifyVolume",
                json!({"StringEquals": {&key: context.stack_prefix}}),
                true,
            ),
            (
                "ec2:ModifyVolume",
                json!({"StringEquals": {&key: "another-stack"}}),
                false,
            ),
            (
                "ec2:ModifyVolume",
                json!({"StringEqualsIfExists": {&key: context.stack_prefix}}),
                false,
            ),
            (
                "ec2:ModifyVolume",
                json!({"StringEquals": {"aws:RequestTag/deployment": context.stack_prefix}}),
                false,
            ),
            (
                "iam:PutRolePolicy",
                json!({"StringEquals": {&key: context.stack_prefix}}),
                false,
            ),
        ] {
            let required = json!({"Version": "2012-10-17", "Statement": [{
                "Effect": "Allow", "Action": action, "Resource": "*", "Condition": condition,
            }]});
            assert_eq!(
                super::super::aws_permissions_boundary_covers(&boundary, &required).unwrap(),
                covered,
                "{required}"
            );
        }
    }

    #[test]
    fn arn_selected_catalog_grants_use_exact_resources_and_conditions() {
        let key_arn =
            "arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab";
        for id in [
            "key/availability",
            "key/native-storage",
            "key/native-volume",
            "key/remote-cryptography",
        ] {
            for global in [false, true] {
                let profile = alien_core::PermissionProfile(indexmap::indexmap! {
                    (if global { "*" } else { "encryption" }).to_string() => vec![PermissionSetReference::from_name(id)]
                });
                let stack = Stack::new("test".to_string())
                    .permission("application", profile)
                    .add(
                        alien_core::Key::new("encryption".to_string()).build(),
                        alien_core::ResourceLifecycle::Frozen,
                    )
                    .build();
                if global {
                    assert_eq!(
                        aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context()))
                            .unwrap_err()
                            .code,
                        "BINDING_TARGET_NOT_SUPPORTED"
                    );
                    continue;
                }
                let boundary = aws_stack_permissions_boundary(&stack, &context(), |_, scope| {
                    Ok(if scope == "*" {
                        context()
                    } else {
                        context().with_resource_name(key_arn)
                    })
                })
                .unwrap();
                let required = serde_json::to_value(
                    AwsRuntimePermissionsGenerator::new()
                        .generate_policy(
                            get_permission_set(id).unwrap(),
                            BindingTarget::Resource,
                            &context().with_resource_name(key_arn),
                        )
                        .unwrap(),
                )
                .unwrap();
                assert!(
                    super::super::aws_permissions_boundary_covers(&boundary, &required).unwrap(),
                    "{id}, global={global}"
                );
                let sibling = serde_json::from_str(&required.to_string().replace(
                    "1234abcd-12ab-34cd-56ef-1234567890ab",
                    "9876abcd-12ab-34cd-56ef-1234567890ab",
                ))
                .unwrap();
                assert!(
                    !super::super::aws_permissions_boundary_covers(&boundary, &sibling).unwrap()
                );
                if required["Statement"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|statement| statement.get("Condition").is_some())
                {
                    let mut unconditional = required.clone();
                    for statement in unconditional["Statement"].as_array_mut().unwrap() {
                        statement.as_object_mut().unwrap().remove("Condition");
                    }
                    assert!(!super::super::aws_permissions_boundary_covers(
                        &boundary,
                        &unconditional
                    )
                    .unwrap());
                }
                assert_eq!(
                    aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context()))
                        .unwrap_err()
                        .code,
                    "VARIABLE_NOT_FOUND"
                );
            }
        }
    }

    #[test]
    fn combined_arn_selected_grants_fit_with_longest_prefix() {
        let stack = Stack::new("test".to_string())
            .permission(
                "application",
                alien_core::PermissionProfile::new().resource(
                    "encryption",
                    [
                        "key/availability",
                        "key/native-storage",
                        "key/native-volume",
                        "key/remote-cryptography",
                    ],
                ),
            )
            .add(
                alien_core::Key::new("encryption".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let render = |context: PermissionContext| {
            aws_stack_permissions_boundary(&stack, &context, |_, _| {
                Ok(context.clone().with_resource_name(
                    "arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab",
                ))
            })
        };
        let boundary = render(context()).unwrap();
        assert!(boundary.to_string().len() <= MANAGED_POLICY_LIMIT);
        eprintln!(
            "Combined boundary at 40-character prefix: {} characters",
            boundary.to_string().len()
        );
        for id in [
            "key/availability",
            "key/native-storage",
            "key/native-volume",
            "key/remote-cryptography",
        ] {
            let required = AwsRuntimePermissionsGenerator::new().generate_policy(
                get_permission_set(id).unwrap(), BindingTarget::Resource,
                &context().with_resource_name("arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab"),
            ).unwrap();
            assert!(super::super::aws_permissions_boundary_covers(
                &boundary,
                &serde_json::to_value(required).unwrap()
            )
            .unwrap());
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
            .filter(|s| s["Resource"] == json!(first.resource[0]))
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
        assert!(statements
            .iter()
            .any(|s| s["Resource"] == json!("arn:aws:s3:::external-global/*")
                && s["Action"] == json!("s3:GetObject")));
        assert!(statements.iter().any(|s| s["Resource"]
            == json!("arn:aws:s3:::actual-existing-bucket/*")
            && s["Action"] == json!("s3:GetObject")
            && s["Condition"]
                == json!({"StringEquals": {"s3:ExistingObjectTag/profile": "uploads"}})));
        assert!(!boundary.to_string().contains("management-only"));
    }

    #[test]
    fn global_inline_resource_grants_cover_imported_names_and_preserve_conditions() {
        let mut profile = inline_profile("arn:aws:s3:::${stackPrefix}-*/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        set.id = "storage/custom-read".to_string();
        let set = set.clone();
        let account = alien_core::ServiceAccount::from_permission_profile(
            "reader-sa".to_string(),
            &profile,
            |_| None,
        )
        .unwrap();
        let stack = Stack::new("test".to_string())
            .permission("reader", profile.clone())
            .add(account, alien_core::ResourceLifecycle::Frozen)
            .add(
                alien_core::Storage::new("uploads".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("archive".to_string()).build(),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                alien_core::Queue::new("jobs".to_string()).build(),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        let mut resolved = Vec::new();
        let boundary = aws_stack_permissions_boundary(&stack, &context(), |name, scope| {
            assert_eq!(name, "reader");
            resolved.push(scope.to_string());
            Ok(context()
                .with_resource_id(scope)
                .with_resource_name(format!("imported-{scope}")))
        })
        .unwrap();
        assert_eq!(resolved, ["*", "uploads", "archive"]);
        let generator = AwsRuntimePermissionsGenerator::new();
        for id in ["uploads", "archive"] {
            let resource_context = context()
                .with_resource_id(id)
                .with_resource_name(format!("imported-{id}"));
            let selected = without_refs_the_stack_policy_grants(
                &stack,
                "reader",
                &profile,
                id,
                vec![PermissionSetReference::Inline(set.clone())],
                &generator,
                &resource_context,
            )
            .unwrap();
            assert_eq!(selected.len(), 1);
            let required = serde_json::to_value(
                generator
                    .generate_policy(&set, BindingTarget::Resource, &resource_context)
                    .unwrap(),
            )
            .unwrap();
            assert!(super::super::aws_permissions_boundary_covers(&boundary, &required).unwrap());
            let mut unconditional = required.clone();
            for statement in unconditional["Statement"].as_array_mut().unwrap() {
                statement.as_object_mut().unwrap().remove("Condition");
            }
            assert!(
                !super::super::aws_permissions_boundary_covers(&boundary, &unconditional).unwrap()
            );
        }
    }

    #[test]
    fn implicit_grants_reuse_runtime_redundancy_selection() {
        let mut profile = inline_profile("arn:aws:s3:::${stackPrefix}-*/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        set.id = "storage/custom-read".to_string();
        let set = set.clone();
        let account = alien_core::ServiceAccount::from_permission_profile(
            "reader-sa".to_string(),
            &profile,
            |_| None,
        )
        .unwrap();
        let mut stack = Stack::new("test".to_string())
            .permission("reader", profile)
            .add(account, alien_core::ResourceLifecycle::Frozen)
            .add(
                alien_core::Storage::new("uploads".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let render = |stack: &Stack| {
            aws_stack_permissions_boundary(stack, &context(), |_, scope| {
                Ok(context()
                    .with_resource_id(scope)
                    .with_resource_name(format!("{}-{scope}", context().stack_prefix.unwrap())))
            })
            .unwrap()
        };
        let globals = AwsRuntimePermissionsGenerator::new()
            .generate_policy(&set, BindingTarget::Stack, &context())
            .unwrap();
        let expected = aws_permissions_boundary(&context(), &globals.statement).unwrap();
        assert_eq!(render(&stack), expected);
        // Explicit grants are retained by runtime even when a global grant covers them.
        stack
            .permissions
            .profiles
            .get_mut("reader")
            .unwrap()
            .0
            .insert(
                "uploads".to_string(),
                vec![PermissionSetReference::Inline(set)],
            );
        let explicit = render(&stack);
        assert_ne!(explicit, expected);
        assert!(super::super::aws_permissions_boundary_covers(&explicit, &expected).unwrap());
        // The implicit and explicit copies of the conditional grant merge into one.
        assert_eq!(
            explicit["Statement"].as_array().unwrap().len(),
            expected["Statement"].as_array().unwrap().len() + 1
        );
    }

    #[test]
    fn implicit_resource_grants_count_toward_the_boundary_size_limit() {
        let mut profile = inline_profile("arn:aws:s3:::${stackPrefix}-*/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        set.id = "storage/custom-read".to_string();
        let mut stack = Stack::new("test".to_string()).permission("reader", profile);
        for index in 0..24 {
            let id = format!("uploads-{index}");
            stack = stack.add(
                alien_core::Storage::new(id).build(),
                alien_core::ResourceLifecycle::Frozen,
            );
        }
        let stack = stack.build();
        let error = aws_stack_permissions_boundary(&stack, &context(), |_, scope| {
            Ok(context()
                .with_resource_id(scope)
                .with_resource_name(format!("imported-{scope}")))
        })
        .unwrap_err();
        assert_eq!(error.code, "PERMISSIONS_BOUNDARY_TOO_LARGE");
    }

    #[test]
    fn implicit_resource_context_failures_are_not_hidden_by_the_stack_binding() {
        let mut profile = inline_profile("arn:aws:s3:::${stackPrefix}-*/*");
        let PermissionSetReference::Inline(set) = &mut profile.0["*"][0] else {
            unreachable!()
        };
        set.id = "storage/custom-read".to_string();
        let stack = Stack::new("test".to_string())
            .permission("reader", profile)
            .add(
                alien_core::Storage::new("uploads".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let error = aws_stack_permissions_boundary(&stack, &context(), |_, scope| {
            if scope == "*" {
                Ok(context())
            } else {
                Err(AlienError::new(ErrorData::VariableNotFound {
                    variable: "importedBucketName".to_string(),
                }))
            }
        })
        .unwrap_err();
        assert_eq!(error.code, "VARIABLE_NOT_FOUND");
        assert!(error.to_string().contains("importedBucketName"));
        // A resolver that supplies no physical name must fail during interpolation too.
        let error =
            aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context())).unwrap_err();
        assert_eq!(error.code, "VARIABLE_NOT_FOUND");
        assert!(error.to_string().contains("resourceName"));
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
            .any(|s| s["Resource"] == json!("arn:aws:s3:::custom-override/*")));
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
