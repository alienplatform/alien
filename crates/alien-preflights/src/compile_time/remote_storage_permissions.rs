use crate::error::Result;
use crate::remote_storage::{resource_ids, REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID};
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{ManagementPermissions, Platform, Stack};

/// Ensures explicit management overrides opt into data access for every remote
/// Storage resource. Unlike provisioning permissions, this grant must be on the
/// concrete resource; wildcard object-data access is intentionally rejected.
pub struct RemoteStoragePermissionsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for RemoteStoragePermissionsCheck {
    fn description(&self) -> &'static str {
        "Remote Storage requires a resource-scoped data permission when management permissions are overridden"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        matches!(stack.management(), ManagementPermissions::Override(_))
            && !resource_ids(stack, platform).is_empty()
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let ManagementPermissions::Override(profile) = stack.management() else {
            return Ok(CheckResult::success());
        };

        let errors = resource_ids(stack, platform)
            .into_iter()
            .filter(|resource_id| {
                !profile.0.get(resource_id).is_some_and(|permissions| {
                    permissions.iter().any(|permission| {
                        permission.id() == REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID
                    })
                })
            })
            .map(|resource_id| {
                format!(
                    "Setup required: remote Storage resource '{resource_id}' needs management permission \
                     '{REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID}' in its exact resource scope. \
                     The stack overrides management permissions, so Alien cannot derive this data-access grant. \
                     Add it under scope '{resource_id}' and rerun setup."
                )
            })
            .collect::<Vec<_>>();

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
    use alien_core::permissions::{PermissionProfile, PermissionSetReference};
    use alien_core::{PermissionsConfig, Resource, ResourceEntry, ResourceLifecycle, Storage};
    use indexmap::IndexMap;

    fn stack_with_override(profile: PermissionProfile, remote_access: bool) -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            "uploads".to_string(),
            ResourceEntry {
                config: Resource::new(Storage::new("uploads".to_string()).build()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access,
                enabled_when: None,
            },
        );
        Stack {
            id: "test".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Override(profile),
            },
            supported_platforms: None,
            inputs: Vec::new(),
        }
    }

    #[tokio::test]
    async fn override_requires_exact_resource_scope() {
        for profile in [
            PermissionProfile::new(),
            PermissionProfile::new().global([REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID]),
        ] {
            let result = RemoteStoragePermissionsCheck
                .check(&stack_with_override(profile, true), Platform::Aws)
                .await
                .unwrap();
            assert!(!result.success);
        }

        let mut profile = PermissionProfile::new();
        profile.0.insert(
            "uploads".to_string(),
            vec![PermissionSetReference::from_name(
                REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID,
            )],
        );
        let result = RemoteStoragePermissionsCheck
            .check(&stack_with_override(profile, true), Platform::Aws)
            .await
            .unwrap();
        assert!(result.success);
    }

    #[test]
    fn skips_ineligible_resources_and_platforms() {
        let stack = stack_with_override(PermissionProfile::new(), false);
        assert!(!RemoteStoragePermissionsCheck.should_run(&stack, Platform::Aws));

        let stack = stack_with_override(PermissionProfile::new(), true);
        assert!(!RemoteStoragePermissionsCheck.should_run(&stack, Platform::Local));
    }
}
