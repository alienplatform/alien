use alien_core::PermissionProfile;
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};

use crate::{get_permission_set, ErrorData, Result};

/// Digest of the resolved management grants, shared by setup registration and runtime.
/// Changing the body of a named permission set changes this revision even when
/// the stack still references the same name.
pub fn management_permissions_revision(
    profile: Option<&PermissionProfile>,
) -> Result<Option<String>> {
    let Some(profile) = profile else {
        return Ok(None);
    };
    let resolved = profile
        .0
        .iter()
        .map(|(scope, references)| {
            let sets = references
                .iter()
                .map(|reference| {
                    reference
                        .resolve(|name| get_permission_set(name).cloned())
                        .ok_or_else(|| {
                            AlienError::new(ErrorData::InvalidPermissionSet {
                                message: format!(
                                    "Unknown management permission set '{}'",
                                    reference.id()
                                ),
                            })
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((scope, sets))
        })
        .collect::<Result<Vec<_>>>()?;
    let serialized = serde_json::to_vec(&resolved).into_alien_error().context(
        ErrorData::SerializationError {
            message: "Failed to serialize resolved management permissions".to_string(),
        },
    )?;
    Ok(Some(format!("{:x}", Sha256::digest(&serialized))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::PermissionSetReference;

    #[test]
    fn named_grants_match_installed_bodies_and_detect_changed_permissions() {
        let named = PermissionProfile::new()
            .global([PermissionSetReference::from_name("worker/provision")]);
        let installed = get_permission_set("worker/provision").unwrap().clone();
        let mut changed = installed.clone();
        changed.platforms.aws.as_mut().unwrap()[0]
            .grant
            .actions
            .as_mut()
            .unwrap()
            .push("lambda:GetFunctionUrlConfig".to_string());
        let installed_profile =
            PermissionProfile::new().global([PermissionSetReference::from_inline(installed)]);
        let changed_profile =
            PermissionProfile::new().global([PermissionSetReference::from_inline(changed)]);
        let revision = management_permissions_revision(Some(&named)).unwrap();
        assert_eq!(
            revision,
            management_permissions_revision(Some(&installed_profile)).unwrap()
        );
        assert_ne!(
            revision,
            management_permissions_revision(Some(&changed_profile)).unwrap()
        );
        let unknown = PermissionProfile::new().global([PermissionSetReference::from_name(
            "worker/missing-permission",
        )]);
        assert!(management_permissions_revision(Some(&unknown)).is_err());
    }
}
