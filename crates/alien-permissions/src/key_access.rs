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
