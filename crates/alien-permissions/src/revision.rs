use alien_core::{PermissionProfile, Platform};
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

/// Per-scope grant receipts let runtime remove grants without requiring setup,
/// while additions and changed definitions still require installer action.
pub fn management_grant_revisions(
    profile: Option<&PermissionProfile>,
    platform: Platform,
) -> Result<Option<std::collections::BTreeMap<String, Vec<String>>>> {
    let Some(profile) = profile else {
        return Ok(None);
    };
    let mut scopes = std::collections::BTreeMap::new();
    for (scope, references) in &profile.0 {
        let mut revisions = Vec::new();
        for reference in references {
            let set = reference
                .resolve(|name| get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InvalidPermissionSet {
                        message: format!("Unknown management permission set '{}'", reference.id()),
                    })
                })?;
            let native = match platform {
                Platform::Aws => set.platforms.aws.as_ref().map(serde_json::to_value),
                Platform::Gcp => set.platforms.gcp.as_ref().map(serde_json::to_value),
                Platform::Azure => set.platforms.azure.as_ref().map(serde_json::to_value),
                _ => None,
            }
            .transpose()
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Failed to serialize installed management grants".to_string(),
            })?;
            let Some(native) = native else {
                continue;
            };
            let serialized = serde_json::to_vec(&(set.id, native))
                .into_alien_error()
                .context(ErrorData::SerializationError {
                    message: "Failed to serialize an installed management grant revision"
                        .to_string(),
                })?;
            revisions.push(format!("{:x}", Sha256::digest(&serialized)));
        }
        revisions.sort();
        revisions.dedup();
        if !revisions.is_empty() {
            scopes.insert(scope.clone(), revisions);
        }
    }
    Ok(Some(scopes))
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
        assert_eq!(
            management_grant_revisions(Some(&named), Platform::Aws).unwrap(),
            management_grant_revisions(Some(&installed_profile), Platform::Aws).unwrap(),
        );
        assert_ne!(
            management_grant_revisions(Some(&installed_profile), Platform::Aws).unwrap(),
            management_grant_revisions(Some(&changed_profile), Platform::Aws).unwrap(),
        );
        assert_eq!(
            management_grant_revisions(Some(&installed_profile), Platform::Gcp).unwrap(),
            management_grant_revisions(Some(&changed_profile), Platform::Gcp).unwrap(),
            "AWS-only permission changes must not invalidate GCP setup",
        );
        let unknown = PermissionProfile::new().global([PermissionSetReference::from_name(
            "worker/missing-permission",
        )]);
        assert!(management_permissions_revision(Some(&unknown)).is_err());
    }
}
