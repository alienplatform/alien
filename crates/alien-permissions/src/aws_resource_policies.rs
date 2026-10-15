//! Pure selection and naming of inline policies written for workload resources.
//!
//! Setup and quota accounting must use the same resolved contexts and explicit
//! references. Unknown deployment variables are errors, not invented ARN values.

use crate::{
    generators::{AwsIamPolicy, AwsIamStatement, AwsRuntimePermissionsGenerator},
    BindingTarget, ErrorData, PermissionContext, Result,
};
use alien_core::{
    permissions::PermissionProfile, PermissionSet, PermissionSetReference, ServiceAccount, Stack,
    Worker,
};
use alien_error::{Context, IntoAlienError};
use tracing::debug;

/// Physical name used by AWS prefixed-resource create controllers.
pub fn aws_prefixed_resource_name(prefix: &str, resource_id: &str) -> String {
    format!("{prefix}-{resource_id}")
}

/// Extra stack-policy statements required for managed Lambda image pulls.
/// Match the ServiceAccount writer, including its empty-stack-set early return.
pub fn aws_service_account_injected_statements(
    account: &ServiceAccount,
    stack: &Stack,
    context: &PermissionContext,
) -> Vec<AwsIamStatement> {
    if account.stack_permission_sets.is_empty() {
        return Vec::new();
    }
    let Some(managing_account) = context.managing_account_id.as_deref() else {
        return Vec::new();
    };
    let profile = account.id.strip_suffix("-sa").unwrap_or(&account.id);
    if !stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<Worker>()
            .is_some_and(|worker| worker.get_permissions() == profile)
    }) {
        return Vec::new();
    }
    vec![
        AwsIamStatement {
            sid: "EcrCrossAccountAuth".into(),
            effect: "Allow".into(),
            action: vec!["ecr:GetAuthorizationToken".into()],
            resource: vec!["*".into()],
            not_resource: Vec::new(),
            condition: None,
        },
        AwsIamStatement {
            sid: "EcrCrossAccountPull".into(),
            effect: "Allow".into(),
            action: vec![
                "ecr:BatchGetImage".into(),
                "ecr:GetDownloadUrlForLayer".into(),
            ],
            resource: vec![format!("arn:aws:ecr:*:{managing_account}:repository/*")],
            not_resource: Vec::new(),
            condition: None,
        },
    ]
}

/// Inputs used to omit implicit grants already covered by the stack document.
/// Explicit resource references are retained even when a stack grant covers them.
pub struct AwsResourcePolicyContext<'a> {
    pub named_by_resource: &'a [PermissionSetReference],
    pub stack_sets: &'a [PermissionSet],
    pub generator: &'a AwsRuntimePermissionsGenerator,
    pub permission_context: &'a PermissionContext,
}

impl AwsResourcePolicyContext<'_> {
    /// Whether setup can omit this resource policy without removing permissions.
    pub fn is_redundant(&self, reference: &PermissionSetReference) -> Result<bool> {
        if self.named_by_resource.contains(reference) {
            return Ok(false);
        }
        // Aliases must be resolved before comparing with mutated identity sets.
        let stack_set = reference
            .resolve(|name| crate::get_permission_set(name).cloned())
            .and_then(|resolved| self.stack_sets.iter().find(|set| **set == resolved));
        // A set with no AWS entry renders no policy to compare; callers skip it themselves.
        let Some(set) = stack_set.filter(|set| set.platforms.aws.is_some()) else {
            return Ok(false);
        };
        let stack =
            self.generator
                .generate_policy(set, BindingTarget::Stack, self.permission_context)?;
        let resource = self.generator.generate_policy(
            set,
            BindingTarget::Resource,
            self.permission_context,
        )?;
        Ok(aws_policy_grants(&stack, &resource))
    }
}

