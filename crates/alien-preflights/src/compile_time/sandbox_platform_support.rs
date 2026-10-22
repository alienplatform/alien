//! Runs each Sandbox's per-platform capability gate at plan time.
//!
//! `Sandbox::validate_for_platform` refuses a declaration the target backend cannot deliver —
//! ceilings a platform does not enforce, domain egress rules it cannot express, preview ports or
//! idle suspend it does not have. Without a caller it refuses nothing, and the failure arrives at
//! the first `create()` on a deployed stack instead.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Sandbox, SandboxEgress, Stack};

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
                // AWS and Kubernetes deployments install and configure the gateway. Other
                // deployment backends cannot yet create and publish one.
                if platform == Platform::Aws && sandbox.image_user == Some(0)
                    && !alien_core::sandbox_egress::sandbox_needs_connector(stack, sandbox)
                {
                    return Some(format!(
                        "Sandbox '{}': root image USER requires an AWS egress connector; this egress mode has no external boundary",
                        sandbox.id()
                    ));
                }
                if sandbox.egress == SandboxEgress::Internet
                    && !matches!(platform, Platform::Aws | Platform::Kubernetes)
                {
                    return Some(format!(
                        "Sandbox '{}': SANDBOX_EGRESS_GATEWAY_UNAVAILABLE: {platform} does not yet \
                         provide gateway hosting for egress internet",
                        sandbox.id()
                    ));
                }
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
        ResourceEntry, ResourceLifecycle, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
        SandboxLimits,
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
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
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
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

    /// A ceiling outside the platform's own sizing rule is refused at plan time: Azure sizes cpu
    /// in steps of 250m, so `333m` is a size it will not create. Caught while planning rather
    /// than at the first session, where it reads as a runtime fault, not a fixable declaration.
    #[tokio::test]
    async fn ceilings_outside_the_platform_rule_fail_at_plan_time() {
        let mut limits = ceilings();
        limits.cpu = "333m".to_string();
        let stack = stack_with(sandbox("agent", Some(limits), SandboxEgress::None));

        let result = SandboxPlatformSupportCheck
            .check(&stack, Platform::Azure)
            .await
            .expect("check runs");

        assert!(!result.success, "a size the platform refuses must not pass");
        let rendered = result.errors.join(" ");
        assert!(rendered.contains("agent"), "names the sandbox: {rendered}");
    }

    /// The same declaration is fine where the platform enforces it.
    #[tokio::test]
    async fn the_same_ceilings_pass_where_they_are_enforced() {
        let stack = stack_with(sandbox("agent", Some(ceilings()), SandboxEgress::None));

        for platform in [Platform::Aws, Platform::Azure, Platform::Kubernetes] {
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

    #[tokio::test]
    async fn root_boot_requires_an_aws_connector_including_legacy_direct_modes() {
        for policy in [
            SandboxEgress::Allow,
            SandboxEgress::None,
            SandboxEgress::AllowDomains {
                domains: vec!["example.com".into()],
            },
            SandboxEgress::Internet,
        ] {
            for legacy in [false, true] {
                let mut value = sandbox("root-boot", None, policy.clone());
                value.image_user = Some(0);
                if legacy {
                    value.privileged_supervisor =
                        Some(alien_core::SandboxPrivilegedSupervisor { command_uid: 60000 });
                }
                let stack = stack_with(value.clone());
                let result = SandboxPlatformSupportCheck
                    .check(&stack, Platform::Aws)
                    .await
                    .unwrap();
                let requires_refusal =
                    policy == SandboxEgress::Allow || policy == SandboxEgress::Internet || legacy;
                if requires_refusal {
                    assert!(!result.success);
                    assert!(result
                        .errors
                        .join(" ")
                        .contains("root image USER requires an AWS egress connector"));
                } else if policy == SandboxEgress::None {
                    assert!(result.success, "{:?}", result.errors);
                } else {
                    assert!(!result.success);
                    assert!(
                        result.errors.join(" ").contains("domainEgressRules"),
                        "{:?}",
                        result.errors
                    );
                }
            }
        }
        let gateway_id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID;
        let gateway = alien_core::Container::new(gateway_id.into())
            .code(alien_core::ContainerCode::Image {
                image: "example/gateway:1".into(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "0.25".into(),
                desired: "0.25".into(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".into(),
                desired: "512Mi".into(),
            })
            .replicas(1)
            .permissions(gateway_id.into())
            .port(3128)
            .build();
        let mut value = sandbox("with-gateway", None, SandboxEgress::Internet);
        value.image_user = Some(0);
        let mut stack = stack_with(value);
        stack.resources.insert(
            gateway_id.into(),
            ResourceEntry {
                config: alien_core::Resource::new(gateway),
                lifecycle: ResourceLifecycle::Live,
                dependencies: vec![],
                remote_access: false,
                enabled_when: None,
            },
        );
        assert!(
            SandboxPlatformSupportCheck
                .check(&stack, Platform::Aws)
                .await
                .unwrap()
                .success
        );
        // Released artifacts retain their old boot contract; non-root new images need no root boundary.
        for user in [None, Some(2001)] {
            let mut value = sandbox("old-or-nonroot", None, SandboxEgress::Allow);
            value.image_user = user;
            assert!(
                SandboxPlatformSupportCheck
                    .check(&stack_with(value), Platform::Aws)
                    .await
                    .unwrap()
                    .success
            );
        }
    }

    #[tokio::test]
    async fn local_and_kubernetes_refuse_root_boot_until_exec_is_separated() {
        for platform in [Platform::Local, Platform::Kubernetes] {
            for user in [None, Some(0), Some(2001)] {
                let mut value = sandbox("boot", None, SandboxEgress::Allow);
                value.image_user = user;
                let result = SandboxPlatformSupportCheck
                    .check(&stack_with(value), platform)
                    .await
                    .unwrap();
                assert_eq!(
                    result.success,
                    user != Some(0),
                    "{platform}: {:?}",
                    result.errors
                );
                if user == Some(0) {
                    assert!(result.errors.join(" ").contains("root image USER"));
                }
            }
        }
    }

    #[tokio::test]
    async fn a_stack_with_no_sandbox_is_not_checked() {
        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };
        assert!(!SandboxPlatformSupportCheck.should_run(&stack, Platform::Aws));
    }
}
