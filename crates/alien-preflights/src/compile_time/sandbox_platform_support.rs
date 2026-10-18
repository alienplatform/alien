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
        let mut errors: Vec<String> = stack
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

        if platform == Platform::Gcp {
            for (_, entry) in stack.resources() {
                let Some(sandbox) = entry.config.downcast_ref::<Sandbox>() else {
                    continue;
                };
                if sandbox.egress == SandboxEgress::Internet {
                    errors.push(format!(
                        "Sandbox '{}': GCP private gateway command transport is not available; \
                         egress \"internet\" requires a gateway and cannot use the native fallback",
                        sandbox.id()
                    ));
                }
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
    use alien_core::{
        Container, ResourceEntry, ResourceLifecycle, SandboxCode, SandboxEgress,
        SandboxLifecyclePolicy, SandboxLimits,
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
    async fn gcp_internet_never_falls_back_when_gateway_transport_is_unavailable() {
        let open = sandbox("agent", None, SandboxEgress::Internet);
        let result = SandboxPlatformSupportCheck
            .check(&stack_with(open.clone()), Platform::Gcp)
            .await
            .expect("check runs");
        assert!(!result.success);
        assert!(result.warnings.is_empty());
        assert!(result.errors[0].contains("requires a gateway"));
        let mut restricted = open;
        restricted.default_policy = Some(alien_core::EgressPolicy::Allow {
            allow: vec!["example.com".into()],
        });
        let result = SandboxPlatformSupportCheck
            .check(&stack_with(restricted), Platform::Gcp)
            .await
            .expect("check runs");
        assert!(!result.success);
        assert!(result.errors[0].contains("requires a gateway"));
        let result = SandboxPlatformSupportCheck
            .check(
                &stack_with(sandbox("closed", None, SandboxEgress::None)),
                Platform::Gcp,
            )
            .await
            .expect("check runs");
        assert!(result.success);
        assert!(result.warnings.is_empty());
    }

    #[tokio::test]
    async fn gcp_gateway_deployment_is_refused_before_cloud_changes() {
        let stack = Stack::new("private-egress".into())
            .add(
                sandbox("agent", None, SandboxEgress::Internet),
                ResourceLifecycle::Frozen,
            )
            .add(
                Container::new("sandbox-egress-gateway".into())
                    .code(alien_core::ContainerCode::Image {
                        image: "gateway:test".into(),
                    })
                    .cpu(alien_core::ResourceSpec {
                        min: "0.5".into(),
                        desired: "0.5".into(),
                    })
                    .memory(alien_core::ResourceSpec {
                        min: "512Mi".into(),
                        desired: "512Mi".into(),
                    })
                    .port(3128)
                    .permissions("gateway".into())
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let result = SandboxPlatformSupportCheck
            .check(&stack, Platform::Gcp)
            .await
            .expect("check runs");
        assert!(!result.success);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("private gateway command transport")));
        assert!(result.warnings.is_empty());
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
