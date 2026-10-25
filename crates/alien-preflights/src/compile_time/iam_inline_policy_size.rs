//! Definite IAM inline-policy overflow from global workload and direct identity grants, before setup.
//!
//! Only documents rendered without deployment variables contribute to this lower
//! bound. Resource-scoped grants, injected grants and existing role policies can
//! consume additional headroom; final render and controller guards still apply.

use crate::{
    error::{ErrorData, Result},
    CheckResult, CompileTimeCheck,
};
use alien_core::{Platform, ServiceAccount, Stack};
use alien_error::{Context, IntoAlienError};
use alien_permissions::{
    generators::aws_runtime::{AwsIamPolicy, AwsIamStatement, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};

/// Rejects definite overflow in workload and direct identity global inline policies.
pub struct IamInlinePolicySizeCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for IamInlinePolicySizeCheck {
    fn code(&self) -> Option<&'static str> {
        Some("IAM_INLINE_POLICY_SIZE")
    }

    fn description(&self) -> &'static str {
        "IAM role global inline policies fit the 10240 character limit"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Aws
            && (!stack.permission_profiles().is_empty()
                || stack
                    .resources()
                    .any(|(_, entry)| entry.config.downcast_ref::<ServiceAccount>().is_some()))
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        if !self.should_run(stack, platform) {
            return Ok(CheckResult::success());
        }
        let generator = AwsRuntimePermissionsGenerator::new();
        let context = PermissionContext::new();
        let mut result = CheckResult::success();
        for (profile_name, profile) in stack.permission_profiles() {
            // This profile creates the separate management identity, whose grants
            // use a different document/chunking layout.
            if profile_name == "management" {
                continue;
            }
            let mut statements = Vec::new();
            let mut unresolved = Vec::new();
            // Resource controllers render and deduplicate these documents. A
            // profile with only concrete grants still needs an incomplete-size
            // warning; absence of a global document does not prove headroom.
            if profile
                .0
                .iter()
                .any(|(resource, grants)| resource != "*" && !grants.is_empty())
            {
                unresolved.push("resource-scoped grants".to_string());
            }
            for reference in profile.0.get("*").into_iter().flatten() {
                let Some(set) =
                    reference.resolve(|name| alien_permissions::get_permission_set(name).cloned())
                else {
                    // PermissionSetsExistCheck reports unknown names.
                    unresolved.push(reference.id().to_string());
                    continue;
                };
                match generator.generate_policy(&set, BindingTarget::Stack, &context) {
                    Ok(policy) => statements.extend(policy.statement),
                    Err(_) => unresolved.push(reference.id().to_string()),
                }
            }
            check_rendered_statements(
                &format!("permission profile '{profile_name}'"),
                statements,
                unresolved,
                &mut result,
            )?;
        }
        for (_, entry) in stack.resources() {
            let Some(account) = entry.config.downcast_ref::<ServiceAccount>() else {
                continue;
            };
            // A matching profile replaces this config during identity mutation;
            // check that profile above instead of rejecting discarded grants.
            if account.id.strip_suffix("-sa").is_some_and(|profile| {
                profile != "management" && stack.permission_profiles().contains_key(profile)
            }) {
                continue;
            }
            let mut statements = Vec::new();
            let mut unresolved = Vec::new();
            for set in &account.stack_permission_sets {
                match generator.generate_policy(set, BindingTarget::Stack, &context) {
                    Ok(policy) => statements.extend(policy.statement),
                    Err(_) => unresolved.push(set.id.clone()),
                }
            }
            // Concrete grants are installed by their resource controllers, in
            // separate documents with resource-specific contexts and layouts.
            // Do not infer those documents from the identity config alone.
            if !account.resource_permission_sets.is_empty() {
                unresolved.push("resource-scoped grants".to_string());
            }
            check_rendered_statements(
                &format!("service account '{}'", account.id),
                statements,
                unresolved,
                &mut result,
            )?;
        }
        Ok(result)
    }
}

