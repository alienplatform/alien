//! Which service accounts may encrypt and decrypt with a setup-owned key.

use alien_core::{ServiceAccount, Stack};

/// Permission set that lets a runtime identity encrypt and decrypt with one key.
pub const REMOTE_CRYPTOGRAPHY: &str = "key/remote-cryptography";

/// Ids of the service accounts whose permission profile grants
/// [`REMOTE_CRYPTOGRAPHY`] on the key, by the key's id or with a `*` grant.
pub fn cryptography_service_accounts(stack: &Stack, key_id: &str) -> Vec<String> {
    stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<ServiceAccount>())
        .filter(|account| {
            stack
                .permission_profiles()
                .get(account.id.strip_suffix("-sa").unwrap_or(&account.id))
                .is_some_and(|profile| {
                    [key_id, "*"].into_iter().any(|scope| {
                        profile.0.get(scope).is_some_and(|refs| {
                            refs.iter()
                                .any(|reference| reference.id() == REMOTE_CRYPTOGRAPHY)
                        })
                    })
                })
        })
        .map(|account| account.id.clone())
        .collect()
}

/// Who gets what on one setup-owned key, worked out once from the stack.
///
/// CloudFormation, the Terraform emitters and the key controllers all render the key's policy and
/// grants from this plan, so a key made by Alien in a direct setup has the access a package-made
/// key has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAccessPlan {
    /// A Storage, Postgres, Redis or volume encrypts with the key through a cloud service.
    pub native_service_use: bool,
    /// The management profile holds `key/availability` on the key.
    pub availability_probe: bool,
    /// The stack has a management identity that can be named in the key policy.
    pub has_management_identity: bool,
    /// Permission set ids from the management profile under the key's id, in profile order
    /// (`key/management`, `key/availability`, `key/native-volume-management`).
    pub management: Vec<String>,
    /// The key is published through Remote Bindings and the stack has that identity.
    pub remote_bindings: bool,
    /// Service accounts granted [`REMOTE_CRYPTOGRAPHY`] on the key.
    pub runtime_cryptography: Vec<String>,
    /// Service accounts the AWS key policy allowlists (see `aws_key_policy`).
    pub policy_runtime_accounts: Vec<String>,
}

/// Works out the access plan for the key `key_id` of `stack`.
pub fn key_access_plan(stack: &Stack, key_id: &str) -> crate::Result<KeyAccessPlan> {
    let management: Vec<String> = stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(key_id))
        .into_iter()
        .flatten()
        .map(|reference| reference.id().to_string())
        .collect();
    let has_resource = |resource_type: alien_core::ResourceType| {
        stack
            .resources()
            .any(|(_, entry)| entry.config.resource_type() == resource_type)
    };
    let remote_access = stack
        .resources()
        .find(|(id, _)| id.as_str() == key_id)
        .is_some_and(|(_, entry)| alien_core::remote_bindings::remote_binding_for_entry(entry).is_some());
    Ok(KeyAccessPlan {
        native_service_use: crate::aws_key_policy::key_has_native_service_use(stack, key_id),
        availability_probe: management.iter().any(|id| id == "key/availability"),
        has_management_identity: has_resource(alien_core::RemoteStackManagement::RESOURCE_TYPE),
        management,
        remote_bindings: remote_access && has_resource(alien_core::RemoteBindings::RESOURCE_TYPE),
        runtime_cryptography: cryptography_service_accounts(stack, key_id),
        policy_runtime_accounts: crate::aws_key_policy::key_runtime_service_accounts(stack, key_id)?,
    })
}

/// IAM statements an AWS key's identities hold on it, with the key named by `key_arn`.
#[derive(Debug, Clone, PartialEq)]
pub struct AwsKeyGrants {
    /// For the management role: the management profile's sets on the key.
    pub management: Vec<serde_json::Value>,
    /// For the access role and each runtime role: encrypt and decrypt on the key.
    pub cryptography: serde_json::Value,
}

