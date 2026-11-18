//! What a direct setup run last reconciled around a sandbox.
//!
//! A sandbox that setup created places grants for the stack's profiles, its management
//! identity and its remote caller. A later setup run must place the ones a newly added profile
//! or caller needs, and must do nothing when none changed. The controller keeps a fingerprint of
//! the inputs that decide the grants and asks for an update only when the desired one differs.

use alien_core::Stack;
use sha2::{Digest, Sha256};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, IntoAlienError};

/// The inputs that decide the grants placed for the sandbox: the profiles and the management
/// permissions, which identities (service accounts, remote callers, management) the stack
/// declares for them to land on, and the management identity itself, which the deployment
/// supplies and the stack does not name (`management_identity`: the email or principal the
/// management grant is made to). A change to any of them is one update; a steady state is none.
/// An identity that cannot be read yet counts as absent, so it is picked up when it can.
pub(crate) fn grant_inputs_fingerprint(
    ctx: &ResourceControllerContext<'_>,
    management_identity: Result<Option<String>>,
) -> Result<String> {
    fingerprint_of(ctx.desired_stack, management_identity.ok().flatten())
}

fn fingerprint_of(stack: &Stack, management_identity: Option<String>) -> Result<String> {
    let mut identities: Vec<(String, String)> = stack
        .resources()
        .map(|(id, entry)| (entry.config.resource_type().to_string(), id.clone()))
        .collect();
    identities.sort();
    let inputs = serde_json::to_vec(&(&stack.permissions, identities, management_identity))
        .into_alien_error()
        .map_err(|error| {
            AlienError::new(ErrorData::InfrastructureError {
                message: format!("Cannot fingerprint the grant inputs: {error}"),
                operation: Some("grant_inputs_fingerprint".to_string()),
                resource_id: None,
            })
        })?;
    Ok(format!("{:x}", Sha256::digest(inputs)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::PermissionProfile;

    #[test]
    fn the_fingerprint_changes_with_a_profile_and_not_without() {
        let base = || Stack::new("s".to_string()).build();
        let mut with_profile = base();
        let same = fingerprint_of(&base(), None).unwrap();
        assert_eq!(same, fingerprint_of(&base(), None).unwrap());
        with_profile.permissions.profiles.insert(
            "worker".to_string(),
            PermissionProfile::new().global(["sandbox/execute"]),
        );
        assert_ne!(same, fingerprint_of(&with_profile, None).unwrap());
    }

    #[test]
    fn the_fingerprint_changes_with_the_management_identity() {
        let stack = Stack::new("s".to_string()).build();
        let one = fingerprint_of(&stack, Some("manager-1".to_string())).unwrap();
        assert_eq!(
            one,
            fingerprint_of(&stack, Some("manager-1".to_string())).unwrap()
        );
        assert_ne!(
            one,
            fingerprint_of(&stack, Some("manager-2".to_string())).unwrap()
        );
        assert_ne!(one, fingerprint_of(&stack, None).unwrap());
    }
}
