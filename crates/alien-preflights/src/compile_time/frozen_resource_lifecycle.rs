use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{ownership_policy_for_resource_type, Platform, Stack};

/// Ensures each resource uses a lifecycle allowed by the ownership policy.
///
/// The policy is intentionally centralized in `alien-core` so preflights,
/// template emitters, importers, and permissions agree on ownership.
pub struct FrozenResourceLifecycleCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for FrozenResourceLifecycleCheck {
    fn description(&self) -> &'static str {
        "Resources must use lifecycles allowed by the ownership policy"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources().next().is_some()
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            let resource_type_value = resource_entry.config.resource_type();
            let resource_type = resource_type_value.0.as_ref();
            let policy = ownership_policy_for_resource_type(resource_type);

            if !policy.allows_lifecycle(resource_entry.lifecycle) {
                errors.push(format!(
                    "Resource '{}' of type '{}' has lifecycle {:?}, but allowed lifecycles are {}",
                    resource_id,
                    resource_type,
                    resource_entry.lifecycle,
                    policy.allowed_lifecycles()
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ArtifactRegistry, Build, CapacityGroup, ComputeCluster, Container, ContainerCode,
        ResourceEntry, ResourceLifecycle, ResourceSpec, Storage, Worker, WorkerCode,
    };
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_frozen_only_resources_succeed_when_frozen() {
        let build = Build::new("test-build".to_string())
            .permissions("test".to_string())
            .build();
        let registry = ArtifactRegistry::new("test-registry".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-build".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(build),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            "test-registry".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(registry),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourceLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_frozen_only_resource_fails_when_live() {
        let build = Build::new("test-build".to_string())
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-build".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(build),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourceLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("allowed lifecycles are Frozen"));
    }

    #[tokio::test]
    async fn test_function_must_be_live() {
        let worker = Worker::new("my-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "my-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourceLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("allowed lifecycles are Live"));
    }

    #[tokio::test]
    async fn test_container_must_be_live() {
        let container = Container::new("my-container".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "my-container".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourceLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("allowed lifecycles are Live"));
    }

    #[tokio::test]
    async fn test_compute_cluster_must_be_frozen() {
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.large".to_string()),
                profile: None,
                min_size: 1,
                max_size: 3,
                nested_virtualization: None,
            })
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let check = FrozenResourceLifecycleCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("allowed lifecycles are Frozen"));
    }

    #[tokio::test]
    async fn test_storage_can_be_frozen_or_live() {
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            let storage = Storage::new(format!("storage-{lifecycle:?}")).build();
            let mut resources = IndexMap::new();
            resources.insert(
                "storage".to_string(),
                ResourceEntry {
                    config: alien_core::Resource::new(storage),
                    lifecycle,
                    dependencies: Vec::new(),
                    remote_access: false,
                },
            );

            let stack = Stack {
                id: "test-stack".to_string(),
                resources,
                permissions: alien_core::permissions::PermissionsConfig::default(),
                supported_platforms: None,
                inputs: vec![],
            };

            let check = FrozenResourceLifecycleCheck;
            let result = check.check(&stack, Platform::Aws).await.unwrap();
            assert!(result.success);
        }
    }
}
