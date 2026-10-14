//! Definite IAM inline-policy overflow from global workload grants, before setup.
//!
//! Only documents rendered without deployment variables contribute to this lower
//! bound. Resource-scoped grants, injected grants and existing role policies can
//! consume additional headroom; final render and controller guards still apply.

use crate::{
    error::{ErrorData, Result},
    CheckResult, CompileTimeCheck,
};
use alien_core::{Platform, Stack};
use alien_error::{Context, IntoAlienError};
use alien_permissions::{
    generators::aws_runtime::{AwsIamPolicy, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};

/// Rejects definite overflow in a workload role's combined global inline policy.
pub struct IamInlinePolicySizeCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for IamInlinePolicySizeCheck {
    fn code(&self) -> Option<&'static str> {
        Some("IAM_INLINE_POLICY_SIZE")
    }

    fn description(&self) -> &'static str {
        "IAM workload role global inline policies fit the 10240 character limit"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Aws && !stack.permission_profiles().is_empty()
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
            let Some(references) = profile.0.get("*") else {
                continue;
            };
            let mut statements = Vec::new();
            let mut unresolved = Vec::new();
            for reference in references {
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
            if !statements.is_empty() {
                // This is the same combined document layout as direct setup.
                // CFN combines these statements too; Terraform writes separate
                // documents, which adds at least as much document overhead.
                let policy = AwsIamPolicy {
                    version: "2012-10-17".to_string(),
                    statement: statements,
                };
                let document = serde_json::to_string(&policy).into_alien_error().context(
                    ErrorData::CompileTimeCheckFailed {
                        check_name: self.description().to_string(),
                        message: "Could not serialize the combined IAM policy".to_string(),
                        resource_id: Some(format!("{profile_name}-sa")),
                    },
                )?;
                let size = document
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .count();
                if size > 10_240 {
                    result.add_error(format!(
                        "IAM role for permission profile '{profile_name}' needs at least {size} inline policy characters; the aggregate per-role limit is 10240. Reduce global grants or use managed policies. https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_iam-quotas.html"
                    ));
                }
            }
            if !unresolved.is_empty() {
                result.add_warning(format!(
                    "IAM inline policy size for permission profile '{profile_name}' is incomplete: grants [{}] need deployment values or could not be rendered. Only rendered global grants were counted; passing does not establish total role headroom.",
                    unresolved.join(", ")
                ));
            }
        }
        Ok(result)
    }
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