fn check_rendered_statements(
    role: &str,
    statements: Vec<AwsIamStatement>,
    unresolved: Vec<String>,
    result: &mut CheckResult,
) -> Result<()> {
    if !statements.is_empty() {
        // Direct setup combines these statements; separate rendered documents
        // add at least as much overhead, so this remains a lower bound.
        let policy = AwsIamPolicy {
            version: "2012-10-17".to_string(),
            statement: statements,
        };
        let document = serde_json::to_string(&policy).into_alien_error().context(
            ErrorData::CompileTimeCheckFailed {
                check_name: "IAM inline policy size".to_string(),
                message: "Could not serialize the combined IAM policy".to_string(),
                resource_id: None,
            },
        )?;
        let size = document
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .count();
        if size > 10_240 {
            result.add_error(format!(
                "IAM role for {role} needs at least {size} inline policy characters; the aggregate per-role limit is 10240. Reduce global grants or use managed policies. https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_iam-quotas.html"
            ));
        }
    }
    if !unresolved.is_empty() {
        result.add_warning(format!(
            "IAM inline policy size for {role} is incomplete: grants [{}] need deployment values or could not be rendered. Only rendered global grants were counted; passing does not establish total role headroom.",
            unresolved.join(", ")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{PermissionProfile, PermissionSetReference};

    fn grant(id: &str, count: usize, dynamic: bool) -> PermissionSetReference {
        let resources: Vec<_> = (0..count)
            .map(|index| {
                if dynamic {
                    format!("arn:aws:s3:::${{stackPrefix}}/object-{index:04}")
                } else {
                    format!("arn:aws:s3:::q5-policy-control/objects/object-{index:04}")
                }
            })
            .collect();
        PermissionSetReference::from_inline(
            serde_json::from_value(serde_json::json!({
                "id": id, "description": "Read synthetic objects",
                "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]},
                    "binding": {"stack": {"resources": resources}}}]}
            }))
            .unwrap(),
        )
    }

    fn stack(profiles: &[(&str, Vec<PermissionSetReference>)]) -> Stack {
        let mut stack = Stack::new("q5-policy".into()).build();
        for (name, grants) in profiles {
            let mut profile = PermissionProfile::new();
            profile.0.insert("*".into(), grants.clone());
            stack.permissions.profiles.insert((*name).into(), profile);
        }
        stack
    }

    #[tokio::test]
    async fn build_runner_rejects_combined_grants_and_reports_every_role() {
        // Each set fits alone. Together they overflow one role; both roles
        // must appear in the single build failure, without any cloud clients.
        let grants = vec![grant("first", 100, false), grant("second", 100, false)];
        for reference in &grants {
            let control = IamInlinePolicySizeCheck
                .check(
                    &stack(&[("control", vec![reference.clone()])]),
                    Platform::Aws,
                )
                .await
                .unwrap();
            assert!(control.success, "{:?}", control.errors);
        }
        let stack = stack(&[("execution", grants.clone()), ("reader", grants)]);
        let error = crate::runner::PreflightRunner::new()
            .run_build_time_preflights(&stack, Platform::Aws)
            .await
            .unwrap_err();
        assert_eq!(error.code, "VALIDATION_FAILED");
        let Some(ErrorData::ValidationFailed { results, .. }) = &error.error else {
            panic!("expected structured validation results: {error}");
        };
        let iam = results
            .iter()
            .find(|result| result.code.as_deref() == Some("IAM_INLINE_POLICY_SIZE"))
            .unwrap();
        assert_eq!(iam.errors.len(), 2);
        assert!(iam.warnings.is_empty());
        let message = iam.errors.join("\n");
        assert!(message.contains("execution"), "{message}");
        assert!(message.contains("reader"), "{message}");
        assert!(message.contains("10240"), "{message}");
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn unresolved_grants_warn_without_hiding_definite_overflow() {
        for (count, success) in [(10, true), (400, false)] {
            let result = IamInlinePolicySizeCheck
                .check(
                    &stack(&[(
                        "execution",
                        vec![grant("literal", count, false), grant("dynamic", 400, true)],
                    )]),
                    Platform::Aws,
                )
                .await
                .unwrap();
            assert_eq!(result.success, success);
            assert_eq!(result.errors.len(), usize::from(!success));
            assert_eq!(result.warnings.len(), 1);
            assert!(result.warnings[0].contains("dynamic"));
        }
    }

    #[tokio::test]
    async fn concrete_profile_grants_warn_with_or_without_global_grants() {
        for global_count in [None, Some(10), Some(400)] {
            let mut profile = PermissionProfile::new();
            profile
                .0
                .insert("objects".into(), vec![grant("concrete", 400, false)]);
            if let Some(count) = global_count {
                profile
                    .0
                    .insert("*".into(), vec![grant("global", count, false)]);
            }
            let mut stack = Stack::new("q5-policy".into()).build();
            stack.permissions.profiles.insert("reader".into(), profile);
            let summary = crate::runner::PreflightRunner::new()
                .run_compile_time_checks(&stack, Platform::Aws)
                .await
                .unwrap();
            let result = summary
                .results
                .iter()
                .find(|result| result.code.as_deref() == Some("IAM_INLINE_POLICY_SIZE"))
                .unwrap();
            let overflow = global_count == Some(400);
            assert_eq!(result.success, !overflow);
            assert_eq!(result.errors.len(), usize::from(overflow));
            assert_eq!(result.warnings.len(), 1);
            assert!(result.warnings[0].contains("reader"));
            assert!(result.warnings[0].contains("resource-scoped grants"));
            assert!(result.warnings[0].contains("passing does not establish total role headroom"));
        }
    }

    fn direct_account(id: &str, count: usize) -> ServiceAccount {
        ServiceAccount::new(id.to_string())
            .stack_permission_set(grant("first", count, false).resolve(|_| None).unwrap())
            .stack_permission_set(grant("second", count, false).resolve(|_| None).unwrap())
            .build()
    }

    #[tokio::test]
    async fn direct_accounts_are_checked_together_before_setup() {
        for (count, success) in [(10, true), (100, false)] {
            let stack = Stack::new("q5-direct".to_string())
                .add(
                    direct_account("reader", count),
                    alien_core::ResourceLifecycle::Frozen,
                )
                .add(
                    direct_account("writer", count),
                    alien_core::ResourceLifecycle::Frozen,
                )
                .build();
            assert!(IamInlinePolicySizeCheck.should_run(&stack, Platform::Aws));
            let summary = crate::runner::PreflightRunner::new()
                .run_compile_time_checks(&stack, Platform::Aws)
                .await
                .unwrap();
            let result = summary
                .results
                .iter()
                .find(|result| result.code.as_deref() == Some("IAM_INLINE_POLICY_SIZE"))
                .unwrap();
            assert_eq!(result.success, success);
            assert_eq!(result.errors.len(), if success { 0 } else { 2 });
            assert!(result.warnings.is_empty());
            if !success {
                assert!(result.errors[0].contains("reader"));
                assert!(result.errors[1].contains("writer"));
            }
        }
    }

    #[tokio::test]
    async fn profile_replacement_does_not_reject_discarded_direct_grants() {
        let mut stack = stack(&[("reader", vec![grant("small", 10, false)])]);
        let account = direct_account("reader-sa", 100);
        stack.resources.insert(
            account.id.clone(),
            alien_core::ResourceEntry {
                config: alien_core::Resource::new(account),
                lifecycle: alien_core::ResourceLifecycle::Frozen,
                enabled_when: None,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
            },
        );
        let result = IamInlinePolicySizeCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[tokio::test]
    async fn direct_dynamic_and_resource_grants_warn_without_masking_overflow() {
        let mut account = direct_account("reader", 100);
        account
            .stack_permission_sets
            .push(grant("dynamic", 400, true).resolve(|_| None).unwrap());
        account.resource_permission_sets.insert(
            "objects".to_string(),
            vec![grant("concrete", 10, false).resolve(|_| None).unwrap()],
        );
        let stack = Stack::new("q5-direct".to_string())
            .add(account, alien_core::ResourceLifecycle::Frozen)
            .build();
        let result = IamInlinePolicySizeCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("dynamic"));
        assert!(result.warnings[0].contains("resource-scoped grants"));
    }

    #[tokio::test]
    async fn other_platforms_do_not_apply_iam_limits() {
        let stack = stack(&[("execution", vec![grant("literal", 400, false)])]);
        for platform in [
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            assert!(!IamInlinePolicySizeCheck.should_run(&stack, platform));
            assert_eq!(
                IamInlinePolicySizeCheck
                    .check(&stack, platform)
                    .await
                    .unwrap(),
                CheckResult::success()
            );
        }
    }
}
