//! Runs each Sandbox's per-platform capability gate at plan time.
//!
//! `Sandbox::validate_for_platform` refuses a declaration the target backend cannot deliver —
//! ceilings a platform does not enforce, domain egress rules it cannot express, preview ports or
//! idle suspend it does not have. Without a caller it refuses nothing, and the failure arrives at
//! the first `create()` on a deployed stack instead.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{
    parse_ecr_image_repository, AzureSandboxImage, Platform, ResourceLifecycle, Sandbox,
    SandboxCode, SandboxEgress, Stack,
};

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
            .filter_map(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<Sandbox>()
                    .map(|sandbox| (sandbox, entry.lifecycle))
            })
            .filter_map(|(sandbox, lifecycle)| {
                // Setup builds a Frozen image once, from a bundle that must already exist. Only a
                // Live sandbox's controller renders one for an image in the vendor's registry, and
                // setup cannot grant that registry without knowing its account.
                if platform == Platform::Aws
                    && lifecycle == ResourceLifecycle::Frozen
                    && match &sandbox.code {
                        SandboxCode::Source { .. } => true,
                        SandboxCode::Image { image, .. } => parse_ecr_image_repository(image).is_ok(),
                    }
                {
                    return Some(format!(
                        "Sandbox '{}': a Frozen AWS sandbox is built by setup from a published \
                         bundle or a public image, and this one is built from source or a private \
                         registry; declare it Live",
                        sandbox.id()
                    ));
                }
                // Internet deployment prepares its gateway after these checks. Use the
                // planned boundary here; deployment prerequisites require the actual gateway.
                if platform == Platform::Aws && sandbox.image_user == Some(0)
                    && !alien_core::sandbox_egress::sandbox_plans_connector(sandbox)
                {
                    return Some(format!(
                        "Sandbox '{}': root image USER requires an AWS egress connector; this egress mode has no external boundary",
                        sandbox.id()
                    ));
                }
                // Azure hosts the gateway as two VMs that setup creates in the deployment's VNet,
                // and the sandbox reaches it through the agent's startup gate. A catalog image
                // runs no agent, so nothing inside it could prove or use that path.
                if platform == Platform::Azure
                    && sandbox.egress == SandboxEgress::Internet
                    && matches!(sandbox.azure_image(), Ok(AzureSandboxImage::Catalog(_)))
                {
                    return Some(format!(
                        "Sandbox '{}': SANDBOX_EGRESS_GATEWAY_UNAVAILABLE: egress internet on Azure \
                         needs an image with the Alien sandbox agent, and a catalog image has none; \
                         use an OCI registry image or a Dockerfile",
                        sandbox.id()
                    ));
                }
                // Setup would build the gateway VMs before the first sandbox create could find
                // out the gateway image cannot serve the sandbox binding, so every path that
                // builds, renders or deploys the stack refuses it first.
                if platform == Platform::Azure
                    && sandbox.egress == SandboxEgress::Internet
                    && !alien_core::sandbox_egress::gateway_image_serves_admin_protocol(
                        alien_core::sandbox_egress::planned_gateway_image(stack),
                    )
                {
                    return Some(format!(
                        "Sandbox '{}': {}",
                        sandbox.id(),
                        alien_core::sandbox_egress::AZURE_INTERNET_SANDBOX_UNAVAILABLE
                    ));
                }
                if sandbox.egress == SandboxEgress::Internet
                    && !matches!(
                        platform,
                        Platform::Aws | Platform::Kubernetes | Platform::Azure | Platform::Gcp
                    )
                {
                    return Some(format!(
                        "Sandbox '{}': SANDBOX_EGRESS_GATEWAY_UNAVAILABLE: {platform} does not yet \
                         provide gateway hosting for egress internet",
                        sandbox.id()
                    ));
                }
                // Preview signs an edge credential as an identity setup creates beside the engine.
                // A Live sandbox's engine is made by the runtime after setup, so nothing creates
                // that identity and a deployed `preview()` would fail at the first call.
                if platform == Platform::Gcp
                    && lifecycle == ResourceLifecycle::Live
                    && !sandbox.preview_ports.is_empty()
                {
                    return Some(format!(
                        "Sandbox '{}': preview on GCP needs a signing identity that setup creates \
                         next to the engine, and a Live sandbox's engine is created after setup; \
                         declare the sandbox Frozen or remove its preview ports",
                        sandbox.id()
                    ));
                }
                // The edge credential is the caller's execute right, so a preview nobody holds
                // execute for would have no one to sign it, and one that someone else could hold
                // would be command access handed out as if it were port access.
                if platform == Platform::Gcp
                    && !sandbox.preview_ports.is_empty()
                    && !stack_grants_execute(stack, sandbox.id())
                {
                    return Some(format!(
                        "Sandbox '{}': {} Grant `sandbox/execute` on the sandbox to the \
                         permission profile that previews it, or remove its preview ports; for a \
                         preview that can be shared per port, use another cloud",
                        sandbox.id(),
                        alien_core::GCP_PREVIEW_IS_COMMAND_ACCESS
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

        let warnings = stack
            .resources()
            .filter_map(|(_, entry)| entry.config.downcast_ref::<Sandbox>())
            .filter(|sandbox| {
                platform == Platform::Azure
                    && matches!(sandbox.azure_image(), Ok(AzureSandboxImage::Catalog(_)))
            })
            .map(|sandbox| {
                format!(
                    "Sandbox '{}': Azure catalog USER and command cannot be inspected. \
                     Native exec may run as root; use an OCI registry image or Dockerfile \
                     for non-root agent exec and the image USER/boot-process contract.",
                    sandbox.id()
                )
            })
            .collect::<Vec<_>>();
        if !errors.is_empty() {
            Ok(CheckResult::failed_with_warnings(errors, warnings))
        } else if !warnings.is_empty() {
            Ok(CheckResult::with_warnings(warnings))
        } else {
            Ok(CheckResult::success())
        }
    }
}

/// Whether some permission profile of the stack holds `sandbox/execute` on the sandbox, by name or
/// through the wildcard. The one right that opens a GCP sandbox's edge and its `:execute`.
fn stack_grants_execute(stack: &Stack, sandbox_id: &str) -> bool {
    stack.permission_profiles().values().any(|profile| {
        [sandbox_id, "*"].iter().any(|key| {
            profile.0.get(*key).is_some_and(|refs| {
                refs.iter()
                    .any(|reference| reference.id() == "sandbox/execute")
            })
        })
    })
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
        stack_with_lifecycle(sandbox, ResourceLifecycle::Frozen)
    }

    fn stack_with_lifecycle(sandbox: Sandbox, lifecycle: ResourceLifecycle) -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            sandbox.id().to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(sandbox),
                lifecycle,
                dependencies: Vec::new(),
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
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
                bundled_from: None,
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

    /// GCP preview is signed as an identity setup creates beside the engine. A Live sandbox has
    /// none, so the plan refuses it instead of leaving the first `preview()` to fail.
    #[tokio::test]
    async fn a_live_gcp_sandbox_with_preview_ports_is_refused_and_a_frozen_one_passes() {
        let with_ports = || {
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::None)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .preview_ports(vec![8080])
                .build()
        };
        let live = SandboxPlatformSupportCheck
            .check(
                &stack_with_lifecycle(with_ports(), ResourceLifecycle::Live),
                Platform::Gcp,
            )
            .await
            .expect("check runs");
        assert!(!live.success, "a Live GCP preview has no signer");
        let rendered = live.errors.join(" ");
        assert!(
            rendered.contains("agent") && rendered.contains("Frozen"),
            "{rendered}"
        );

        let mut owned = stack_with(with_ports());
        owned.permissions.profiles.insert(
            "worker".to_string(),
            alien_core::permissions::PermissionProfile::new()
                .resource("agent", ["sandbox/execute"]),
        );
        let frozen = SandboxPlatformSupportCheck
            .check(&owned, Platform::Gcp)
            .await
            .expect("check runs");
        assert!(frozen.success, "{:?}", frozen.errors);
    }

    /// A GCP preview is the caller's execute right, so preview ports that no profile holds execute
    /// for are refused at plan time with the plain rule, whatever else the stack grants.
    #[tokio::test]
    async fn a_gcp_preview_nobody_may_execute_is_refused_with_the_command_access_rule() {
        let sandbox = Sandbox::new("agent".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu".to_string(),
                bundled_from: None,
            })
            .egress(SandboxEgress::None)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .preview_ports(vec![8080])
            .build();
        let mut stack = stack_with(sandbox);
        // Reaching the sandbox without running anything in it: not a preview right on GCP.
        stack.permissions.profiles.insert(
            "viewer".to_string(),
            alien_core::permissions::PermissionProfile::new()
                .resource("agent", ["sandbox/heartbeat"]),
        );
        let refused = SandboxPlatformSupportCheck
            .check(&stack, Platform::Gcp)
            .await
            .expect("check runs");
        assert!(!refused.success, "preview without execute must be refused");
        let rendered = refused.errors.join(" ");
        assert!(
            rendered.contains(
                "On Google Cloud, preview access equals command access; port-scoped preview \
                 sharing is not available."
            ),
            "{rendered}"
        );
        assert!(rendered.contains("agent"), "names the sandbox: {rendered}");

        // The same stack, once a profile holds execute, is accepted; and so is the wildcard.
        for key in ["agent", "*"] {
            let mut owned = stack.clone();
            owned.permissions.profiles.insert(
                "worker".to_string(),
                alien_core::permissions::PermissionProfile::new()
                    .resource(key, ["sandbox/execute"]),
            );
            let accepted = SandboxPlatformSupportCheck
                .check(&owned, Platform::Gcp)
                .await
                .expect("check runs");
            assert!(accepted.success, "{key}: {:?}", accepted.errors);
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
                    policy == SandboxEgress::Allow || (legacy && policy != SandboxEgress::Internet);
                if requires_refusal {
                    assert!(!result.success);
                    assert!(result
                        .errors
                        .join(" ")
                        .contains("root image USER requires an AWS egress connector"));
                } else if matches!(policy, SandboxEgress::None | SandboxEgress::Internet) {
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
                display_name: None,
                description: None,
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
    async fn local_refuses_root_boot_until_exec_is_separated() {
        for user in [None, Some(0), Some(2001)] {
            let mut value = sandbox("boot", None, SandboxEgress::Allow);
            value.image_user = user;
            let result = SandboxPlatformSupportCheck
                .check(&stack_with(value), Platform::Local)
                .await
                .unwrap();
            assert_eq!(result.success, user != Some(0), "{:?}", result.errors);
            if user == Some(0) {
                assert!(result.errors.join(" ").contains("root image USER"));
            }
        }
    }

    /// Kubernetes boots a root image only with an egress boundary outside the pod; the refusal
    /// names the declaration, and non-root images are unaffected by egress mode.
    #[tokio::test]
    async fn kubernetes_root_boot_needs_internet_or_none_egress() {
        for (egress, root_ok) in [
            (SandboxEgress::Internet, true),
            (SandboxEgress::None, true),
            (SandboxEgress::Allow, false),
        ] {
            for user in [None, Some(0), Some(2001)] {
                let mut value = sandbox("boot", None, egress.clone());
                value.image_user = user;
                let result = SandboxPlatformSupportCheck
                    .check(&stack_with(value), Platform::Kubernetes)
                    .await
                    .unwrap();
                assert_eq!(
                    result.success,
                    user != Some(0) || root_ok,
                    "{egress:?} {user:?}: {:?}",
                    result.errors
                );
            }
        }
    }

    /// The gateway image this release installs cannot serve the Azure sandbox binding, so an
    /// Azure internet stack is refused at build and render, before setup creates the gateway VMs.
    /// Other clouds and other egress modes are unaffected, and a newer gateway digest lifts it.
    #[tokio::test]
    async fn azure_internet_is_refused_until_the_gateway_serves_the_admin_protocol() {
        let internet = |id: &str, egress: SandboxEgress| {
            let mut value = sandbox(id, None, egress);
            value.code = SandboxCode::Image {
                image: "example.azurecr.io/agent:1".to_string(),
                bundled_from: None,
            };
            stack_with(value)
        };
        let refused = SandboxPlatformSupportCheck
            .check(&internet("agent", SandboxEgress::Internet), Platform::Azure)
            .await
            .unwrap();
        assert!(!refused.success);
        assert_eq!(
            refused.errors,
            vec![format!(
                "Sandbox 'agent': {}",
                alien_core::sandbox_egress::AZURE_INTERNET_SANDBOX_UNAVAILABLE
            )]
        );

        for platform in [Platform::Aws, Platform::Gcp, Platform::Kubernetes] {
            let result = SandboxPlatformSupportCheck
                .check(&internet("agent", SandboxEgress::Internet), platform)
                .await
                .unwrap();
            assert!(result.success, "{platform}: {:?}", result.errors);
        }
        for egress in [SandboxEgress::None, SandboxEgress::Allow] {
            let result = SandboxPlatformSupportCheck
                .check(&internet("agent", egress), Platform::Azure)
                .await
                .unwrap();
            assert!(result.success, "{:?}", result.errors);
        }

        // A stack whose gateway already names a newer digest is accepted.
        let gateway_id = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID;
        let mut newer = internet("agent", SandboxEgress::Internet);
        newer.resources.insert(
            gateway_id.into(),
            ResourceEntry {
                config: alien_core::Resource::new(
                    alien_core::Container::new(gateway_id.into())
                        .code(alien_core::ContainerCode::Image {
                            image: "ghcr.io/alienplatform/alien-egress-gateway@sha256:0000000000000000000000000000000000000000000000000000000000000001".into(),
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
                        .build(),
                ),
                lifecycle: ResourceLifecycle::Live,
                dependencies: vec![],
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        let accepted = SandboxPlatformSupportCheck
            .check(&newer, Platform::Azure)
            .await
            .unwrap();
        assert!(accepted.success, "{:?}", accepted.errors);
    }

    /// `alien build`, `alien release` and the Terraform/CloudFormation render all run these
    /// checks, so none of them reaches the point of creating gateway VMs.
    #[tokio::test]
    async fn build_and_template_preflights_refuse_azure_internet() {
        let mut value = sandbox("agent", None, SandboxEgress::Internet);
        value.code = SandboxCode::Image {
            image: "example.azurecr.io/agent:1".to_string(),
            bundled_from: None,
        };
        let stack = stack_with(value);
        let runner = crate::runner::PreflightRunner::new();
        let build = runner
            .run_build_time_preflights(&stack, Platform::Azure)
            .await
            .expect_err("build refuses");
        let template = runner
            .run_template_preflights(&stack, Platform::Azure)
            .await
            .expect_err("render refuses");
        for error in [format!("{build:?}"), format!("{template:?}")] {
            assert!(
                error.contains(alien_core::sandbox_egress::AZURE_INTERNET_SANDBOX_UNAVAILABLE),
                "{error}"
            );
        }
        assert!(runner
            .run_build_time_preflights(&stack, Platform::Aws)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn a_stack_with_no_sandbox_is_not_checked() {
        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };
        assert!(!SandboxPlatformSupportCheck.should_run(&stack, Platform::Aws));
    }

    /// Setup builds a Frozen image before any controller can render a bundle, so a Frozen AWS
    /// sandbox must name a published bundle or a public image; the same image is fine when Live.
    #[tokio::test]
    async fn a_frozen_aws_sandbox_from_a_private_registry_must_be_live() {
        let with_image = |image: &str| {
            let mut sandbox = sandbox("agents", None, SandboxEgress::Allow);
            sandbox.code = SandboxCode::Image {
                image: image.to_string(),
                bundled_from: None,
            };
            stack_with(sandbox)
        };
        let private = "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:aaaa";

        let frozen = SandboxPlatformSupportCheck
            .check(&with_image(private), Platform::Aws)
            .await
            .unwrap();
        assert!(!frozen.success);
        assert!(
            frozen.errors[0].contains("declare it Live"),
            "{:?}",
            frozen.errors
        );

        let mut live = with_image(private);
        live.resources["agents"].lifecycle = ResourceLifecycle::Live;
        for (stack, platform) in [
            (live, Platform::Aws),
            (
                with_image("s3://publisher/sandbox-bundle/v1/bundle.zip"),
                Platform::Aws,
            ),
            (with_image(private), Platform::Gcp),
        ] {
            let result = SandboxPlatformSupportCheck
                .check(&stack, platform)
                .await
                .unwrap();
            assert!(result.success, "{platform}: {:?}", result.errors);
        }
    }
}
