use crate::error::Result;
use crate::{CheckResult, StackCompatibilityCheck};
use alien_core::Stack;

/// Validates that permission profiles in the stack haven't been modified.
///
/// Permission profiles define the security model of the stack and changing them
/// during updates could lead to security vulnerabilities or privilege escalation.
pub struct PermissionProfilesUnchangedCheck;

#[async_trait::async_trait]
impl StackCompatibilityCheck for PermissionProfilesUnchangedCheck {
    fn description(&self) -> &'static str {
        "Permission profiles in the stack shouldn't be modified"
    }

    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Check if permission profiles have been added, removed, or modified
        let old_profiles = &old_stack.permissions.profiles;
        let new_profiles = &new_stack.permissions.profiles;

        // Check for removed profiles
        for (profile_name, _) in old_profiles {
            if !new_profiles.contains_key(profile_name) {
                errors.push(format!(
                    "Permission profile '{}' was removed from the stack",
                    profile_name
                ));
            }
        }

        // Check for modified or added profiles
        for (profile_name, new_profile) in new_profiles {
            if let Some(old_profile) = old_profiles.get(profile_name) {
                // Profile exists in both - check if it was modified
                if old_profile != new_profile {
                    errors.push(format!(
                        "Permission profile '{}' was modified",
                        profile_name
                    ));
                }
            } else {
                // Profile is new
                warnings.push(format!(
                    "New permission profile '{}' was added",
                    profile_name
                ));
            }
        }

        // Check management permissions
        if old_stack.management() != new_stack.management() {
            errors.push("Management permissions configuration was modified".to_string());
        }

        if errors.is_empty() {
            if warnings.is_empty() {
                Ok(CheckResult::success())
            } else {
                Ok(CheckResult::with_warnings(warnings))
            }
        } else {
            Ok(CheckResult::failed_with_warnings(errors, warnings))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::permissions::{
        ManagementPermissions, PermissionProfile, PermissionSetReference, PermissionsConfig,
    };
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_unchanged_profiles_success() {
        let mut profile = PermissionProfile::new();
        profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("function/execute")],
        );

        let mut profiles = IndexMap::new();
        profiles.insert("test-profile".to_string(), profile.clone());

        let permissions_config = PermissionsConfig {
            profiles: profiles.clone(),
            management: ManagementPermissions::Auto,
        };

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: permissions_config.clone(),
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: permissions_config,
        };

        let check = PermissionProfilesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(result.success);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_modified_profile_failure() {
        let mut old_profile = PermissionProfile::new();
        old_profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("function/execute")],
        );

        let mut new_profile = PermissionProfile::new();
        new_profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("storage/data-read")],
        );

        let mut old_profiles = IndexMap::new();
        old_profiles.insert("test-profile".to_string(), old_profile);

        let mut new_profiles = IndexMap::new();
        new_profiles.insert("test-profile".to_string(), new_profile);

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: old_profiles,
                management: ManagementPermissions::Auto,
            },
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: new_profiles,
                management: ManagementPermissions::Auto,
            },
        };

        let check = PermissionProfilesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("was modified"));
    }

    #[tokio::test]
    async fn test_added_profile_warning() {
        let old_profiles = IndexMap::new();

        let mut new_profile = PermissionProfile::new();
        new_profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("function/execute")],
        );

        let mut new_profiles = IndexMap::new();
        new_profiles.insert("new-profile".to_string(), new_profile);

        let old_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: old_profiles,
                management: ManagementPermissions::Auto,
            },
        };

        let new_stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles: new_profiles,
                management: ManagementPermissions::Auto,
            },
        };

        let check = PermissionProfilesUnchangedCheck;
        let result = check.check(&old_stack, &new_stack).await.unwrap();
        assert!(result.success); // Success but with warnings
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("was added"));
    }
}
