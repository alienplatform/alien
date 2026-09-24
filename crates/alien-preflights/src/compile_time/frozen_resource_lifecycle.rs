use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{
    ownership_policy_for_resource_type, Container, Daemon, Platform, ResourceLifecycle, Sandbox,
    SandboxCode, Stack, Storage,
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

            // Two reasons, and the second outlives the first. Only AWS has a runtime sandbox
            // controller, so elsewhere a Live sandbox is emitted nowhere and provisioned by
            // nobody. And a remote grant on Azure or GCP names a parent that must exist when the
            // package applies, which only the package that creates it can do — so publishing a
            // Live sandbox stays impossible on those clouds even once they gain a controller.
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

            // Setup builds a Frozen image before the deployment registers, and only registration
            // tells the registry which customer account to open the base image's repository to.
            if let Some(sandbox) = resource_entry.config.downcast_ref::<Sandbox>() {
                if resource_entry.lifecycle == ResourceLifecycle::Frozen {
                    if sandbox.private_base_image.is_some() {
                        errors.push(format!(
                            "Sandbox '{}' declares a private base image, which requires the Live \
                             lifecycle; a Frozen image is built before the registry can grant the \
                             customer's account access to it",
                            resource_id
                        ));
                    }
                    // Refused on the declaration rather than on the field it becomes: the release
                    // pushes a source build to the project's own repository, so it is private by
                    // the time anything reads it, and by then setup has already tried to pull.
                    if matches!(&sandbox.code, SandboxCode::Source { .. }) {
                        errors.push(format!(
                            "Sandbox '{}' is built from source, which requires the Live \
                             lifecycle; the release pushes it to a private repository the \
                             registry cannot open to a customer account setup has not yet \
                             reported",
                            resource_id
                        ));
                    }
                }
            }

            // Setup-rendered consumers need the complete binding while the package is applied.
            // A Live sandbox has no image ARN/version until its runtime controller finishes.
            // Containers and Daemons are also runtime-provisioned: their controllers wait for
            // dependencies and resolve the completed sandbox binding from controller state.
            for link in alien_core::links_of(&resource_entry.config) {
                let Some(target) = stack.resources.get(link.id()) else {
                    continue;
                };
                if target.config.downcast_ref::<Sandbox>().is_some()
                    && target.lifecycle == ResourceLifecycle::Live
                    && resource_entry.config.downcast_ref::<Container>().is_none()
                    && resource_entry.config.downcast_ref::<Daemon>().is_none()
                {
                    errors.push(format!(
                        "Resource '{}' links sandbox '{}', which uses the Live lifecycle; its \
                         image is built after setup, but this resource requires its binding \
                         during setup. Add the sandbox with `remoteAccess` and reach it through a \
                         remote binding, or give it a base image that needs no build so it can stay \
                         Frozen and be linked",
                        resource_id,
                        link.id()
                    ));
                }
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
        ArtifactRegistry, Build, CapacityGroup, ComputeCluster, Container, ContainerCode, Daemon,
        DaemonCode, Key, ResourceEntry, ResourceLifecycle, ResourceRef, ResourceSpec, Storage,
        Worker, WorkerCode,
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
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
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
        for platform in [
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
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
                .unwrap_or_else(|| {
                    panic!(
                        "the refusal must name why, on {platform:?}: {:?}",
                        result.errors
                    )
                });
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

    /// The release makes a source build private, so the declaration is refused rather than the
    /// field it becomes — by the time that field exists, setup has already tried to pull.
    #[tokio::test]
    async fn source_is_refused_on_a_frozen_sandbox() {
        let with_source = |lifecycle| {
            let mut stack = sandbox_stack(lifecycle);
            let entry = stack.resources.get_mut("agents").expect("sandbox entry");
            let mut sandbox = entry
                .config
                .downcast_ref::<alien_core::Sandbox>()
                .expect("sandbox config")
                .clone();
            sandbox.code = SandboxCode::Source {
                src: "./sandbox".to_string(),
                toolchain: alien_core::ToolchainConfig::Docker {
                    dockerfile: None,
                    target: None,
                    build_args: None,
                },
            };
            entry.config = alien_core::Resource::new(sandbox);
            stack
        };

        let frozen = FrozenResourceLifecycleCheck
            .check(&with_source(ResourceLifecycle::Frozen), Platform::Aws)
            .await
            .expect("the check runs");
        assert!(!frozen.success);
        assert!(
            frozen
                .errors
                .iter()
                .any(|error| error.contains("built from source")),
            "the refusal must name the declaration: {:?}",
            frozen.errors
        );

        let live = FrozenResourceLifecycleCheck
            .check(&with_source(ResourceLifecycle::Live), Platform::Aws)
            .await
            .expect("the check runs");
        assert!(
            live.success,
            "Live is where a source build belongs: {live:?}"
        );
    }

    #[tokio::test]
    async fn a_private_base_image_is_refused_on_a_frozen_sandbox() {
        let with_private_base = |lifecycle| {
            let mut stack = sandbox_stack(lifecycle);
            let entry = stack.resources.get_mut("agents").expect("sandbox entry");
            let mut sandbox = entry
                .config
                .downcast_ref::<alien_core::Sandbox>()
                .expect("sandbox config")
                .clone();
            sandbox.private_base_image = Some(
                "123456789012.dkr.ecr.{region}.amazonaws.com/alien-artifacts-prj_a:v1".to_string(),
            );
            entry.config = alien_core::Resource::new(sandbox);
            stack
        };

        let frozen = FrozenResourceLifecycleCheck
            .check(&with_private_base(ResourceLifecycle::Frozen), Platform::Aws)
            .await
            .expect("the check runs");
        assert!(!frozen.success);
        let message = frozen
            .errors
            .iter()
            .find(|error| error.contains("requires the Live lifecycle"))
            .unwrap_or_else(|| panic!("the refusal must name why: {:?}", frozen.errors));
        assert!(!message.contains("  "), "collapsed indentation: {message}");

        let live = FrozenResourceLifecycleCheck
            .check(&with_private_base(ResourceLifecycle::Live), Platform::Aws)
            .await
            .expect("the check runs");
        assert!(live.success, "{:?}", live.errors);
    }

    /// A Worker's binding to a sandbox carries `imageArn` and `imageVersion`, both required fields
    /// of `AwsSandboxBinding`. A Live sandbox has neither at setup time, so the binding would fail
    /// to deserialize at Worker startup instead of at plan time — the wrong end to discover it.
    #[tokio::test]
    async fn a_setup_rendered_consumer_cannot_link_a_live_sandbox() {
        let mut stack = sandbox_stack(ResourceLifecycle::Live);
        let worker = alien_core::Worker::new("api".to_string())
            .permissions("execution".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "example.com/api:latest".to_string(),
            })
            .link(
                &alien_core::Sandbox::new("agents".to_string())
                    .code(alien_core::SandboxCode::Image {
                        image: "s3://acme-artifacts/agents/bundle.zip".to_string(),
                    })
                    .egress(alien_core::SandboxEgress::Allow)
                    .lifecycle(alien_core::SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
            )
            .build();
        stack.resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let result = FrozenResourceLifecycleCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("the check runs");

        assert!(!result.success, "a link to a Live sandbox must be refused");
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.contains("links sandbox 'agents'")
                    && error.contains("requires its binding during setup")),
            "the refusal must name the link and why: {:?}",
            result.errors
        );
    }

    #[tokio::test]
    async fn runtime_provisioned_consumers_may_link_a_live_sandbox() {
        let linked_sandbox = || {
            alien_core::Sandbox::new("agents".to_string())
                .code(alien_core::SandboxCode::Image {
                    image: "s3://example-artifacts/agents/bundle.zip".to_string(),
                })
                .egress(alien_core::SandboxEgress::Allow)
                .lifecycle(alien_core::SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build()
        };

        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "example.com/api:latest".to_string(),
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
            .permissions("execution".to_string())
            .link(&linked_sandbox())
            .build();
        let daemon = Daemon::new("scheduler".to_string())
            .code(DaemonCode::Image {
                image: "example.com/scheduler:latest".to_string(),
            })
            .permissions("execution".to_string())
            .link(&linked_sandbox())
            .build();

        for (id, resource) in [
            ("api", alien_core::Resource::new(container)),
            ("scheduler", alien_core::Resource::new(daemon)),
        ] {
            let mut stack = sandbox_stack(ResourceLifecycle::Live);
            stack.resources.insert(
                id.to_string(),
                ResourceEntry {
                    config: resource,
                    lifecycle: ResourceLifecycle::Live,
                    dependencies: Vec::new(),
                    remote_access: false,
                    enabled_when: None,
                },
            );

            let result = FrozenResourceLifecycleCheck
                .check(&stack, Platform::Aws)
                .await
                .expect("the check runs");

            assert!(
                result.success,
                "runtime-provisioned consumer '{id}' should resolve the Live Sandbox binding after the dependency is ready: {:?}",
                result.errors
            );
        }
    }

    /// The same link against a Frozen sandbox is exactly what ships today.
    #[tokio::test]
    async fn linking_a_frozen_sandbox_stays_valid() {
        let mut stack = sandbox_stack(ResourceLifecycle::Frozen);
        let worker = alien_core::Worker::new("api".to_string())
            .permissions("execution".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "example.com/api:latest".to_string(),
            })
            .link(
                &alien_core::Sandbox::new("agents".to_string())
                    .code(alien_core::SandboxCode::Image {
                        image: "s3://acme-artifacts/agents/bundle.zip".to_string(),
                    })
                    .egress(alien_core::SandboxEgress::Allow)
                    .lifecycle(alien_core::SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
            )
            .build();
        stack.resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let result = FrozenResourceLifecycleCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("the check runs");
        assert!(
            result.success,
            "a link to a Frozen sandbox is the shipping path: {:?}",
            result.errors
        );
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
            assert!(
                result.success,
                "a Frozen sandbox must stay valid on {platform:?}: {:?}",
                result.errors
            );
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
