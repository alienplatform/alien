//! Refuses the GCP management profiles whose `sandbox/templates` grant would bind nothing.
//!
//! The grant binds only on a Frozen sandbox's engine, so at `*`, under any other resource id, or
//! left off a Frozen sandbox by an override, it renders nothing. Each surfaces only as a later 403.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{ManagementPermissions, Platform, ResourceLifecycle, Sandbox, Stack};

const TEMPLATES: &str = "sandbox/templates";

/// Ensures `sandbox/templates` is granted per sandbox on GCP, and present under an override.
pub struct SandboxTemplatePermissionsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SandboxTemplatePermissionsCheck {
    fn description(&self) -> &'static str {
        "GCP sandbox template permissions must be scoped to the sandbox that needs them"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Gcp
            && stack.resources().any(|(_, entry)| {
                entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
            })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let profile = match stack.management() {
            ManagementPermissions::Auto => return Ok(CheckResult::success()),
            ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
                profile
            }
        };
        let grants = |scope: &str| {
            profile
                .0
                .get(scope)
                .is_some_and(|refs| refs.iter().any(|reference| reference.id() == TEMPLATES))
        };

        let is_frozen_sandbox = |resource_id: &str| {
            stack.resources().any(|(id, entry)| {
                id == resource_id
                    && entry.config.downcast_ref::<Sandbox>().is_some()
                    && entry.lifecycle == ResourceLifecycle::Frozen
            })
        };

        let mut errors = Vec::new();
        if grants("*") {
            errors.push(format!(
                "'{TEMPLATES}' cannot be granted at '*': it binds on one sandbox's engine. Grant it \
                 under each Frozen sandbox's id instead."
            ));
        }
        for scope in profile.0.keys() {
            if scope != "*" && grants(scope) && !is_frozen_sandbox(scope) {
                errors.push(format!(
                    "'{TEMPLATES}' is granted under '{scope}', which is not a Frozen sandbox. Only a \
                     Frozen sandbox's engine takes this grant; remove it from '{scope}'."
                ));
            }
        }
        if matches!(stack.management(), ManagementPermissions::Override(_)) {
            for (resource_id, entry) in stack.resources() {
                let frozen_sandbox = entry.config.downcast_ref::<Sandbox>().is_some()
                    && entry.lifecycle == ResourceLifecycle::Frozen;
                if frozen_sandbox && !grants(resource_id) {
                    errors.push(format!(
                        "Setup required: Frozen sandbox '{resource_id}' needs management \
                         permission '{TEMPLATES}' to publish its image. The stack overrides \
                         management permissions, so add '{TEMPLATES}' under '{resource_id}' and \
                         rerun setup."
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
    use alien_core::{PermissionProfile, SandboxCode, SandboxEgress, SandboxLifecyclePolicy};

    fn stack(management: ManagementPermissions) -> Stack {
        let sandbox = |id: &str| {
            Sandbox::new(id.to_string())
                .code(SandboxCode::Image {
                    image: "us-central1-docker.pkg.dev/p/r/agent:1".to_string(),
                })
                .egress(SandboxEgress::Deny)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build()
        };
        Stack::new("test".to_string())
            .add(sandbox("frozen-box"), ResourceLifecycle::Frozen)
            .add(sandbox("live-box"), ResourceLifecycle::Live)
            .management(management)
            .build()
    }

    async fn errors(management: ManagementPermissions) -> Vec<String> {
        let stack = stack(management);
        assert!(SandboxTemplatePermissionsCheck.should_run(&stack, Platform::Gcp));
        SandboxTemplatePermissionsCheck
            .check(&stack, Platform::Gcp)
            .await
            .expect("the check runs")
            .errors
    }

    #[tokio::test]
    async fn an_override_must_grant_templates_on_each_frozen_sandbox() {
        let missing = errors(ManagementPermissions::override_(
            PermissionProfile::new().global(["sandbox/heartbeat"]),
        ))
        .await;
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("'frozen-box'"), "{missing:?}");

        let granted = errors(ManagementPermissions::override_(
            PermissionProfile::new().resource("frozen-box", [TEMPLATES]),
        ))
        .await;
        assert!(granted.is_empty(), "{granted:?}");
    }

    #[tokio::test]
    async fn templates_at_star_are_refused_in_every_profile() {
        for management in [
            ManagementPermissions::Extend(PermissionProfile::new().global([TEMPLATES])),
            ManagementPermissions::override_(
                PermissionProfile::new()
                    .global([TEMPLATES])
                    .resource("frozen-box", [TEMPLATES]),
            ),
        ] {
            let found = errors(management).await;
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].contains("'*'"), "{found:?}");
        }
        assert!(errors(ManagementPermissions::Auto).await.is_empty());
    }

    #[tokio::test]
    async fn templates_under_anything_but_a_frozen_sandbox_are_refused() {
        for scope in ["live-box", "no-such-resource"] {
            let found = errors(ManagementPermissions::Extend(
                PermissionProfile::new().resource(scope, [TEMPLATES]),
            ))
            .await;
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].contains(&format!("'{scope}'")), "{found:?}");
        }
        let frozen = errors(ManagementPermissions::Extend(
            PermissionProfile::new().resource("frozen-box", [TEMPLATES]),
        ))
        .await;
        assert!(frozen.is_empty(), "{frozen:?}");
    }

    #[tokio::test]
    async fn other_platforms_are_not_checked() {
        let stack = stack(ManagementPermissions::override_(PermissionProfile::new()));
        for platform in [Platform::Aws, Platform::Azure] {
            assert!(!SandboxTemplatePermissionsCheck.should_run(&stack, platform));
        }
    }
}
