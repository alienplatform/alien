use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{
    ownership_policy_for_resource_type, Platform, ResourceLifecycle, Sandbox, Stack, Storage,
};

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

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let encrypted_azure_storage_count = stack
            .resources()
            .filter_map(|(_, entry)| entry.config.downcast_ref::<Storage>())
            .filter(|storage| storage.encryption_key.is_some())
            .count();
        let azure_storage_count = stack
            .resources()
            .filter(|(_, entry)| entry.config.downcast_ref::<Storage>().is_some())
            .count();

        if platform == Platform::Azure
            && encrypted_azure_storage_count > 0
            && (encrypted_azure_storage_count != 1 || azure_storage_count != 1)
        {
            errors.push(
                "Azure Storage customer-managed encryption is account-wide; a stack using \
                 Storage.encryptionKey() must contain exactly one Storage resource"
                    .to_string(),
            );
        }

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

            // Only AWS has a runtime sandbox controller that builds the image itself. On the
            // other backends a Live sandbox would be accepted, emitted nowhere, and provisioned
            // by nobody.
            if resource_entry.config.downcast_ref::<Sandbox>().is_some()
                && resource_entry.lifecycle == ResourceLifecycle::Live
                && platform != Platform::Aws
            {
                errors.push(format!(
                    "Sandbox '{}' uses the Live lifecycle, which platform '{}' does not \
                     support; only AWS provisions a sandbox at runtime",
                    resource_id,
                    platform.as_str()
                ));
            }

            let Some(storage) = resource_entry.config.downcast_ref::<Storage>() else {
                continue;
            };
            let Some(key_ref) = &storage.encryption_key else {
                continue;
            };
            if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
                errors.push(format!(
                    "Storage '{}' uses encryptionKey, which is not supported on platform '{}'",
                    resource_id,
                    platform.as_str()
                ));
            }
            if resource_entry.lifecycle != ResourceLifecycle::Frozen {
                errors.push(format!(
                    "Storage '{}' uses encryptionKey and must use the Frozen lifecycle",
                    resource_id
                ));
            }
            if let Some(key_entry) = stack.resources.get(&key_ref.id) {
                if key_entry.lifecycle != ResourceLifecycle::Frozen {
                    errors.push(format!(
                        "Storage '{}' encryption Key '{}' must use the Frozen lifecycle",
                        resource_id, key_ref.id
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
        ArtifactRegistry, Build, CapacityGroup, ComputeCluster, Container, ContainerCode, Key,
        ResourceEntry, ResourceLifecycle, ResourceRef, ResourceSpec, Storage, Worker, WorkerCode,
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
                enabled_when: None,
            },
        );
        resources.insert(
            "test-registry".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(registry),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
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
                enabled_when: None,
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
                enabled_when: None,
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
                enabled_when: None,
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
                scale_policy: None,
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
                enabled_when: None,
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
                    enabled_when: None,
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

    fn sandbox_stack(lifecycle: ResourceLifecycle) -> Stack {
        let sandbox = alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: "s3://acme-artifacts/agents/bundle.zip".to_string(),
            })
            .egress(alien_core::SandboxEgress::Allow)
            .session(alien_core::SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build();
        let mut resources = IndexMap::new();
        resources.insert(
            "agents".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(sandbox),
                lifecycle,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        }
    }

    /// Only AWS has a runtime controller that builds a sandbox image, so only AWS may declare one
    /// Live. Elsewhere the resource would pass the ownership policy, emit no image, and wait on a
    /// controller that does not exist — a deployment that hangs rather than one that fails.
    #[tokio::test]
    async fn a_live_sandbox_is_refused_on_every_platform_but_aws() {
        for platform in [Platform::Gcp, Platform::Azure, Platform::Kubernetes, Platform::Local] {
            let result = FrozenResourceLifecycleCheck
                .check(&sandbox_stack(ResourceLifecycle::Live), platform)
                .await
                .expect("the check runs");

            assert!(
                !result.success,
                "a Live sandbox must be refused on {platform:?}"
            );
            let message = result
                .errors
                .iter()
                .find(|error| error.contains("only AWS provisions a sandbox at runtime"))
                .unwrap_or_else(|| panic!("the refusal must name why, on {platform:?}: {:?}", result.errors));
            assert!(
                message.contains(platform.as_str()),
                "the refusal must name the platform it applies to: {message}"
            );
            // The message reaches a user, and a Rust line continuation that loses its backslash
            // silently pads it with the source file's indentation.
            assert!(
                !message.contains("  "),
                "the refusal must not carry collapsed indentation: {message}"
            );
        }
    }

    /// AWS accepts both, and every other platform keeps the Frozen sandbox it has today.
    #[tokio::test]
    async fn a_sandbox_is_accepted_frozen_everywhere_and_live_on_aws() {
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            let result = FrozenResourceLifecycleCheck
                .check(&sandbox_stack(ResourceLifecycle::Frozen), platform)
                .await
                .expect("the check runs");
            assert!(result.success, "a Frozen sandbox must stay valid on {platform:?}: {:?}", result.errors);
        }

        let result = FrozenResourceLifecycleCheck
            .check(&sandbox_stack(ResourceLifecycle::Live), Platform::Aws)
            .await
            .expect("the check runs");
        assert!(
            result.success,
            "AWS is the platform that provisions a sandbox at runtime: {:?}",
            result.errors
        );
    }

    fn stack_with_encrypted_storage(storage_lifecycle: ResourceLifecycle) -> Stack {
        let key = Key::new("customer-key".to_string()).build();
        let storage = Storage::new("customer-data".to_string())
            .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "customer-key"))
            .build();
        let mut resources = IndexMap::new();
        resources.insert(
            "customer-key".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(key),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "customer-data".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: storage_lifecycle,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        }
    }

    #[tokio::test]
    async fn encrypted_storage_must_be_frozen() {
        let result = FrozenResourceLifecycleCheck
            .check(
                &stack_with_encrypted_storage(ResourceLifecycle::Live),
                Platform::Aws,
            )
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("uses encryptionKey and must use the Frozen lifecycle")));
    }

    #[tokio::test]
    async fn encrypted_storage_rejects_unsupported_platforms() {
        let result = FrozenResourceLifecycleCheck
            .check(
                &stack_with_encrypted_storage(ResourceLifecycle::Frozen),
                Platform::Kubernetes,
            )
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("not supported on platform 'kubernetes'")));
    }

    #[tokio::test]
    async fn azure_encrypted_storage_rejects_a_shared_storage_account() {
        let mut stack = stack_with_encrypted_storage(ResourceLifecycle::Frozen);
        stack.resources.insert(
            "other-data".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(Storage::new("other-data".to_string()).build()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let result = FrozenResourceLifecycleCheck
            .check(&stack, Platform::Azure)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("must contain exactly one Storage resource")));
    }
}
