//! Authors permission-profile grants for resource links and triggers.
//!
//! Resource links and triggers are dependency edges. They provide default
//! data-access grants only when the consumer's profile has no entry for the
//! resource. Explicit resource grants take precedence over those defaults.

use std::collections::HashSet;

use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::PermissionSetReference;
use alien_core::{
    Build, Container, Daemon, DeploymentConfig, Kv, Queue, ResourceRef, Sandbox, Stack, StackState,
    Storage, Vault, Worker, WorkerTrigger,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Adds concrete resource-scoped permissions for compute/build links and queue triggers.
pub struct ResourceLinkPermissionsMutation;

#[async_trait]
impl StackMutation for ResourceLinkPermissionsMutation {
    fn description(&self) -> &'static str {
        "Add permission-profile grants for resource links and triggers"
    }

    fn should_run(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        stack.resources.values().any(|entry| {
            if let Some(worker) = entry.config.downcast_ref::<Worker>() {
                !worker.links.is_empty() || !worker.triggers.is_empty()
            } else if let Some(container) = entry.config.downcast_ref::<Container>() {
                !container.links.is_empty()
            } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                !daemon.links.is_empty()
            } else if let Some(build) = entry.config.downcast_ref::<Build>() {
                !build.links.is_empty()
            } else {
                false
            }
        })
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding permission-profile grants for resource links and triggers");

        let mut grants = Vec::new();
        for entry in stack.resources.values() {
            if let Some(worker) = entry.config.downcast_ref::<Worker>() {
                collect_link_grants(&mut grants, &worker.permissions, &worker.links);
                collect_worker_trigger_grants(&mut grants, &worker.permissions, &worker.triggers);
            } else if let Some(container) = entry.config.downcast_ref::<Container>() {
                collect_link_grants(&mut grants, &container.permissions, &container.links);
            } else if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                collect_link_grants(&mut grants, &daemon.permissions, &daemon.links);
            } else if let Some(build) = entry.config.downcast_ref::<Build>() {
                collect_link_grants(&mut grants, &build.permissions, &build.links);
            }
        }

        // Capture existing targets before adding any defaults. A profile may have
        // multiple consumers or both a link and a trigger for the same resource.
        let explicit_targets: HashSet<_> = stack
            .permissions
            .profiles
            .iter()
            .flat_map(|(profile_name, profile)| {
                profile
                    .0
                    .keys()
                    .map(|resource_id| (profile_name.clone(), resource_id.clone()))
            })
            .collect();

        let mut grants_added = 0;
        for grant in grants {
            if explicit_targets.contains(&(grant.profile_name.clone(), grant.resource_id.clone())) {
                continue;
            }

            let Some(profile) = stack.permissions.profiles.get_mut(&grant.profile_name) else {
                debug!(
                    profile_name = %grant.profile_name,
                    "Skipping link permission for nonexistent profile"
                );
                continue;
            };

            let permissions = profile.0.entry(grant.resource_id).or_default();
            for permission_set_id in grant.permission_set_ids {
                if permissions
                    .iter()
                    .any(|permission| permission.id() == *permission_set_id)
                {
                    continue;
                }

                permissions.push(PermissionSetReference::from_name(*permission_set_id));
                grants_added += 1;
            }
        }

        info!(
            "Added {} permission-profile grants for resource links and triggers",
            grants_added
        );

        Ok(stack)
    }
}

#[derive(Debug)]
struct ResourcePermissionGrant {
    profile_name: String,
    resource_id: String,
    permission_set_ids: &'static [&'static str],
}

fn collect_link_grants(
    grants: &mut Vec<ResourcePermissionGrant>,
    profile_name: &str,
    links: &[ResourceRef],
) {
    for link in links {
        let Some(permission_set_ids) = permission_sets_for_link(link) else {
            continue;
        };

        grants.push(ResourcePermissionGrant {
            profile_name: profile_name.to_string(),
            resource_id: link.id().to_string(),
            permission_set_ids,
        });
    }
}

fn collect_worker_trigger_grants(
    grants: &mut Vec<ResourcePermissionGrant>,
    profile_name: &str,
    triggers: &[WorkerTrigger],
) {
    for trigger in triggers {
        match trigger {
            WorkerTrigger::Queue { queue } => {
                grants.push(ResourcePermissionGrant {
                    profile_name: profile_name.to_string(),
                    resource_id: queue.id().to_string(),
                    permission_set_ids: &["queue/data-read"],
                });
            }
            WorkerTrigger::Storage { storage, .. } => {
                grants.push(ResourcePermissionGrant {
                    profile_name: profile_name.to_string(),
                    resource_id: storage.id().to_string(),
                    permission_set_ids: &["storage/data-write"],
                });
            }
            WorkerTrigger::Schedule { .. } => {}
        }
    }
}

