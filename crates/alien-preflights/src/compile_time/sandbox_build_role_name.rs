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
//! It also refuses an id long enough that IAM's 64-character ceiling costs the suffix to a hash,
//! which would leave the image build refused the role it needs at apply.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Sandbox, Stack};

/// Suffix `sandbox/provision` scopes its `iam:PassRole` to.
const BUILD_ROLE_SUFFIX: &str = "-build";

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

        for (resource_id, entry) in stack.resources() {
            let is_sandbox =
                entry.config.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref();

            if !is_sandbox && resource_id.ends_with(BUILD_ROLE_SUFFIX) {
                errors.push(format!(
                    "Resource '{resource_id}' ends in '{BUILD_ROLE_SUFFIX}', which is reserved: a \
                     sandbox's build role is named '<prefix>-<sandbox>{BUILD_ROLE_SUFFIX}' and \
                     that name is what limits which role the image build may run as. Rename it."
                ));
                continue;
            }

            if is_sandbox {
                let longest =
                    MAX_RESOURCE_PREFIX_LEN + 1 + resource_id.len() + BUILD_ROLE_SUFFIX.len();
                if longest > IAM_ROLE_NAME_MAX_LEN {
                    errors.push(format!(
                        "Sandbox '{resource_id}' makes a build role name of up to {longest} \
                         characters, past IAM's {IAM_ROLE_NAME_MAX_LEN}. Checked against the \
                         widest prefix a deployment may use rather than this one's, because the \
                         same stack is installed under prefixes of differing length. Past the \
                         ceiling the name either loses its '{BUILD_ROLE_SUFFIX}' suffix to a hash \
                         and is refused the role it needs, or is rejected by IAM outright. Use a \
                         shorter id."
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
    use alien_core::{Kv, ResourceLifecycle, SandboxCode, SandboxEgress, SandboxSessionPolicy};

    fn sandbox(id: &str) -> Sandbox {
        Sandbox::new(id.to_string())
            .code(SandboxCode::Image {
                image: "s3://bucket/sandbox.zip".to_string(),
            })
            .egress(SandboxEgress::Allow)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
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

    /// Pinned at the boundary, not near it: with a widest-prefix budget of 40 the longest id that
    /// still fits is 17, and every constant in the arithmetic could drift several characters
    /// before a test using 6 and one using 40 noticed.
    #[tokio::test]
    async fn the_length_boundary_is_where_the_arithmetic_says_it_is() {
        let longest_that_fits = Stack::new("app".to_string())
            .add(sandbox(&"r".repeat(17)), ResourceLifecycle::Frozen)
            .build();
        assert!(
            run(longest_that_fits).await.success,
            "17 characters still leaves the suffix intact"
        );

        let one_too_many = Stack::new("app".to_string())
            .add(sandbox(&"r".repeat(18)), ResourceLifecycle::Frozen)
            .build();
        assert!(
            !run(one_too_many).await.success,
            "18 characters passes IAM's ceiling at the widest prefix"
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
