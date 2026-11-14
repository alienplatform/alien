//! Which permission sets a protected install treats as built-in.

use std::sync::OnceLock;

use alien_core::PermissionSet;

use crate::registry::get_permission_set;

/// Whether `permission_set` is the built-in set of its id, unchanged. Anything else came from the
/// stack, so its grants and IAM conditions are not ones this repository reviewed.
pub fn is_built_in_unchanged(permission_set: &PermissionSet) -> bool {
    get_permission_set(&permission_set.id) == Some(permission_set)
}

/// The set that lets automatic AWS setup's management role read and sync the stack's vault
/// parameters. Setup adds it as an inline set, so it is held by its exact contents, not its id.
/// The deployer-secrets and signing Denies still apply on top of it.
pub fn management_stack_secrets_data() -> &'static PermissionSet {
    static SET: OnceLock<PermissionSet> = OnceLock::new();
    SET.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../protected-management/stack-secrets-data.json"
        ))
        .expect("protected-management/stack-secrets-data.json is a permission set")
    })
}
