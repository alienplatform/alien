use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{
    ManagementPermissions, PermissionProfile, Platform, ResourceLifecycle, Stack, Storage, Worker,
    WorkerTrigger,
};

/// Validates trigger edges whose source resource is setup-owned.
///
/// A storage trigger can mutate the source storage resource on several providers. When the storage
/// resource is Frozen, Alien may attach trigger wiring only through an explicit management grant on
/// that source resource. Auto/Extend management can derive that grant; Override must author it.
pub struct TriggerEdgeOwnershipCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for TriggerEdgeOwnershipCheck {
    fn description(&self) -> &'static str {
        "Trigger edges must be owned by the actor that mutates every touched resource"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Worker>()
                .is_some_and(|worker| !worker.triggers.is_empty())
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        if !storage_trigger_source_requires_management(platform) {
            return Ok(CheckResult::success());
        }

        let mut errors = Vec::new();

        let ManagementPermissions::Override(management_profile) = stack.management() else {
            return Ok(CheckResult::success());
        };

        for (function_id, entry) in stack.resources() {
            let Some(worker) = entry.config.downcast_ref::<Worker>() else {
                continue;
            };

            if entry.lifecycle != ResourceLifecycle::Live {
                continue;
            }

            for trigger in &worker.triggers {
                if let WorkerTrigger::Storage { storage, .. } = trigger {
                    if storage.resource_type != Storage::RESOURCE_TYPE {
                        continue;
                    }

                    let Some(source_entry) = stack.resources.get(storage.id()) else {
                        continue;
                    };

                    if source_entry.lifecycle == ResourceLifecycle::Frozen {
                        let source_id = storage.id();
                        if !profile_contains_permission(
                            management_profile,
                            source_id,
                            "storage/trigger-management",
                        ) {
                            errors.push(format!(
                                "Setup required: worker '{}' has a storage trigger from Frozen storage '{}'. \
                                 Storage trigger wiring mutates the source storage resource on this platform. \
                                 The stack overrides management permissions, so Alien cannot derive this automatically. \
                                 Add 'storage/trigger-management' to the management override for '{}' or '*' and rerun setup.",
                                function_id, source_id, source_id
                            ));
                        }
                    }
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

fn storage_trigger_source_requires_management(platform: Platform) -> bool {
    matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
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
        PermissionProfile, PermissionsConfig, Resource, ResourceEntry, Storage, WorkerCode,
    };
    use indexmap::IndexMap;

    fn stack_with_storage_lifecycle(
        storage_lifecycle: ResourceLifecycle,
        management: ManagementPermissions,
    ) -> Stack {
        let storage = Storage::new("uploads".to_string()).build();
        let worker = Worker::new("processor".to_string())
            .code(WorkerCode::Image {
                image: "processor:latest".to_string(),
            })
            .permissions("execution".to_string())
            .trigger(WorkerTrigger::storage(
                &storage,
                vec!["object-created".to_string()],
            ))
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "uploads".to_string(),
            ResourceEntry {
                config: Resource::new(storage),
                lifecycle: storage_lifecycle,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "processor".to_string(),
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
                profiles: IndexMap::from([(
                    "execution".to_string(),
                    PermissionProfile::new().global(Vec::<&str>::new()),
                )]),
                management,
            },
            supported_platforms: None,
            inputs: vec![],
        }
    }

    #[tokio::test]
    async fn frozen_storage_trigger_with_auto_management_succeeds() {
        let stack =
            stack_with_storage_lifecycle(ResourceLifecycle::Frozen, ManagementPermissions::Auto);

        let result = TriggerEdgeOwnershipCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn frozen_storage_trigger_with_override_without_trigger_management_fails() {
        let stack = stack_with_storage_lifecycle(
            ResourceLifecycle::Frozen,
            ManagementPermissions::override_(PermissionProfile::new().global(["worker/provision"])),
        );

        let result = TriggerEdgeOwnershipCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("Frozen storage 'uploads'"));
        assert!(result.errors[0].contains("storage/trigger-management"));
    }

    #[tokio::test]
    async fn frozen_storage_trigger_with_override_trigger_management_succeeds() {
        let stack = stack_with_storage_lifecycle(
            ResourceLifecycle::Frozen,
            ManagementPermissions::override_(
                PermissionProfile::new().resource("uploads", ["storage/trigger-management"]),
            ),
        );

        let result = TriggerEdgeOwnershipCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn azure_storage_trigger_requires_source_storage_management() {
        let stack = stack_with_storage_lifecycle(
            ResourceLifecycle::Frozen,
            ManagementPermissions::override_(PermissionProfile::new().global(["worker/provision"])),
        );

        let result = TriggerEdgeOwnershipCheck
            .check(&stack, Platform::Azure)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("storage/trigger-management"));
    }

    #[tokio::test]
    async fn live_storage_trigger_succeeds() {
        let stack = stack_with_storage_lifecycle(
            ResourceLifecycle::Live,
            ManagementPermissions::override_(PermissionProfile::new().global(["worker/provision"])),
        );

        let result = TriggerEdgeOwnershipCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(result.success);
    }
}
