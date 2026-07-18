//! Authors permission-profile grants for resource links and triggers.
//!
//! Resource links and triggers are dependency edges, not implicit data-access grants.
//! This mutation makes the required data permissions explicit in the consumer's
//! permission profile so setup emitters and runtime controllers consume one
//! permission source of truth.

use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::PermissionSetReference;
use alien_core::{
    Build, Container, Daemon, DeploymentConfig, Kv, Queue, ResourceRef, Stack, StackState, Storage,
    Vault, Worker, WorkerTrigger,
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

        let mut grants_added = 0;
        for grant in grants {
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
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        permissions::{ManagementPermissions, PermissionProfile, PermissionsConfig},
        EnvironmentVariablesSnapshot, ExternalBindings, Platform, Resource, ResourceEntry,
        ResourceLifecycle, StackSettings, WorkerCode,
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
            },
        );
        resources.insert(
            "artifacts".to_string(),
            ResourceEntry {
                config: Resource::new(storage),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        resources.insert(
            worker.id.clone(),
            ResourceEntry {
                config: Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
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
                gates: Vec::new(),
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
}
