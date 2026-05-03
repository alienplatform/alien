use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack};

/// Ensures that all permission profiles referenced by resources actually exist in the stack's permissions config.
///
/// This check validates that Functions, Containers, and Builds reference existing permission profiles.
/// Running this BEFORE mutations ensures we catch typos and missing profiles early,
/// rather than having them silently created by mutations.
pub struct PermissionProfilesExistCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for PermissionProfilesExistCheck {
    fn description(&self) -> &'static str {
        "All permission profiles referenced by resources must exist in the permissions config"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Run if stack has any resources that might reference permission profiles
        stack.resources().any(|(_, resource_entry)| {
            resource_entry
                .config
                .downcast_ref::<alien_core::Function>()
                .is_some()
                || resource_entry
                    .config
                    .downcast_ref::<alien_core::Container>()
                    .is_some()
                || resource_entry
                    .config
                    .downcast_ref::<alien_core::Build>()
                    .is_some()
        })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let defined_profiles = &stack.permissions.profiles;

        for (resource_id, resource_entry) in stack.resources() {
            // Check Functions
            if let Some(function) = resource_entry.config.downcast_ref::<alien_core::Function>() {
                let profile_name = &function.permissions;
                if !defined_profiles.contains_key(profile_name) {
                    errors.push(format!(
                        "Function '{}' references permission profile '{}' which does not exist. \
                         Available profiles: {}",
                        resource_id,
                        profile_name,
                        if defined_profiles.is_empty() {
                            "(none defined)".to_string()
                        } else {
                            defined_profiles
                                .keys()
                                .map(|k| format!("'{}'", k))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    ));
                }
            }

            // Check Containers
            if let Some(container) = resource_entry
                .config
                .downcast_ref::<alien_core::Container>()
            {
                let profile_name = &container.permissions;
                if !defined_profiles.contains_key(profile_name) {
                    errors.push(format!(
                        "Container '{}' references permission profile '{}' which does not exist. \
                         Available profiles: {}",
                        resource_id,
                        profile_name,
                        if defined_profiles.is_empty() {
                            "(none defined)".to_string()
                        } else {
                            defined_profiles
                                .keys()
                                .map(|k| format!("'{}'", k))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    ));
                }
            }

            // Check Builds
            if let Some(build) = resource_entry.config.downcast_ref::<alien_core::Build>() {
                let profile_name = &build.permissions;
                if !defined_profiles.contains_key(profile_name) {
                    errors.push(format!(
                        "Build '{}' references permission profile '{}' which does not exist. \
                         Available profiles: {}",
                        resource_id,
                        profile_name,
                        if defined_profiles.is_empty() {
                            "(none defined)".to_string()
                        } else {
                            defined_profiles
                                .keys()
                                .map(|k| format!("'{}'", k))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Function, FunctionCode, Ingress, PermissionProfile, PermissionsConfig, ResourceEntry,
        ResourceLifecycle,
    };
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_permission_profile_exists_success() {
        let function = Function::new("api".to_string())
            .code(FunctionCode::Image {
                image: "api:latest".to_string(),
            })
            .permissions("api-profile".to_string())
            .ingress(Ingress::Private)
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        // Define the profile
        let mut profiles = IndexMap::new();
        let mut api_profile = PermissionProfile::new();
        api_profile.0.insert("*".to_string(), vec![]);
        profiles.insert("api-profile".to_string(), api_profile);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: alien_core::permissions::ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let check = PermissionProfilesExistCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_permission_profile_missing_fails() {
        let function = Function::new("api".to_string())
            .code(FunctionCode::Image {
                image: "api:latest".to_string(),
            })
            .permissions("nonexistent".to_string()) // Profile doesn't exist!
            .ingress(Ingress::Private)
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(), // No profiles defined!
                management: alien_core::permissions::ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let check = PermissionProfilesExistCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("nonexistent"));
        assert!(result.errors[0].contains("does not exist"));
    }

    #[tokio::test]
    async fn test_multiple_profiles_some_missing() {
        let api = Function::new("api".to_string())
            .code(FunctionCode::Image {
                image: "api:latest".to_string(),
            })
            .permissions("api-profile".to_string()) // Exists
            .ingress(Ingress::Private)
            .build();

        let worker = Function::new("worker".to_string())
            .code(FunctionCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("worker-profile".to_string()) // Doesn't exist!
            .ingress(Ingress::Private)
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(api),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        // Only define api-profile, not worker-profile
        let mut profiles = IndexMap::new();
        let mut api_profile = PermissionProfile::new();
        api_profile.0.insert("*".to_string(), vec![]);
        profiles.insert("api-profile".to_string(), api_profile);

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: alien_core::permissions::ManagementPermissions::Auto,
            },
            supported_platforms: None,
        };

        let check = PermissionProfilesExistCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("worker"));
        assert!(result.errors[0].contains("worker-profile"));
    }
}
