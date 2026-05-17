use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, ResourceLifecycle, Stack, Storage, Worker, WorkerTrigger};

/// Validates trigger edges whose source resource is setup-owned.
///
/// A storage trigger mutates the source storage resource on several providers. When that storage
/// resource is Frozen, setup owns the notification wiring by default, so normal deployment must
/// fail before the worker controller attempts cloud mutations.
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

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (function_id, entry) in stack.resources() {
            let Some(worker) = entry.config.downcast_ref::<Worker>() else {
                continue;
            };

            for trigger in &worker.triggers {
                if let WorkerTrigger::Storage { storage, .. } = trigger {
                    if storage.resource_type != Storage::RESOURCE_TYPE {
                        continue;
                    }

                    let Some(source_entry) = stack.resources.get(storage.id()) else {
                        continue;
                    };

                    if source_entry.lifecycle == ResourceLifecycle::Frozen {
                        errors.push(format!(
                            "Setup required: worker '{}' has a storage trigger from Frozen storage '{}'. \
                             Storage notification wiring is setup-owned by default because it mutates the storage resource. \
                             Rerun setup with the updated stack, or make the storage resource Live and grant storage/provision.",
                            function_id,
                            storage.id()
                        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        PermissionProfile, PermissionsConfig, Resource, ResourceEntry, Storage, WorkerCode,
    };
    use indexmap::IndexMap;

    fn stack_with_storage_lifecycle(storage_lifecycle: ResourceLifecycle) -> Stack {
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
                management: Default::default(),
            },
            supported_platforms: None,
        }
    }

    #[tokio::test]
    async fn frozen_storage_trigger_fails_with_setup_required() {
        let stack = stack_with_storage_lifecycle(ResourceLifecycle::Frozen);

        let result = TriggerEdgeOwnershipCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("Frozen storage 'uploads'"));
        assert!(result.errors[0].contains("Rerun setup"));
    }

    #[tokio::test]
    async fn live_storage_trigger_succeeds() {
        let stack = stack_with_storage_lifecycle(ResourceLifecycle::Live);

        let result = TriggerEdgeOwnershipCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(result.success);
    }
}
