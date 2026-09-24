//! Keeps a sandbox's AWS build role name unambiguous and intact.
//!
//! `sandbox/provision` scopes `iam:PassRole` by name, and two other emitted roles can wear the
//! same `<prefix>-<id>-build` shape. They are not covered by the same thing:
//!
//! - a `Build` resource's fallback role trusts codebuild and not lambda, so the image builder
//!   cannot assume it and the pass fails at use — nothing here is needed for that one;
//! - a service account whose id ends in `-build` trusts lambda by default and carries whatever
//!   sets its author attached, so **this check is the only thing standing between it and a
//!   customer-authored Dockerfile running as it**.
//!
//! It also refuses an id long enough that IAM's 64-character ceiling costs a suffix to a hash,
//! which would leave the image build refused the role it needs at apply, and an id that takes the
//! name of a deny sandbox's egress operator role.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Build, Platform, Sandbox, SandboxEgress, Stack};

/// Suffix `sandbox/provision` scopes its `iam:PassRole` to.
const BUILD_ROLE_SUFFIX: &str = "-build";

/// Suffix of the egress operator role setup creates for a deny sandbox, longer than the build
/// role's. Only a deny sandbox is held to it: an open one creates no such role, and a switch to
/// deny runs this check again.
const EGRESS_ROLE_SUFFIX: &str = "-egress";

/// IAM's ceiling on a role name, past which the generators hash the tail away.
const IAM_ROLE_NAME_MAX_LEN: usize = 64;

/// Longest resource prefix the generators accept, used as the worst case here.
///
/// Checked against the widest prefix rather than the one in hand: the same stack is rendered for
/// deployments whose prefixes differ, and a name that fits one and not another would fail in a
/// customer's account rather than at plan time.
const MAX_RESOURCE_PREFIX_LEN: usize = 40;

/// Whether the stack's own management profile asks for `sandbox/provision`.
fn profile_names_sandbox_provision(stack: &Stack) -> bool {
    let profile = match stack.management() {
        alien_core::permissions::ManagementPermissions::Auto => return false,
        alien_core::permissions::ManagementPermissions::Extend(profile)
        | alien_core::permissions::ManagementPermissions::Override(profile) => profile,
    };
    profile
        .0
        .values()
        .flatten()
        .any(|reference| reference.id() == "sandbox/provision")
}

/// Refuses the two id shapes that would blunt the grant's name scoping.
pub struct SandboxBuildRoleNameCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SandboxBuildRoleNameCheck {
    fn description(&self) -> &'static str {
        "A sandbox's AWS build role name must stay unambiguous and unhashed"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        if platform != Platform::Aws {
            return false;
        }
        let declares_a_sandbox = stack.resources().any(|(_, entry)| {
            entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
        });