/// Drops the wildcard sets that the service account's stack-level policy already grants for
/// this resource.
///
/// A profile's `"*"` sets are written once on its role as the stack-level policy, scoped to
/// the stack prefix. Writing them again per resource duplicates that grant, and IAM caps a
/// role's inline policies at 10,240 characters in total, so a stack with a handful of
/// resources fails setup. A set stays when the resource names it, or when the stack-level
/// grant does not reach this resource (a bring-your-own resource outside the stack prefix).
/// The runtime, Terraform and CloudFormation all select a resource's policies with this.
pub fn without_refs_the_stack_policy_grants(
    stack: &Stack,
    profile_name: &str,
    profile: &PermissionProfile,
    resource_id: &str,
    refs: Vec<PermissionSetReference>,
    generator: &AwsRuntimePermissionsGenerator,
    permission_context: &PermissionContext,
) -> Result<Vec<PermissionSetReference>> {
    let stack_sets = stack
        .resources
        .get(&format!("{profile_name}-sa"))
        .and_then(|entry| entry.config.downcast_ref::<ServiceAccount>())
        .map(|service_account| service_account.stack_permission_sets.as_slice())
        .unwrap_or_default();
    let selection = AwsResourcePolicyContext {
        named_by_resource: profile
            .0
            .get(resource_id)
            .map(Vec::as_slice)
            .unwrap_or_default(),
        stack_sets,
        generator,
        permission_context,
    };
    let mut kept = Vec::new();
    for reference in refs {
        let redundant = selection
            .is_redundant(&reference)
            .context(ErrorData::GeneratorError {
                platform: "aws".to_string(),
                message: format!(
                    "Failed to compare permission set '{}' on '{resource_id}' with the \
                     stack-level policy of profile '{profile_name}'",
                    reference.id()
                ),
            })?;
        if redundant {
            debug!(
                resource_id = %resource_id,
                profile = %profile_name,
                permission_set = %reference.id(),
                "Stack-level policy already grants this set for the resource; skipping its resource policy"
            );
        } else {
            kept.push(reference);
        }
    }
    Ok(kept)
}

/// The exact name and document supplied to IAM's PutRolePolicy operation.
/// Inserting these into a role's map by name models IAM's replacement semantics.
#[derive(Debug)]
pub struct AwsNamedResourcePolicy {
    pub name: String,
    pub document: String,
}

/// Render a resolved workload permission set using its controller's context.
/// Selection and reference resolution happen before this function is called.
pub fn render_aws_resource_policy(
    resource_id: &str,
    permission_set: &PermissionSet,
    generator: &AwsRuntimePermissionsGenerator,
    permission_context: &PermissionContext,
) -> Result<AwsNamedResourcePolicy> {
    let policy =
        generator.generate_policy(permission_set, BindingTarget::Resource, permission_context)?;
    let document = serde_json::to_string_pretty(&policy)
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: "Failed to serialize IAM policy document".to_string(),
        })?;
    Ok(AwsNamedResourcePolicy {
        name: aws_resource_policy_name(resource_id, &permission_set.id),
        document,
    })
}

/// Stable inline policy name for a published resource's Remote Bindings grant.
pub fn aws_remote_access_policy_name(resource_id: &str) -> String {
    format!("alien-{resource_id}-remote-access")
}

/// Render the registered Remote Bindings grant using the resource writer's context.
pub fn aws_remote_access_policy(
    generator: &AwsRuntimePermissionsGenerator,
    definition: &alien_core::remote_bindings::RemoteBindingDefinition,
    permission_context: &PermissionContext,
) -> Result<AwsIamPolicy> {
    let set = crate::get_permission_set(definition.permission_set).ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::InvalidPermissionSet {
            message: format!(
                "Remote Bindings permission set '{}' is not registered",
                definition.permission_set
            ),
        })
    })?;
    generator.generate_policy(set, BindingTarget::Resource, permission_context)
}

