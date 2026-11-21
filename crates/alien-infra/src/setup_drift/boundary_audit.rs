//! Policies someone added by hand to a service-account role that the permissions boundary will
//! limit.
//!
//! A setup update on an install that never had the permissions boundary puts every
//! service-account role under it. The boundary caps what *all* the role's policies grant, so a
//! hand-added policy that grants anything outside it stops granting that, and nothing reports
//! it. This reads the live roles (read-only) so the CLI can name those policies before the
//! operator confirms.

use alien_aws_clients::iam::IamApi;
use alien_core::StackState;

use crate::service_account::AwsServiceAccountController;

/// A policy on a role that Alien did not write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandAddedPolicy {
    pub role: String,
    pub policy: String,
}

/// Whether Alien writes an inline policy with this name: the profile policy, per-resource grants
/// and remote-access policies all start with `deployment-` (`alien-` before the rename).
fn is_alien_named_policy(name: &str) -> bool {
    name.starts_with("deployment-") || name.starts_with("alien-")
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
    state: &StackState,
) -> Result<Vec<HandAddedPolicy>, String> {
    let mut found = Vec::new();
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
            .filter(|name| !is_alien_named_policy(name))
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

    #[test]
    fn alien_written_policy_names_are_not_hand_added() {
        for name in [
            "deployment-permissions",
            "deployment-appvault-vault-protect-unmapped-deployer-secrets",
            "alien-data-storage-data-read",
        ] {
            assert!(is_alien_named_policy(name), "{name}");
        }
        for name in ["custom-hand-added", "AllowEverything", "deploymentless"] {
            assert!(!is_alien_named_policy(name), "{name}");
        }
    }
}