        // The grant follows the permission set, not the resource: a profile naming
        // `sandbox/provision` carries the pass whether or not a sandbox is declared, and a stack
        // that skipped this check could then declare a service account whose role ends in
        // `-build` and be passed it.
        declares_a_sandbox || profile_names_sandbox_provision(stack)
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let deny_sandbox_ids: Vec<&str> = stack
            .resources()
            .filter(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<Sandbox>()
                    .is_some_and(|sandbox| sandbox.egress == SandboxEgress::Deny)
            })
            .map(|(resource_id, _)| resource_id.as_str())
            .collect();

        for (resource_id, entry) in stack.resources() {
            let resource_type = entry.config.resource_type();
            let is_sandbox = resource_type.as_ref() == Sandbox::RESOURCE_TYPE.as_ref();
            // A `Build` is the one type that may wear the shape safely: its role trusts codebuild,
            // so the image builder cannot assume it whatever the name says. Refusing it would cost
            // a caller the most natural id for a build without buying anything.
            let is_build = resource_type.as_ref() == Build::RESOURCE_TYPE.as_ref();

            if !is_sandbox && !is_build && resource_id.ends_with(BUILD_ROLE_SUFFIX) {
                errors.push(format!(
                    "Resource '{resource_id}' ends in '{BUILD_ROLE_SUFFIX}', which is reserved: a \
                     sandbox's build role is named '<prefix>-<sandbox>{BUILD_ROLE_SUFFIX}' and \
                     that name is what limits which role the image build may run as. Rename it."
                ));
                continue;
            }

            if !is_sandbox {
                // A service account's role is `<prefix>-<id>`, which here is the operator role's
                // name, and no install path can create both.
                if let Some(sandbox_id) = resource_id
                    .strip_suffix(EGRESS_ROLE_SUFFIX)
                    .filter(|stem| deny_sandbox_ids.contains(stem))
                {
                    errors.push(format!(
                        "Resource '{resource_id}' takes the name of the egress operator role of \
                         sandbox '{sandbox_id}', '<prefix>-{sandbox_id}{EGRESS_ROLE_SUFFIX}'. \
                         Rename it."
                    ));
                }
                continue;
            }

            let suffix = if deny_sandbox_ids.contains(&resource_id.as_str()) {
                EGRESS_ROLE_SUFFIX
            } else {
                BUILD_ROLE_SUFFIX
            };
            let longest = MAX_RESOURCE_PREFIX_LEN + 1 + resource_id.len() + suffix.len();
            if longest > IAM_ROLE_NAME_MAX_LEN {
                errors.push(format!(
                    "Sandbox '{resource_id}' makes a role name of up to {longest} characters, \
                     past IAM's {IAM_ROLE_NAME_MAX_LEN}. Checked against the widest prefix a \
                     deployment may use rather than this one's, because the same stack is \
                     installed under prefixes of differing length. Past the ceiling the build \
                     role loses its '{BUILD_ROLE_SUFFIX}' suffix to a hash and is refused the \
                     role it needs, or IAM rejects the name outright. Use a shorter id."
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
    use alien_core::{Kv, ResourceLifecycle, SandboxCode, SandboxLifecyclePolicy};

    fn sandbox(id: &str) -> Sandbox {
        Sandbox::new(id.to_string())
            .code(SandboxCode::Image {
                image: "s3://bucket/sandbox.zip".to_string(),
            })
            .egress(SandboxEgress::Allow)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build()
    }

    async fn run(stack: Stack) -> CheckResult {
        SandboxBuildRoleNameCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("the check itself must not fail")
    }

    #[tokio::test]
    async fn an_ordinary_sandbox_passes() {
        let stack = Stack::new("app".to_string())
            .add(sandbox("runner"), ResourceLifecycle::Frozen)
            .build();
        assert!(run(stack).await.success);
    }

    /// Another resource ending in `-build` produces a role matching the pattern meant to name only
    /// the sandbox's build role, and it carries whatever sets its author attached.
    #[tokio::test]
    async fn another_resource_may_not_claim_the_build_role_name() {
        let stack = Stack::new("app".to_string())
            .add(sandbox("runner"), ResourceLifecycle::Frozen)
            .add(
                Kv::new("image-build".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let result = run(stack).await;
        assert!(!result.success, "a reserved suffix must be refused");
        assert!(result.errors.iter().any(|error| error.contains("reserved")));
    }

    /// The natural id for a build is `<something>-build`, and a `Build` may keep it: its role
    /// trusts codebuild, so the image builder cannot assume it however the name reads. Refusing it
    /// would block a stack that is not at risk.
    #[tokio::test]
    async fn a_build_resource_may_keep_the_build_suffix() {
        let stack = Stack::new("app".to_string())
            .add(sandbox("runner"), ResourceLifecycle::Frozen)
            .add(
                Build::new("image-build".to_string())
                    .permissions("build-execution".to_string())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let result = run(stack).await;
        assert!(
            result.success,
            "a Build may wear the suffix: {:?}",
            result.errors
        );
    }

    /// The grant follows the permission set, so a profile can carry it with no sandbox in sight.
    ///
    /// A service account is user-declarable and its role is named `<prefix>-<id>`, so one whose
    /// id ends in `-build` wears the grant's shape — and it trusts lambda by default — so unlike a `Build` role, nothing at the trust policy stops the
    /// image builder assuming it and running a customer-authored Dockerfile as it.
    #[tokio::test]
    async fn a_profile_asking_for_provision_is_checked_without_a_sandbox_present() {
        use alien_core::permissions::{ManagementPermissions, PermissionProfile};

        let stack = Stack::new("app".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global(["sandbox/provision"]),
            ))
            .add(
                Kv::new("image-build".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        assert!(
            SandboxBuildRoleNameCheck.should_run(&stack, Platform::Aws),
            "the grant is present, so the guard has to be too"
        );
        let result = run(stack).await;
        assert!(!result.success, "the colliding id must still be refused");
    }

    fn deny_sandbox(id: &str) -> Sandbox {
        Sandbox {
            egress: SandboxEgress::Deny,
            ..sandbox(id)
        }
    }

    async fn fits(sandbox: Sandbox) -> bool {
        run(Stack::new("app".to_string())
            .add(sandbox, ResourceLifecycle::Frozen)
            .build())
        .await
        .success
    }

    /// Pinned at the boundary: with a widest prefix of 40, a deny sandbox's `-egress` leaves 16 for
    /// the id and an open one's `-build` leaves 17. A test far from the edge would miss a constant
    /// drifting by several characters.
    #[tokio::test]
    async fn the_length_boundary_is_where_the_arithmetic_says_it_is() {
        assert!(fits(deny_sandbox(&"r".repeat(16))).await);
        assert!(
            !fits(deny_sandbox(&"r".repeat(17))).await,
            "17 characters takes the egress role name past IAM's ceiling at the widest prefix"
        );
        assert!(
            fits(sandbox(&"r".repeat(17))).await,
            "an open sandbox creates no egress role, so only its build role counts"
        );
        assert!(!fits(sandbox(&"r".repeat(18))).await);
    }

    #[tokio::test]
    async fn another_resource_may_not_take_a_deny_sandboxs_egress_role_name() {
        let with = |sandbox: Sandbox| {
            Stack::new("app".to_string())
                .add(sandbox, ResourceLifecycle::Frozen)
                .add(
                    Kv::new("runner-egress".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build()
        };

        let result = run(with(deny_sandbox("runner"))).await;
        assert!(!result.success, "the operator role's name must be refused");
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("egress operator role")));

        assert!(
            run(with(sandbox("runner"))).await.success,
            "an open sandbox has no operator role to collide with"
        );
    }

    /// Past IAM's ceiling the generators hash the tail away, taking the suffix the grant matches.
    #[tokio::test]
    async fn a_sandbox_id_that_would_lose_the_suffix_is_refused() {
        let stack = Stack::new("app".to_string())
            .add(sandbox(&"r".repeat(40)), ResourceLifecycle::Frozen)
            .build();

        let result = run(stack).await;
        assert!(
            !result.success,
            "a name that loses its suffix must be refused"
        );
        assert!(result.errors.iter().any(|error| error.contains("IAM's 64")));
    }
}