/// Whether `broad` allows every request `narrow` allows.
///
/// Answers yes only when it can prove it: every statement of `narrow` must be an `Allow` with
/// plain `Resource`s that one `Allow` statement of `broad` matches, with no condition `narrow`
/// lacks. A `Deny`, a `NotResource` or a differing condition answers no, which keeps the
/// narrow policy.
fn aws_policy_grants(broad: &AwsIamPolicy, narrow: &AwsIamPolicy) -> bool {
    let plain_allow = |statement: &AwsIamStatement| {
        statement.effect == "Allow" && statement.not_resource.is_empty()
    };
    narrow.statement.iter().all(|wanted| {
        plain_allow(wanted)
            && broad.statement.iter().any(|granted| {
                plain_allow(granted)
                    && (granted.condition.is_none() || granted.condition == wanted.condition)
                    && wanted.action.iter().all(|action| {
                        granted.action.iter().any(|pattern| {
                            iam_pattern_contains(
                                &pattern.to_ascii_lowercase(),
                                &action.to_ascii_lowercase(),
                            )
                        })
                    })
                    && wanted.resource.iter().all(|resource| {
                        granted
                            .resource
                            .iter()
                            .any(|pattern| iam_pattern_contains(pattern, resource))
                    })
            })
    })
}

/// Whether every string IAM matches with `narrow` is also matched by `pattern`, both using IAM's
/// `*` (any run of characters) and `?` (one character).
///
/// A wildcard in `narrow` is only accepted against a wildcard in `pattern` at least as wide, so
/// a `true` is always a real containment; some true containments answer `false`.
fn iam_pattern_contains(pattern: &str, narrow: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let narrow: Vec<char> = narrow.chars().collect();
    // covered[j] after processing pattern[..i]: pattern[..i] covers narrow[..j].
    let mut covered = vec![false; narrow.len() + 1];
    covered[0] = true;
    for &p in &pattern {
        let mut next = vec![false; narrow.len() + 1];
        for j in 0..=narrow.len() {
            next[j] = match p {
                // `*` covers whatever an earlier prefix covered, extended by any character.
                '*' => covered[j] || (j > 0 && next[j - 1]),
                // `?` covers one character, or a `?` in `narrow`, never a `*`.
                '?' => j > 0 && covered[j - 1] && narrow[j - 1] != '*',
                // A literal never covers a wildcard in `narrow`, which matches more than it.
                literal => j > 0 && covered[j - 1] && narrow[j - 1] == literal,
            };
        }
        covered = next;
    }
    covered[narrow.len()]
}

/// Inline policy a resource attaches to a profile's service-account role.
pub fn aws_resource_policy_name(resource_id: &str, permission_set_id: &str) -> String {
    format!(
        "alien-{}-{}",
        resource_id,
        permission_set_id.replace('/', "-")
    )
}

