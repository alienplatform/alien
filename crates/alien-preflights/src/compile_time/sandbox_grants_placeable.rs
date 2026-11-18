//! Refuses at build a sandbox grant that Azure setup would refuse to place.
//!
//! The Azure setup package grants a `sandbox/*` set on one sandbox group, and its emitter refuses
//! the grants it cannot place. Those refusals arrive at deploy, after the stack is built and
//! released. This check applies the same rules to the declared profiles, so the build says what to
//! change. A `"*"` entry never fails here: the sets it names that have no stack binding are placed
//! on every sandbox group, and `sandbox/provision` under `"*"` stays at the stack.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{PermissionSet, PermissionSetReference, Platform, Sandbox, Stack};

pub struct SandboxGrantsPlaceableCheck;

fn has_resource_binding(set: &PermissionSet) -> bool {
    set.platforms
        .azure
        .as_ref()
        .is_some_and(|permissions| permissions.iter().any(|p| p.binding.resource.is_some()))
}

#[async_trait::async_trait]
impl CompileTimeCheck for SandboxGrantsPlaceableCheck {
    fn description(&self) -> &'static str {
        "A grant on a sandbox must be one the setup can place on its sandbox group"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Azure
            && stack.resources().any(|(_, entry)| {
                entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
            })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        for (resource_id, entry) in stack.resources() {
            if entry.config.downcast_ref::<Sandbox>().is_none() {
                continue;
            }
            for (profile_name, profile) in stack.permission_profiles() {
                for reference in profile.0.get(resource_id).into_iter().flatten() {
                    let grant = |reason: &str| {
                        format!(
                            "Profile '{profile_name}' grants {} on sandbox '{resource_id}', which \
                             Azure cannot place: {reason}",
                            reference.id()
                        )
                    };
                    let name = match reference {
                        PermissionSetReference::Name(name) if name.starts_with("sandbox/") => name,
                        PermissionSetReference::Name(_) => {
                            errors.push(grant(
                                "only sandbox/* permission sets can be granted on a sandbox. Move \
                                 it to the resource it belongs to",
                            ));
                            continue;
                        }
                        PermissionSetReference::Inline(_) => {
                            errors.push(grant(
                                "only built-in permission sets referenced by name can be granted \
                                 on a sandbox. Reference a built-in sandbox/* set by name",
                            ));
                            continue;
                        }
                    };
                    if name == "sandbox/provision" {
                        errors.push(grant(
                            "sandbox/provision is held at the stack on Azure. Grant it under \
                             \"*\" instead of keying it by the sandbox",
                        ));
                        continue;
                    }
                    if entry.has_remote_bindings() {
                        errors.push(grant(
                            "the sandbox is published for remote access and belongs to its remote \
                             caller, so no workload identity can hold a grant on it. Remove the \
                             grant, or remove remoteAccess from the sandbox",
                        ));
                        continue;
                    }
                    if let Some(set) = alien_permissions::get_permission_set(name) {
                        if set.platforms.azure.is_some() && !has_resource_binding(&set) {
                            errors.push(grant(
                                "the set has no Azure resource binding to place on a sandbox \
                                 group. Grant it under \"*\" instead",
                            ));
                        }
                    }
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

    fn stack(profile: PermissionProfile, remote_access: bool) -> Stack {
        let sandbox = Sandbox::new("box".to_string())
            .code(SandboxCode::Image {
                image: "python:3.12".to_string(),
                bundled_from: None,
            })
            .egress(SandboxEgress::None)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let mut stack = Stack::new("s".to_string())
            .add(sandbox, ResourceLifecycle::Frozen)
            .permission("worker", profile)
            .build();
        stack.resources.get_mut("box").unwrap().remote_access = remote_access;
        stack
    }

    async fn errors(profile: PermissionProfile, remote_access: bool) -> Vec<String> {
        SandboxGrantsPlaceableCheck
            .check(&stack(profile, remote_access), Platform::Azure)
            .await
            .unwrap()
            .errors
    }

    #[tokio::test]
    async fn the_customer_shape_and_a_named_execute_grant_build() {
        let star = PermissionProfile::new().resource(
            "*",
            ["sandbox/execute", "sandbox/management", "sandbox/provision"],
        );
        assert!(errors(star, false).await.is_empty());
        let named = PermissionProfile::new().resource("box", ["sandbox/execute"]);
        assert!(errors(named, false).await.is_empty());
    }

    #[tokio::test]
    async fn a_grant_the_azure_setup_would_refuse_fails_at_build_and_says_what_to_change() {
        let keyed_provision = PermissionProfile::new().resource("box", ["sandbox/provision"]);
        let found = errors(keyed_provision, false).await;
        assert!(
            found.iter().any(|e| e.contains("Grant it under \"*\"")),
            "{found:?}"
        );

        let published = PermissionProfile::new().resource("box", ["sandbox/execute"]);
        let found = errors(published, true).await;
        assert!(
            found.iter().any(|e| e.contains("remove remoteAccess")),
            "{found:?}"
        );

        let not_a_sandbox_set = PermissionProfile::new().resource("box", ["storage/data-read"]);
        let found = errors(not_a_sandbox_set, false).await;
        assert!(
            found
                .iter()
                .any(|e| e.contains("only sandbox/* permission sets")),
            "{found:?}"
        );
    }
}