fn permission_sets_for_link(link: &ResourceRef) -> Option<&'static [&'static str]> {
    if link.resource_type().as_ref() == Storage::RESOURCE_TYPE.as_ref() {
        Some(&["storage/data-write"])
    } else if link.resource_type().as_ref() == Queue::RESOURCE_TYPE.as_ref() {
        Some(&["queue/data-read", "queue/data-write"])
    } else if link.resource_type().as_ref() == Kv::RESOURCE_TYPE.as_ref() {
        Some(&["kv/data-write"])
    } else if link.resource_type().as_ref() == Vault::RESOURCE_TYPE.as_ref() {
        Some(&["vault/data-read", "vault/data-write"])
    } else if link.resource_type().as_ref() == Sandbox::RESOURCE_TYPE.as_ref() {
        // Execute only. Creating and terminating sessions is `sandbox/management`, which an app
        // grants explicitly — a link should not hand out session lifecycle control over a
        // resource that runs untrusted code.
        Some(&["sandbox/execute"])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        permissions::{ManagementPermissions, PermissionProfile, PermissionsConfig},
        ContainerCode, EnvironmentVariablesSnapshot, ExternalBindings, Platform, Resource,
        ResourceEntry, ResourceLifecycle, ResourceSpec, StackSettings, WorkerCode,
    };
    use indexmap::IndexMap;

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn authors_resource_scoped_permissions_for_links_and_triggers() {
        let queue = Queue::new("jobs".to_string()).build();
        let storage = Storage::new("artifacts".to_string()).build();
        let worker = Worker::new("processor".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/processor:latest".to_string(),
            })
            .link(&storage)
            .trigger(WorkerTrigger::queue(&queue))
            .build();
        let mut resources = IndexMap::new();
        resources.insert(
            "jobs".to_string(),
            ResourceEntry {
                config: Resource::new(queue),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "artifacts".to_string(),
            ResourceEntry {
                config: Resource::new(storage),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            worker.id.clone(),
            ResourceEntry {
                config: Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        let mut profiles = IndexMap::new();
        profiles.insert("execution".to_string(), PermissionProfile::new());
        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let mutation = ResourceLinkPermissionsMutation;
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutated = mutation
            .mutate(stack, &stack_state, &config)
            .await
            .expect("mutation should succeed");

        let profile = mutated
            .permissions
            .profiles
            .get("execution")
            .expect("execution profile should exist");
        let storage_permissions = profile
            .0
            .get("artifacts")
            .expect("storage permissions should be scoped to linked storage");
        assert!(storage_permissions
            .iter()
            .any(|permission| permission.id() == "storage/data-write"));
        let queue_permissions = profile
            .0
            .get("jobs")
            .expect("queue permissions should be scoped to trigger queue");
        assert!(queue_permissions
            .iter()
            .any(|permission| permission.id() == "queue/data-read"));
    }

    #[tokio::test]
    async fn explicit_resource_permissions_override_link_defaults() {
        let storage = Storage::new("objects".to_string()).build();
        let queue = Queue::new("messages".to_string()).build();
        let container = |id: &str, link_queue: bool| {
            let builder = Container::new(id.to_string())
                .code(ContainerCode::Image {
                    image: "example.com/app:latest".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.5".to_string(),
                    desired: "1".to_string(),
                })
                .memory(ResourceSpec {
                    min: "512Mi".to_string(),
                    desired: "1Gi".to_string(),
                })
                .permissions(id.to_string())
                .link(&storage);
            if link_queue {
                builder.link(&queue).build()
            } else {
                builder.build()
            }
        };
        let sender = container("sender", true);
        let receiver = container("receiver", true);
        let automatic = container("automatic", true);
        let no_access = container("no-access", false);

        let stack = Stack::new("test-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(queue, ResourceLifecycle::Frozen)
            .add(sender, ResourceLifecycle::Live)
            .add(receiver, ResourceLifecycle::Live)
            .add(automatic, ResourceLifecycle::Live)
            .add(no_access, ResourceLifecycle::Live)
            .permission(
                "sender",
                PermissionProfile::new()
                    .resource("objects", ["storage/data-read"])
                    .resource("messages", ["queue/data-write"]),
            )
            .permission(
                "receiver",
                PermissionProfile::new()
                    .resource("objects", ["storage/data-read", "storage/data-write"])
                    .resource("messages", ["queue/data-read"]),
            )
            .permission("automatic", PermissionProfile::new())
            .permission(
                "no-access",
                PermissionProfile::new().resource("objects", Vec::<&str>::new()),
            )
            .build();

        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutated = ResourceLinkPermissionsMutation
            .mutate(stack, &StackState::new(Platform::Aws), &config)
            .await
            .expect("mutation should succeed");

        let permission_ids = |profile_name: &str, resource_id: &str| {
            mutated.permissions.profiles[profile_name].0[resource_id]
                .iter()
                .map(|permission| permission.id().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(permission_ids("sender", "objects"), ["storage/data-read"]);
        assert_eq!(permission_ids("sender", "messages"), ["queue/data-write"]);
        assert_eq!(
            permission_ids("receiver", "objects"),
            ["storage/data-read", "storage/data-write"]
        );
        assert_eq!(permission_ids("receiver", "messages"), ["queue/data-read"]);
        assert_eq!(
            permission_ids("automatic", "objects"),
            ["storage/data-write"]
        );
        assert_eq!(
            permission_ids("automatic", "messages"),
            ["queue/data-read", "queue/data-write"]
        );
        assert!(permission_ids("no-access", "objects").is_empty());

        for consumer in ["sender", "receiver", "automatic", "no-access"] {
            let container = mutated.resources[consumer]
                .config
                .downcast_ref::<Container>()
                .expect("linked resource should remain a container");
            assert!(container.links.iter().any(|link| link.id() == "objects"));
            if consumer != "no-access" {
                assert!(container.links.iter().any(|link| link.id() == "messages"));
            }
        }
    }
}
