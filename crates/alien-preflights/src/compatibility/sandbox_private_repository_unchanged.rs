use crate::error::{ErrorData, Result};
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::{
    declined_live_resources, surviving_frozen_gate_answers, DeploymentConfig, ResourceLifecycle,
    Sandbox, Stack,
};
use alien_error::AlienError;

/// Setup grants pull on exactly one private base-image repository, so changing it on a non-Frozen
/// sandbox needs setup; a new tag or digest does not. A sandbox the installed stack lacks counts as
/// having none, and a declined one needs none. The Frozen check covers Frozen sandboxes.
pub struct SandboxPrivateRepositoryUnchangedCheck;

impl SandboxPrivateRepositoryUnchangedCheck {
    fn compare(old_stack: &Stack, new_stack: &Stack, declined: &[String]) -> CheckResult {
        let mut errors = Vec::new();

        for (id, new_entry) in new_stack.resources() {
            if new_entry.lifecycle == ResourceLifecycle::Frozen || declined.contains(id) {
                continue;
            }
            let Some(new_sandbox) = new_entry.config.downcast_ref::<Sandbox>() else {
                continue;
            };
            let old_repository = old_stack
                .resources
                .get(id)
                .and_then(|entry| entry.config.downcast_ref::<Sandbox>())
                .and_then(Sandbox::private_base_image_repository);
            let new_repository = new_sandbox.private_base_image_repository();
            if old_repository != new_repository {
                errors.push(format!(
                    "Sandbox '{}' changed its private base-image repository from {} to {}. \
                     Setup grants pull on that repository. Rerun setup with the updated stack.",
                    id,
                    old_repository.as_deref().unwrap_or("none"),
                    new_repository.as_deref().unwrap_or("none"),
                ));
            }
        }

        if errors.is_empty() {
            CheckResult::success()
        } else {
            CheckResult::failed(errors)
        }
    }
}

#[async_trait::async_trait]
impl StackCompatibilityCheck for SandboxPrivateRepositoryUnchangedCheck {
    fn description(&self) -> &'static str {
        "A sandbox's private base-image repository shouldn't change during updates"
    }

    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        Ok(Self::compare(old_stack, new_stack, &[]))
    }

    async fn check_with_config(
        &self,
        old_stack: &Stack,
        new_stack: &Stack,
        config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        let (answers, still_frozen_gating) = surviving_frozen_gate_answers(new_stack);
        let declined = declined_live_resources(
            new_stack,
            &config.input_values,
            &answers,
            &still_frozen_gating,
        )
        .map_err(|message| {
            AlienError::new(ErrorData::StackCompatibilityCheckFailed {
                check_name: self.description().to_string(),
                message,
                old_resource_id: None,
                new_resource_id: None,
            })
        })?;
        Ok(Self::compare(old_stack, new_stack, &declined))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{SandboxCode, SandboxEgress, SandboxLifecyclePolicy};

    const REPO_A: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:1";

    fn stack(private_base_image: Option<&str>, image: &str) -> Stack {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: image.to_string(),
            })
            .maybe_private_base_image(private_base_image.map(str::to_string))
            .egress(SandboxEgress::Deny)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        Stack::new("stack".to_string())
            .add(sandbox, ResourceLifecycle::Live)
            .build()
    }

    async fn run(old: Stack, new: Stack) -> CheckResult {
        SandboxPrivateRepositoryUnchangedCheck
            .check(&old, &new)
            .await
            .expect("check runs")
    }

    #[tokio::test]
    async fn a_new_tag_or_bundle_passes() {
        let result = run(
            stack(Some(REPO_A), "s3://bucket/one.zip"),
            stack(
                Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:2"),
                "s3://bucket/two.zip",
            ),
        )
        .await;
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn a_sandbox_new_to_the_installed_stack_needs_setup_only_for_a_private_base() {
        let installed = Stack::new("stack".to_string()).build();

        let private = run(
            installed.clone(),
            stack(Some(REPO_A), "s3://bucket/one.zip"),
        )
        .await;
        assert!(
            !private.success,
            "no grant was rendered for this repository"
        );

        let public = run(installed, stack(None, "s3://bucket/one.zip")).await;
        assert!(public.success, "{:?}", public.errors);
    }

    #[tokio::test]
    async fn a_repository_change_fails() {
        for (old, new) in [
            (
                Some(REPO_A),
                Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-b:1"),
            ),
            (None, Some(REPO_A)),
            (Some(REPO_A), None),
        ] {
            let result = run(
                stack(old, "s3://bucket/one.zip"),
                stack(new, "s3://bucket/one.zip"),
            )
            .await;
            assert!(!result.success, "{old:?} -> {new:?} must need setup");
            assert!(result.errors[0].contains("agents"));
        }
    }
}