/// Renders the grants of [`KeyAccessPlan`] for AWS. `key_arn` is a literal ARN.
pub fn aws_key_grants(plan: &KeyAccessPlan, key_arn: &str) -> crate::Result<AwsKeyGrants> {
    use crate::{generators::AwsRuntimePermissionsGenerator, BindingTarget, PermissionContext};
    use alien_error::AlienError;

    let invalid = |message: String| AlienError::new(crate::ErrorData::InvalidPermissionSet { message });
    let context = PermissionContext::new().with_resource_name(key_arn.to_string());
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut management = Vec::new();
    for id in &plan.management {
        let set = crate::get_permission_set(id)
            .ok_or_else(|| invalid(format!("Unknown Key management permission '{id}'")))?;
        let document = generator.generate_policy(set, BindingTarget::Resource, &context)?;
        for mut statement in document.statement {
            statement.resource = vec![key_arn.to_string()];
            management.push(
                serde_json::to_value(statement)
                    .map_err(|error| invalid(format!("Failed to serialize Key grant: {error}")))?,
            );
        }
    }
    let actions = crate::get_permission_set(REMOTE_CRYPTOGRAPHY)
        .and_then(|set| set.platforms.aws.as_ref())
        .into_iter()
        .flatten()
        .flat_map(|entry| entry.grant.actions.iter().flatten())
        .cloned()
        .collect::<Vec<_>>();
    if actions.is_empty() {
        return Err(invalid(format!("{REMOTE_CRYPTOGRAPHY} has no AWS actions")));
    }
    Ok(AwsKeyGrants {
        management,
        cryptography: serde_json::json!({
            "Effect": "Allow",
            "Action": actions,
            "Resource": key_arn,
        }),
    })
}

/// Role bindings a GCP key's identities hold on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcpKeyRoles {
    /// Predefined role of [`REMOTE_CRYPTOGRAPHY`], for the access account and runtime accounts.
    pub cryptography: String,
    /// Role for the management account, when the management profile holds `key/management`.
    pub management: Option<&'static str>,
}

/// Renders the roles of [`KeyAccessPlan`] for GCP. The Terraform emitter and the key controller
/// both bind these roles, so they agree on who may do what with the key.
pub fn gcp_key_roles(plan: &KeyAccessPlan) -> crate::Result<GcpKeyRoles> {
    use alien_error::AlienError;
    let invalid = |message: String| AlienError::new(crate::ErrorData::InvalidPermissionSet { message });
    let cryptography = crate::get_permission_set(REMOTE_CRYPTOGRAPHY)
        .and_then(|set| set.platforms.gcp.as_ref())
        .and_then(|entries| entries.first())
        .and_then(|entry| entry.grant.predefined_roles.as_ref())
        .and_then(|roles| roles.first().cloned())
        .ok_or_else(|| invalid(format!("{REMOTE_CRYPTOGRAPHY} has no GCP predefined role")))?;
    Ok(GcpKeyRoles {
        cryptography,
        management: plan
            .management
            .iter()
            .any(|id| id == "key/management")
            .then_some("roles/cloudkms.viewer"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Key, PermissionProfile, PermissionsConfig, ResourceLifecycle};

    fn stack(profiles: PermissionsConfig) -> Stack {
        let mut stack = Stack::new("stack".to_string()).add(
            Key::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
        for id in ["manager-sa", "other-sa", "wild-sa", "reader-sa"] {
            stack = stack.add(
                ServiceAccount::new(id.to_string()).build(),
                ResourceLifecycle::Frozen,
            );
        }
        stack.permissions(profiles).build()
    }

    #[test]
    fn only_profiles_granted_cryptography_on_the_key_are_listed() {
        let stack = stack(
            PermissionsConfig::new()
                .with_profile(
                    "manager",
                    PermissionProfile::new().resource("secrets", [REMOTE_CRYPTOGRAPHY]),
                )
                .with_profile(
                    "other",
                    PermissionProfile::new().resource("elsewhere", [REMOTE_CRYPTOGRAPHY]),
                )
                .with_profile(
                    "wild",
                    PermissionProfile::new().global([REMOTE_CRYPTOGRAPHY]),
                )
                .with_profile(
                    "reader",
                    PermissionProfile::new().resource("secrets", ["key/availability"]),
                ),
        );

        assert_eq!(
            cryptography_service_accounts(&stack, "secrets"),
            vec!["manager-sa".to_string(), "wild-sa".to_string()]
        );
    }
}
