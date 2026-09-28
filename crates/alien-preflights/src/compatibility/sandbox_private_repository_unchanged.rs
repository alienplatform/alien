use crate::error::Result;
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::{ResourceLifecycle, Sandbox, Stack};

/// Validates that a non-Frozen sandbox keeps its private base-image repository across an update.
///
/// Setup renders the sandbox build role, which grants pull on exactly one repository, so a new
/// repository (or a private base appearing or going away) needs setup even though the runtime
/// rolls the image itself. A new tag or digest in the same repository passes. Frozen sandboxes
/// are left to the Frozen check, which already refuses any change to them.
pub struct SandboxPrivateRepositoryUnchangedCheck;

#[async_trait::async_trait]
impl StackCompatibilityCheck for SandboxPrivateRepositoryUnchangedCheck {
    fn description(&self) -> &'static str {
        "A sandbox's private base-image repository shouldn't change during updates"
    }

    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (id, new_entry) in new_stack.resources() {
            if new_entry.lifecycle == ResourceLifecycle::Frozen {
                continue;
            }
            let Some(new_sandbox) = new_entry.config.downcast_ref::<Sandbox>() else {
                continue;
            };
            let Some(old_sandbox) = old_stack
                .resources
                .get(id)
                .and_then(|entry| entry.config.downcast_ref::<Sandbox>())
            else {
                continue;
            };

            let old_repository = old_sandbox.private_base_image_repository();
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
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
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
