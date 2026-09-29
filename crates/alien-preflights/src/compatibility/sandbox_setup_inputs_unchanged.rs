use crate::error::{ErrorData, Result};
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::{
    declined_live_resources, sandbox_setup_inputs::comparable_aws_sandbox_setup_inputs,
    surviving_frozen_gate_answers, DeploymentConfig, ResourceLifecycle, Sandbox, Stack,
};
use alien_error::AlienError;

/// A Live sandbox's setup inputs (egress, its network, the build role policy, the remote grant)
/// are applied by setup alone, so changing one needs setup; a new bundle or tag does not. A sandbox
/// the installed stack lacks has none, and a declined one needs none.
pub struct SandboxSetupInputsUnchangedCheck;

impl SandboxSetupInputsUnchangedCheck {
    fn compare(old_stack: &Stack, new_stack: &Stack, declined: &[String]) -> CheckResult {
        let mut errors = Vec::new();

        for (id, new_entry) in new_stack.resources() {
            if new_entry.lifecycle == ResourceLifecycle::Frozen || declined.contains(id) {
                continue;
            }
            let Some(new_sandbox) = new_entry.config.downcast_ref::<Sandbox>() else {
                continue;
            };
            let Some(old_inputs) = old_stack.resources.get(id).and_then(|old_entry| {
                let old_sandbox = old_entry.config.downcast_ref::<Sandbox>()?;
                Some(comparable_aws_sandbox_setup_inputs(
                    old_stack,
                    old_sandbox,
                    old_entry.lifecycle,
                ))
            }) else {
                errors.push(format!(
                    "Sandbox '{id}' is new, and setup creates its build role. Rerun setup with \
                     the updated stack."
                ));
                continue;
            };
            let new_inputs =
                comparable_aws_sandbox_setup_inputs(new_stack, new_sandbox, new_entry.lifecycle);
            let changed: Vec<&str> = if old_inputs.len() == new_inputs.len() {
                old_inputs
                    .iter()
                    .zip(&new_inputs)
                    .filter(|((_, was), (_, is))| was != is)
                    .map(|((name, _), _)| *name)
                    .collect()
            } else {
                vec!["configuration"]
            };
            for name in changed {
                errors.push(format!(
                    "Sandbox '{id}' changes its {name}, which setup applies. Rerun setup with \
                     the updated stack."
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
impl StackCompatibilityCheck for SandboxSetupInputsUnchangedCheck {
    fn description(&self) -> &'static str {
        "A Live sandbox's setup inputs shouldn't change during updates"
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
    use alien_core::{
        Network, NetworkSettings, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    };

    const REPO_A: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:1";
    const BUNDLE_V1: &str = "s3://bucket/sandbox-bundle/v1/bundle.zip";

    fn stack(private_base_image: Option<&str>, image: &str) -> Stack {
        stack_with(private_base_image, image, SandboxEgress::Allow, "net-a")
    }

    fn stack_with(
        private_base_image: Option<&str>,
        image: &str,
        egress: SandboxEgress,
        network_id: &str,
    ) -> Stack {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: image.to_string(),
            })
            .maybe_private_base_image(private_base_image.map(str::to_string))
            .egress(egress)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        Stack::new("stack".to_string())
            .add(
                Network::new(network_id.to_string())
                    .settings(NetworkSettings::Create {
                        cidr: Some("10.0.0.0/16".to_string()),
                        availability_zones: 2,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(sandbox, ResourceLifecycle::Live)
            .build()
    }

    async fn run(old: Stack, new: Stack) -> CheckResult {
        SandboxSetupInputsUnchangedCheck
            .check(&old, &new)
            .await
            .expect("check runs")
    }

    #[tokio::test]
    async fn a_new_tag_or_bundle_passes() {
        let result = run(
            stack(Some(REPO_A), BUNDLE_V1),
            stack(
                Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:2"),
                "s3://bucket/sandbox-bundle/v2/bundle.zip",
            ),
        )
        .await;
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn a_live_sandbox_new_to_the_installed_stack_needs_setup() {
        for private_base_image in [Some(REPO_A), None] {
            let result = run(
                Stack::new("stack".to_string()).build(),
                stack(private_base_image, BUNDLE_V1),
            )
            .await;
            assert!(
                !result.success,
                "setup creates the build role of {private_base_image:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_setup_input_change_fails_and_names_the_input() {
        let deny = |network_id| stack_with(None, BUNDLE_V1, SandboxEgress::Deny, network_id);
        let mut remote = stack(None, BUNDLE_V1);
        remote
            .resources
            .get_mut("agents")
            .expect("sandbox")
            .remote_access = true;
        for (old, new, input) in [
            (
                stack(Some(REPO_A), BUNDLE_V1),
                stack(
                    Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-b:1"),
                    BUNDLE_V1,
                ),
                "build role policy",
            ),
            (
                stack(None, BUNDLE_V1),
                stack(Some(REPO_A), BUNDLE_V1),
                "build role policy",
            ),
            (
                stack(Some(REPO_A), BUNDLE_V1),
                stack(None, BUNDLE_V1),
                "build role policy",
            ),
            (
                stack(None, BUNDLE_V1),
                stack(None, "s3://other-bucket/sandbox-bundle/v1/bundle.zip"),
                "build role policy",
            ),
            (
                stack(None, BUNDLE_V1),
                stack(None, "s3://bucket/other-prefix/v1/bundle.zip"),
                "build role policy",
            ),
            (stack(None, BUNDLE_V1), deny("net-a"), "egress"),
            (deny("net-a"), deny("net-b"), "egress network"),
            (stack(None, BUNDLE_V1), remote, "remote grant"),
        ] {
            let result = run(old, new).await;
            assert!(!result.success, "a {input} change must need setup");
            assert!(
                result
                    .errors
                    .iter()
                    .any(|error| error.contains("agents") && error.contains(input)),
                "{input}: {:?}",
                result.errors
            );
        }
    }
}
