//! Runs each Sandbox's per-platform capability gate at plan time.
//!
//! `Sandbox::validate_for_platform` refuses a declaration the target backend cannot deliver —
//! ceilings a platform does not enforce, domain egress rules it cannot express, preview ports or
//! idle suspend it does not have. Without a caller it refuses nothing, and the failure arrives at
//! the first `create()` on a deployed stack instead.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Sandbox, Stack};

/// Ensures every Sandbox can be delivered by the platform it targets.
pub struct SandboxPlatformSupportCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SandboxPlatformSupportCheck {
    fn description(&self) -> &'static str {
        "A Sandbox must declare only what its target platform can enforce"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources().any(|(_, entry)| {
            entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let errors: Vec<String> = stack
            .resources()
            .filter_map(|(_, entry)| entry.config.downcast_ref::<Sandbox>())
            .filter_map(|sandbox| {
                sandbox
                    .validate_for_platform(platform)
                    .err()
                    // A capability refusal names the platform and the capability, which is the
                    // right shape for the runtime caller but leaves a deployer with several
                    // sandboxes unable to tell which declaration to change.
                    .map(|error| format!("Sandbox '{}': {error}", sandbox.id()))
            })
            .collect();

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
    use alien_core::{
        ResourceEntry, ResourceLifecycle, SandboxCode, SandboxEgress, SandboxLimits,
        SandboxSessionPolicy,
    };
    use indexmap::IndexMap;

    fn stack_with(sandbox: Sandbox) -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            sandbox.id().to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(sandbox),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        }
    }

    fn sandbox(id: &str, limits: Option<SandboxLimits>, egress: SandboxEgress) -> Sandbox {
        let builder = Sandbox::new(id.to_string())
            .code(SandboxCode::Image {
                // A bare name, because Azure takes a catalog entry rather than a reference and
                // these cases are about egress and limits rather than about the image.
                image: "ubuntu".to_string(),
            })
            .egress(egress)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            });
        match limits {
            Some(limits) => builder.limits(limits).build(),
            None => builder.build(),
        }
    }

    fn ceilings() -> SandboxLimits {
        SandboxLimits {
            cpu: "1".to_string(),
            memory: "2Gi".to_string(),
            disk: "20Gi".to_string(),
            max_processes: None,
        }
    }

    /// GCP runs sandboxes as subprocesses of the app's own Cloud Run instance, which applies no
    /// per-sandbox ceiling. A stack that declares one reads as bounded while the sandbox is not.
    #[tokio::test]
    async fn ceilings_on_a_platform_that_ignores_them_fail_at_plan_time() {
        let stack = stack_with(sandbox("agent", Some(ceilings()), SandboxEgress::Deny));

        let result = SandboxPlatformSupportCheck
            .check(&stack, Platform::Gcp)
            .await
            .expect("check runs");

        assert!(!result.success, "unenforceable ceilings must not pass");
        let rendered = result.errors.join(" ");
        assert!(rendered.contains("agent"), "names the sandbox: {rendered}");
    }

    /// The same declaration is fine where the platform enforces it.
    #[tokio::test]
    async fn the_same_ceilings_pass_where_they_are_enforced() {
        let stack = stack_with(sandbox("agent", Some(ceilings()), SandboxEgress::Deny));

        for platform in [Platform::Aws, Platform::Kubernetes] {
            let result = SandboxPlatformSupportCheck
                .check(&stack, platform)
                .await
                .expect("check runs");
            assert!(
                result.success,
                "{platform} enforces ceilings: {:?}",
                result.errors
            );
        }
    }

    /// Azure's egress proxy matches on host pattern; the other four filter by address or carry a
    /// single switch, so the declaration is refused there rather than accepted and dropped.
    #[tokio::test]
    async fn domain_egress_rules_are_refused_where_they_cannot_be_expressed() {
        let stack = stack_with(sandbox(
            "agent",
            None,
            SandboxEgress::AllowDomains {
                domains: vec!["registry.npmjs.org".to_string()],
            },
        ));

        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            let result = SandboxPlatformSupportCheck
                .check(&stack, platform)
                .await
                .expect("check runs");
            assert!(
                !result.success,
                "{platform} has no hostname allowlist and must refuse the declaration"
            );
        }

        let azure = SandboxPlatformSupportCheck
            .check(&stack, Platform::Azure)
            .await
            .expect("check runs");
        assert!(
            azure.success,
            "Azure creates the sandbox under host rules: {:?}",
            azure.errors
        );
    }

    #[tokio::test]
    async fn a_stack_with_no_sandbox_is_not_checked() {
        let stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };
        assert!(!SandboxPlatformSupportCheck.should_run(&stack, Platform::Aws));
    }
}
