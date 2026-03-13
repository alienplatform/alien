use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{permissions::PermissionSetReference, Platform, ServiceAccount, Stack};

/// Validates that service-account/impersonate permission is used correctly.
///
/// This check enforces the following rules:
/// 1. `service-account/impersonate` must NEVER be used at stack level ("*" scope)
/// 2. `service-account/impersonate` must only be scoped to resources that are ServiceAccounts
///    or will become ServiceAccounts via permission profile conversion
/// 3. Warns if a manually-defined ServiceAccount has impersonate permissions targeting it
///    (impersonation only works for profile-generated ServiceAccounts)
pub struct ServiceAccountImpersonateValidationCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ServiceAccountImpersonateValidationCheck {
    fn description(&self) -> &'static str {
        "Validate service-account/impersonate permission usage"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Run if any permission profile exists (might contain impersonate permissions)
        !stack.permissions.profiles.is_empty()
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Collect all ServiceAccount resource IDs and determine which are manually-defined
        // Also collect profile names that will create ServiceAccounts
        let mut service_account_ids = std::collections::HashSet::new();
        let mut profile_names = std::collections::HashSet::new();
        let mut manually_defined_sa = std::collections::HashSet::new();

        for (resource_id, resource_entry) in stack.resources() {
            if resource_entry
                .config
                .downcast_ref::<ServiceAccount>()
                .is_some()
            {
                service_account_ids.insert(resource_id.clone());

                // Check if this ServiceAccount has a corresponding permission profile
                // Convention: "xyz-sa" corresponds to profile "xyz"
                let potential_profile_name = resource_id.strip_suffix("-sa").unwrap_or(resource_id);

                if !stack
                    .permissions
                    .profiles
                    .contains_key(potential_profile_name)
                {
                    manually_defined_sa.insert(resource_id.clone());
                }
            }
        }

        // ServiceAccounts that WILL be created from profiles
        for profile_name in stack.permissions.profiles.keys() {
            profile_names.insert(profile_name.clone());
            let sa_id = format!("{}-sa", profile_name);
            service_account_ids.insert(sa_id);
        }

        // Check each permission profile
        for (profile_name, permission_profile) in &stack.permissions.profiles {
            // Check for stack-level impersonate (ERROR)
            if let Some(permission_set_refs) = permission_profile.0.get("*") {
                for perm_ref in permission_set_refs {
                    let perm_id = match perm_ref {
                        PermissionSetReference::Name(name) => name.as_str(),
                        PermissionSetReference::Inline(inline) => inline.id.as_str(),
                    };

                    if perm_id == "service-account/impersonate" {
                        errors.push(format!(
                            "Permission profile '{}' uses 'service-account/impersonate' at stack level (\"*\" scope). \
                            Impersonate permission must be resource-scoped to specific ServiceAccounts, not stack-level.",
                            profile_name
                        ));
                    }
                }
            }

            // Check resource-scoped impersonate permissions
            for (resource_scope, permission_set_refs) in &permission_profile.0 {
                // Skip stack-level scope (already checked)
                if resource_scope == "*" {
                    continue;
                }

                for perm_ref in permission_set_refs {
                    let perm_id = match perm_ref {
                        PermissionSetReference::Name(name) => name.as_str(),
                        PermissionSetReference::Inline(inline) => inline.id.as_str(),
                    };

                    if perm_id == "service-account/impersonate" {
                        // Validate that the target resource is either:
                        // 1. A ServiceAccount resource ID (e.g., "agent-management-sa")
                        // 2. A profile name that will create a ServiceAccount (e.g., "agent-management")
                        let is_service_account = service_account_ids.contains(resource_scope);
                        let is_profile_name = profile_names.contains(resource_scope);

                        if !is_service_account && !is_profile_name {
                            errors.push(format!(
                                "Permission profile '{}' uses 'service-account/impersonate' scoped to '{}', \
                                but '{}' is not a ServiceAccount resource or permission profile. \
                                Impersonate permission can only target ServiceAccounts or profiles that create ServiceAccounts.",
                                profile_name, resource_scope, resource_scope
                            ));
                        }
                        // Warn if targeting a manually-defined ServiceAccount
                        else if manually_defined_sa.contains(resource_scope) {
                            warnings.push(format!(
                                "Permission profile '{}' uses 'service-account/impersonate' on manually-defined ServiceAccount '{}'. \
                                Impersonation only works for ServiceAccounts created from permission profiles. \
                                This permission will have no effect.",
                                profile_name, resource_scope
                            ));
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            if warnings.is_empty() {
                Ok(CheckResult::success())
            } else {
                Ok(CheckResult::with_warnings(warnings))
            }
        } else {
            if warnings.is_empty() {
                Ok(CheckResult::failed(errors))
            } else {
                Ok(CheckResult::failed_with_warnings(errors, warnings))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{permissions::PermissionProfile, ResourceEntry, ResourceLifecycle};
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_stack_level_impersonate_fails() {
        let mut profiles = IndexMap::new();
        let profile = PermissionProfile::new().global(vec!["service-account/impersonate"]);
        profiles.insert("execution".to_string(), profile);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: alien_core::permissions::PermissionsConfig {
                profiles,
                management: alien_core::permissions::ManagementPermissions::Auto,
            },
        };

        let check = ServiceAccountImpersonateValidationCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors.iter().any(|e| e.contains("stack level")));
    }

    #[tokio::test]
    async fn test_resource_scoped_to_non_service_account_fails() {
        let mut profiles = IndexMap::new();
        let profile =
            PermissionProfile::new().resource("some-storage", vec!["service-account/impersonate"]);
        profiles.insert("execution".to_string(), profile);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: alien_core::permissions::PermissionsConfig {
                profiles,
                management: alien_core::permissions::ManagementPermissions::Auto,
            },
        };

        let check = ServiceAccountImpersonateValidationCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("not a ServiceAccount")));
    }

    #[tokio::test]
    async fn test_valid_resource_scoped_impersonate_succeeds() {
        let mut profiles = IndexMap::new();

        // Profile "execution" with impersonate permission to "agent-management"
        let execution_profile = PermissionProfile::new()
            .resource("agent-management", vec!["service-account/impersonate"]);
        profiles.insert("execution".to_string(), execution_profile);

        // Profile "agent-management" will create "agent-management-sa"
        let agent_management_profile = PermissionProfile::new();
        profiles.insert("agent-management".to_string(), agent_management_profile);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: alien_core::permissions::PermissionsConfig {
                profiles,
                management: alien_core::permissions::ManagementPermissions::Auto,
            },
        };

        let check = ServiceAccountImpersonateValidationCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_manually_defined_service_account_warns() {
        // Manually create a ServiceAccount without a corresponding profile
        let manual_sa = ServiceAccount::new("manual-sa".to_string()).build();
        let mut resources = IndexMap::new();
        resources.insert(
            "manual-sa".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(manual_sa),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let mut profiles = IndexMap::new();
        let profile =
            PermissionProfile::new().resource("manual-sa", vec!["service-account/impersonate"]);
        profiles.insert("execution".to_string(), profile);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig {
                profiles,
                management: alien_core::permissions::ManagementPermissions::Auto,
            },
        };

        let check = ServiceAccountImpersonateValidationCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success); // Success but with warnings
        assert!(!result.warnings.is_empty());
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("manually-defined")));
    }
}
