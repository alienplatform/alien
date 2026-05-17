use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{
    ownership_policy_for_resource_type, ManagementPermissions, PermissionProfile, Platform,
    ResourceLifecycle, Stack,
};

/// Ensures explicit management overrides still grant provision permissions for Live resources.
///
/// Auto and Extend management permissions are derived by mutations. Override means the user is
/// taking full control, so the preflight must fail before deployment if required Live ownership
/// permissions are missing.
pub struct LiveProvisionPermissionsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for LiveProvisionPermissionsCheck {
    fn description(&self) -> &'static str {
        "Live resources require provision permissions when management permissions are overridden"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        matches!(stack.management(), ManagementPermissions::Override(_))
            && stack
                .resources()
                .any(|(_, entry)| entry.lifecycle == ResourceLifecycle::Live)
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let ManagementPermissions::Override(profile) = stack.management() else {
            return Ok(CheckResult::success());
        };

        let mut errors = Vec::new();

        for (resource_id, entry) in stack.resources() {
            if entry.lifecycle != ResourceLifecycle::Live {
                continue;
            }

            let resource_type = entry.config.resource_type();
            let policy = ownership_policy_for_resource_type(resource_type.0.as_ref());
            if !policy.allows_live() {
                continue;
            }

            let required_permission = format!("{}/provision", resource_type.0);
            if !profile_contains_permission(profile, resource_id, &required_permission) {
                errors.push(format!(
                    "Setup required: Live resource '{}' of type '{}' needs management permission '{}'. \
                     The stack overrides management permissions, so Alien cannot derive this automatically. \
                     Add '{}' to the management override for '{}' or '*' and rerun setup with the updated stack.",
                    resource_id,
                    resource_type.0,
                    required_permission,
                    required_permission,
                    resource_id
                ));
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

fn profile_contains_permission(
    profile: &PermissionProfile,
    resource_id: &str,
    permission_id: &str,
) -> bool {
    profile_scope_contains_permission(profile, "*", permission_id)
        || profile_scope_contains_permission(profile, resource_id, permission_id)
}

fn profile_scope_contains_permission(
    profile: &PermissionProfile,
    scope: &str,
    permission_id: &str,
) -> bool {
    profile.0.get(scope).is_some_and(|permissions| {
        permissions
            .iter()
            .any(|permission| permission.id() == permission_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Worker, WorkerCode, PermissionsConfig, Resource, ResourceEntry, ResourceLifecycle,
    };
    use indexmap::IndexMap;

    fn stack_with_management(management: ManagementPermissions) -> Stack {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "api:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        Stack {
            id: "test".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management,
            },
            supported_platforms: None,
        }
    }

    #[tokio::test]
    async fn override_without_live_provision_fails() {
        let stack = stack_with_management(ManagementPermissions::override_(
            PermissionProfile::new().global(["worker/management"]),
        ));

        let result = LiveProvisionPermissionsCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("worker/provision"));
        assert!(result.errors[0].contains("rerun setup"));
    }

    #[tokio::test]
    async fn override_with_resource_scoped_live_provision_succeeds() {
        let stack = stack_with_management(ManagementPermissions::override_(
            PermissionProfile::new().resource("api", ["worker/provision"]),
        ));

        let result = LiveProvisionPermissionsCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn auto_management_succeeds() {
        let stack = stack_with_management(ManagementPermissions::Auto);

        let result = LiveProvisionPermissionsCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(result.success);
    }
}
