//! Policies someone added by hand to a service-account role that the permissions boundary will
//! limit.
//!
//! A setup update on an install that never had the permissions boundary puts every
//! service-account role under it. The boundary caps what *all* the role's policies grant, so a
//! hand-added policy that grants anything outside it stops granting that, and nothing reports
//! it. This reads the live roles (read-only) so the CLI can name those policies before the
//! operator confirms.

use alien_aws_clients::iam::IamApi;
use alien_core::{Stack, StackState};

use crate::service_account::AwsServiceAccountController;

/// A policy on a role that Alien did not write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandAddedPolicy {
    pub role: String,
    pub policy: String,
}

/// Whether Alien writes an inline policy with this exact name for one of these roles' grants:
/// the profile policy, or a per-resource grant (`deployment-<resource>-<permission set>`, also
/// under the previous `alien-` prefix) of a permission set the stack or the catalog defines.
/// A prefix alone is not enough: a policy someone named `deployment-custom-access` is theirs.
fn is_alien_named_policy(name: &str, permission_set_ids: &[String]) -> bool {
    name == "deployment-permissions"
        || name == "alien-permissions"
        || alien_permissions::aws_resource_policies::names_aws_resource_grant(
            name,
            permission_set_ids.iter().map(String::as_str),
        )
}

/// The permission set ids a grant policy can be named for: the catalog's and the stack's own.
fn known_permission_set_ids(stack: &alien_core::Stack) -> Vec<String> {
    let mut ids: Vec<String> = alien_permissions::list_permission_set_ids()
        .into_iter()
        .map(str::to_string)
        .collect();
    ids.extend(
        stack
            .permissions
            .profiles
            .values()
            .flat_map(|profile| profile.0.values())
            .flatten()
            .map(|reference| reference.id().to_string()),
    );
    ids
}

/// The IAM role names of the stack's AWS service accounts, from the recorded controller state.
fn service_account_role_names(state: &StackState) -> Vec<String> {
    let mut names: Vec<String> = state
        .resources
        .values()
        .filter(|resource| resource.resource_type == "service-account")
        .filter_map(|resource| resource.internal_state.clone())
        .filter_map(|internal| serde_json::from_value::<AwsServiceAccountController>(internal).ok())
        .filter_map(|controller| controller.role_name)
        .collect();
    names.sort();
    names
}

/// Hand-added inline and customer-managed attached policies on the service-account roles that do
/// not yet have a permissions boundary. AWS-managed policies are not listed: adopting a role
/// that has one is refused with its own message.
///
/// An error means a read was denied or failed; the caller says so in one line and proceeds.
pub async fn hand_added_policies_under_new_boundary(
    iam: &dyn IamApi,
    stack: &Stack,
    state: &StackState,
) -> Result<Vec<HandAddedPolicy>, String> {
    let mut found = Vec::new();
    let permission_set_ids = known_permission_set_ids(stack);
    for role_name in service_account_role_names(state) {
        let role = iam
            .get_role(&role_name)
            .await
            .map_err(|error| format!("could not read role '{role_name}': {error}"))?
            .get_role_result
            .role;
        if role.permissions_boundary.is_some() {
            continue;
        }
        let inline = iam
            .list_role_policies(&role_name)
            .await
            .map_err(|error| format!("could not list the policies of role '{role_name}': {error}"))?
            .list_role_policies_result
            .policy_names
            .map(|names| names.member)
            .unwrap_or_default();
        let attached = iam
            .list_attached_role_policies(&role_name)
            .await
            .map_err(|error| {
                format!("could not list the attached policies of role '{role_name}': {error}")
            })?
            .list_attached_role_policies_result
            .attached_policies
            .map(|policies| policies.member)
            .unwrap_or_default();
        let customer_managed = attached
            .into_iter()
            .filter(|policy| policy.policy_arn.split(':').nth(4) != Some("aws"))
            .map(|policy| policy.policy_name);
        for policy in inline
            .into_iter()
            .filter(|name| !is_alien_named_policy(name, &permission_set_ids))
            .chain(customer_managed)
        {
            found.push(HandAddedPolicy {
                role: role_name.clone(),
                policy,
            });
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        vec![
            "vault/protect-unmapped-deployer-secrets".to_string(),
            "data-storage/data-read".to_string(),
        ]
    }

    #[test]
    fn alien_written_policy_names_are_not_hand_added() {
        for name in [
            "deployment-permissions",
            "deployment-appvault-vault-protect-unmapped-deployer-secrets",
            "alien-data-data-storage-data-read",
        ] {
            assert!(is_alien_named_policy(name, &ids()), "{name}");
        }
    }

    /// A prefix is not ownership: a hand-added policy that merely starts with `deployment-` or
    /// `alien-` is still the customer's and is warned about.
    #[test]
    fn a_hand_added_policy_with_an_alien_looking_prefix_is_still_hand_added() {
        for name in [
            "deployment-custom-access",
            "alien-extra",
            "custom-hand-added",
            "AllowEverything",
            "deploymentless",
        ] {
            assert!(!is_alien_named_policy(name, &ids()), "{name}");
        }
    }
}
