//! Refuses the Azure management profiles whose `sandbox/images` grant would bind nothing.
//!
//! The grant binds only on a sandbox's own group, so at `*` it fails to render, under any other
//! resource id it binds nothing, and an override that leaves it off a registry-image sandbox 403s
//! at its build.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::permissions::PermissionSetReference;
use alien_core::{AzureSandboxImage, ManagementPermissions, Platform, Sandbox, Stack};

const IMAGES: &str = "sandbox/images";

/// Ensures `sandbox/images` is granted per sandbox on Azure, and present under an override.
pub struct SandboxImagePermissionsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SandboxImagePermissionsCheck {
    fn description(&self) -> &'static str {
        "Azure sandbox image permissions must be scoped to the sandbox that needs them"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Azure
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
                // By registry name, as the emitter binds it: an inline set's id is whatever its
                // author typed.
                .is_some_and(|refs| {
                    refs.iter().any(|reference| {
                        matches!(reference, PermissionSetReference::Name(name) if name == IMAGES)
                    })
                })
        };
        let is_sandbox = |resource_id: &str| {
            stack.resources().any(|(id, entry)| {
                id == resource_id && entry.config.downcast_ref::<Sandbox>().is_some()
            })
        };

        let mut errors = Vec::new();
        // The setup package renders this set's role by its registry name, so an inline set
        // carrying the name would render in its place.
        let borrows_the_name = profile.0.values().flatten().any(|reference| {
            matches!(reference, PermissionSetReference::Inline(set) if set.id == IMAGES)
        });
        if borrows_the_name {
            errors.push(format!(
                "An inline permission set cannot be named '{IMAGES}', which is a built-in set. \
                 Rename it."
            ));
        }
        if grants("*") {
            errors.push(format!(
                "'{IMAGES}' cannot be granted at '*': it binds on one sandbox's group. Grant it \
                 under each sandbox's id instead."
            ));
        }
        for scope in profile.0.keys() {
            if scope != "*" && grants(scope) && !is_sandbox(scope) {
                errors.push(format!(
                    "'{IMAGES}' is granted under '{scope}', which is not a sandbox. Only a \
                     sandbox's group takes this grant; remove it from '{scope}'."
                ));
            }
        }
        // Only a registry image is built by the manager, so a catalog-only sandbox needs nothing.
        if matches!(stack.management(), ManagementPermissions::Override(_)) {
            for (resource_id, entry) in stack.resources() {
                let builds_an_image =
                    entry
                        .config
                        .downcast_ref::<Sandbox>()
                        .is_some_and(|sandbox| {
                            matches!(sandbox.azure_image(), Ok(AzureSandboxImage::Registry(_)))
                        });
                if builds_an_image && !grants(resource_id) {
                    errors.push(format!(
                        "Setup required: sandbox '{resource_id}' runs a registry image and needs \
                         management permission '{IMAGES}' to build it. The stack overrides \
                         management permissions, so add '{IMAGES}' under '{resource_id}' and \
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
    use alien_core::{
        PermissionProfile, ResourceLifecycle, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    };

    fn stack(management: ManagementPermissions) -> Stack {
        let sandbox = |id: &str, image: &str| {
            Sandbox::new(id.to_string())
                .code(SandboxCode::Image {
                    image: image.to_string(),
                })
                .egress(SandboxEgress::Deny)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build()
        };
        Stack::new("test".to_string())
            .add(
                sandbox("registry-box", "docker.io/library/python:3.14-slim"),
                ResourceLifecycle::Frozen,
            )
            .add(sandbox("catalog-box", "ubuntu"), ResourceLifecycle::Frozen)
            .management(management)
            .build()
    }

    async fn errors(management: ManagementPermissions) -> Vec<String> {
        let stack = stack(management);
        assert!(SandboxImagePermissionsCheck.should_run(&stack, Platform::Azure));
        SandboxImagePermissionsCheck
            .check(&stack, Platform::Azure)
            .await
            .expect("the check runs")
            .errors
    }

    #[tokio::test]
    async fn an_override_must_grant_images_on_each_registry_image_sandbox() {
        let missing = errors(ManagementPermissions::override_(
            PermissionProfile::new().global(["sandbox/heartbeat"]),
        ))
        .await;
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("'registry-box'"), "{missing:?}");

        let granted = errors(ManagementPermissions::override_(
            PermissionProfile::new().resource("registry-box", [IMAGES]),
        ))
        .await;
        assert!(granted.is_empty(), "{granted:?}");
    }

    #[tokio::test]
    async fn images_at_star_are_refused_in_every_profile() {
        for management in [
            ManagementPermissions::Extend(PermissionProfile::new().global([IMAGES])),
            ManagementPermissions::override_(
                PermissionProfile::new()
                    .global([IMAGES])
                    .resource("registry-box", [IMAGES]),
            ),
        ] {
            let found = errors(management).await;
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].contains("'*'"), "{found:?}");
        }
        assert!(errors(ManagementPermissions::Auto).await.is_empty());
    }

    #[tokio::test]
    async fn images_under_anything_but_a_sandbox_are_refused() {
        let found = errors(ManagementPermissions::Extend(
            PermissionProfile::new().resource("no-such-resource", [IMAGES]),
        ))
        .await;
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'no-such-resource'"), "{found:?}");

        let on_a_sandbox = errors(ManagementPermissions::Extend(
            PermissionProfile::new().resource("catalog-box", [IMAGES]),
        ))
        .await;
        assert!(on_a_sandbox.is_empty(), "{on_a_sandbox:?}");
    }

    #[tokio::test]
    async fn an_inline_set_named_like_images_is_refused() {
        let borrowed = alien_core::permissions::PermissionSet {
            id: IMAGES.to_string(),
            description: "not the built-in set".to_string(),
            platforms: alien_core::permissions::PlatformPermissions {
                aws: None,
                gcp: None,
                azure: None,
            },
        };
        let found = errors(ManagementPermissions::Extend(
            PermissionProfile::new().resource(
                "registry-box",
                [PermissionSetReference::from_inline(borrowed)],
            ),
        ))
        .await;
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("inline"), "{found:?}");
    }

    #[tokio::test]
    async fn other_platforms_are_not_checked() {
        let stack = stack(ManagementPermissions::override_(PermissionProfile::new()));
        for platform in [Platform::Aws, Platform::Gcp] {
            assert!(!SandboxImagePermissionsCheck.should_run(&stack, platform));
        }
    }
}