/// Inline policy a resource attaches to the management role.
pub fn aws_management_resource_policy_name(resource_id: &str, permission_set_id: &str) -> String {
    format!(
        "alien-mgmt-{}-{}",
        resource_id,
        permission_set_id.replace('/', "-")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_documents_preserve_setup_rendering_and_resolved_resources() {
        let generator = AwsRuntimePermissionsGenerator::new();
        let set = crate::get_permission_set("storage/data-read").unwrap();
        for resource_name in ["q5-render-objects", "q5-render-imported"] {
            let context = PermissionContext::new().with_resource_name(resource_name.to_string());
            let named = render_aws_resource_policy("objects", set, &generator, &context).unwrap();
            let expected = generator
                .generate_policy(set, BindingTarget::Resource, &context)
                .unwrap();
            assert_eq!(named.name, "alien-objects-storage-data-read");
            assert_eq!(
                named.document,
                serde_json::to_string_pretty(&expected).unwrap()
            );
            let document: serde_json::Value = serde_json::from_str(&named.document).unwrap();
            let resources = document["Statement"][0]["Resource"].as_array().unwrap();
            assert!(resources
                .iter()
                .any(|value| value == &format!("arn:aws:s3:::{resource_name}")));
        }
        let error =
            render_aws_resource_policy("objects", set, &generator, &PermissionContext::new())
                .unwrap_err();
        assert_eq!(error.code, "VARIABLE_NOT_FOUND");
    }

    #[test]
    fn selection_preserves_explicit_and_uncovered_grants() {
        let reference = PermissionSetReference::from_name("storage/data-read".to_string());
        let stack_sets = vec![crate::get_permission_set("storage/data-read")
            .unwrap()
            .clone()];
        let generator = AwsRuntimePermissionsGenerator::new();
        for (resource_name, explicit, expected) in [
            ("q5-selection-objects", false, true),
            ("q5-selection-objects", true, false),
            ("outside-prefix", false, false),
        ] {
            let context = PermissionContext::new()
                .with_stack_prefix("q5-selection".to_string())
                .with_resource_name(resource_name.to_string());
            let named = if explicit {
                vec![reference.clone()]
            } else {
                vec![]
            };
            let selection = AwsResourcePolicyContext {
                named_by_resource: &named,
                stack_sets: &stack_sets,
                generator: &generator,
                permission_context: &context,
            };
            assert_eq!(selection.is_redundant(&reference).unwrap(), expected);
        }
        let selection = AwsResourcePolicyContext {
            named_by_resource: &[],
            stack_sets: &[],
            generator: &generator,
            permission_context: &PermissionContext::new(),
        };
        assert!(!selection.is_redundant(&reference).unwrap());
    }

    #[test]
    fn selection_propagates_missing_context_without_claiming_coverage() {
        let reference = PermissionSetReference::from_name("storage/data-read".to_string());
        let stack_sets = vec![crate::get_permission_set("storage/data-read")
            .unwrap()
            .clone()];
        let selection = AwsResourcePolicyContext {
            named_by_resource: &[],
            stack_sets: &stack_sets,
            generator: &AwsRuntimePermissionsGenerator::new(),
            permission_context: &PermissionContext::new(),
        };
        let error = selection.is_redundant(&reference).unwrap_err();
        assert_eq!(error.code, "VARIABLE_NOT_FOUND");
        assert!(!error.retryable);
    }

    /// A failed comparison names the set it was comparing, since a profile grants many.
    #[test]
    fn a_failed_comparison_names_the_set() {
        let profile = PermissionProfile::new().global(["storage/data-read"]);
        let stack = Stack::new("q5-naming".to_string())
            .add(
                ServiceAccount::from_permission_profile(
                    "execution-sa".to_string(),
                    &profile,
                    |name| crate::get_permission_set(name).cloned(),
                )
                .unwrap(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let error = without_refs_the_stack_policy_grants(
            &stack,
            "execution",
            &profile,
            "objects",
            vec![PermissionSetReference::from_name(
                "storage/data-read".to_string(),
            )],
            &AwsRuntimePermissionsGenerator::new(),
            &PermissionContext::new(),
        )
        .unwrap_err();
        assert_eq!(error.code, "GENERATOR_ERROR");
        assert!(error.message.contains("'storage/data-read'"), "{error}");
        assert!(!error.retryable);
    }

    /// A multi-cloud profile can grant an Azure-only set under `"*"`; on AWS it is kept for the
    /// emitter's own skip, not compared.
    #[test]
    fn a_set_without_an_aws_entry_is_kept_without_comparing() {
        let profile = PermissionProfile::new().global(["sandbox/images"]);
        let stack = Stack::new("q5-azure-only".to_string())
            .add(
                ServiceAccount::from_permission_profile(
                    "execution-sa".to_string(),
                    &profile,
                    |name| crate::get_permission_set(name).cloned(),
                )
                .unwrap(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let reference = PermissionSetReference::from_name("sandbox/images".to_string());
        let kept = without_refs_the_stack_policy_grants(
            &stack,
            "execution",
            &profile,
            "agents",
            vec![reference.clone()],
            &AwsRuntimePermissionsGenerator::new(),
            &PermissionContext::new().with_stack_prefix("q5-azure-only".to_string()),
        )
        .unwrap();
        assert_eq!(kept, vec![reference]);
    }

    fn aws_policies(set_id: &str, resource_name: &str) -> (AwsIamPolicy, AwsIamPolicy) {
        let set = crate::get_permission_set(set_id)
            .unwrap_or_else(|| panic!("permission set '{set_id}' is registered"));
        let context = PermissionContext::new()
            .with_aws_account_id("123456789012".to_string())
            .with_aws_region("us-east-1".to_string())
            .with_stack_prefix("p26eed71".to_string())
            .with_resource_id("res".to_string())
            .with_resource_name(resource_name.to_string());
        let generator = AwsRuntimePermissionsGenerator::new();
        (
            generator
                .generate_policy(set, BindingTarget::Stack, &context)
                .unwrap(),
            generator
                .generate_policy(set, BindingTarget::Resource, &context)
                .unwrap(),
        )
    }

    /// Every `"*"` set of a typical worker stack, for resources named under the stack prefix:
    /// the stack-level policy already grants each one, so none needs a policy per resource.
    #[test]
    fn stack_level_policy_grants_wildcard_sets_for_resources_under_the_stack_prefix() {
        for (set_id, resource_name) in [
            ("storage/data-read", "p26eed71-test-alien-storage"),
            ("storage/data-write", "p26eed71-test-alien-storage"),
            ("build/execute", "p26eed71-test-alien-build"),
            (
                "artifact-registry/pull",
                "p26eed71-test-alien-artifact-registry",
            ),
            (
                "artifact-registry/push",
                "p26eed71-test-alien-artifact-registry",
            ),
            (
                "artifact-registry/provision",
                "p26eed71-test-alien-artifact-registry",
            ),
            ("vault/data-read", "p26eed71-test-vault"),
            ("vault/data-write", "p26eed71-secrets"),
            ("kv/data-read", "p26eed71-test-alien-kv"),
            ("kv/data-write", "p26eed71-test-alien-kv"),
            ("queue/data-read", "p26eed71-test-alien-queue"),
            ("queue/data-write", "p26eed71-test-alien-queue"),
        ] {
            let (stack, resource) = aws_policies(set_id, resource_name);
            assert!(
                aws_policy_grants(&stack, &resource),
                "{set_id} on {resource_name}: stack {stack:#?} resource {resource:#?}"
            );
        }
    }

    /// A bring-your-own bucket is named outside the stack prefix, so only its own policy
    /// reaches it.
    #[test]
    fn stack_level_policy_does_not_grant_a_resource_outside_the_stack_prefix() {
        for set_id in ["storage/data-read", "storage/data-write"] {
            let (stack, resource) = aws_policies(set_id, "customer-data");
            assert!(!aws_policy_grants(&stack, &resource), "{set_id}");
        }
    }

    #[test]
    fn iam_pattern_containment_never_claims_a_wider_pattern() {
        // (pattern, narrow, pattern matches everything narrow matches)
        for (pattern, narrow, contains) in [
            ("arn:aws:s3:::p-*", "arn:aws:s3:::p-bucket", true),
            ("arn:aws:s3:::p-*", "arn:aws:s3:::p-bucket/*", true),
            (
                "arn:aws:ecr:*:1:repository/p-*",
                "arn:aws:ecr:us-east-1:1:repository/p-r-*",
                true,
            ),
            ("role/p-*-pull", "role/p-registry-pull", true),
            ("*", "*", true),
            ("a*", "a?", true),
            ("a?c", "a?c", true),
            ("arn:aws:s3:::p-*", "arn:aws:s3:::customer", false),
            ("arn:aws:s3:::p-bucket", "arn:aws:s3:::p-*", false),
            ("a?c", "a*c", false),
            ("abc", "a?c", false),
            ("role/p-*-pull", "role/p-registry-push", false),
        ] {
            assert_eq!(
                iam_pattern_contains(pattern, narrow),
                contains,
                "{pattern} vs {narrow}"
            );
        }
    }

    /// A broad statement with a condition grants less than an unconditional one.
    #[test]
    fn conditioned_stack_statement_does_not_grant_an_unconditioned_one() {
        let (mut stack, resource) = aws_policies("kv/data-read", "p26eed71-test-alien-kv");
        assert!(aws_policy_grants(&stack, &resource));
        for statement in &mut stack.statement {
            statement.condition = Some(
                [(
                    "StringEquals".to_string(),
                    [("aws:ResourceTag/owner".to_string(), "x".to_string())].into(),
                )]
                .into(),
            );
        }
        assert!(!aws_policy_grants(&stack, &resource));
    }
}
