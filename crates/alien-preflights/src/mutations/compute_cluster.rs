//! ComputeCluster mutation that auto-generates ComputeCluster resources for Container workloads.
//!
//! When containers are defined in the stack without an explicit ComputeCluster,
//! this mutation creates a default cluster to host them. It analyzes container
//! resource requirements and selects an appropriate instance type and machine profile
//! using the instance catalog.

use crate::error::Result;
use crate::{
    compile_time::{permission_sets_exist::node_permissions_apply, PermissionSetsExistCheck},
    CompileTimeCheck, StackMutation,
};
use alien_core::{
    compute_planner::{
        capacity_group_requirements, check_pool_capacity, default_persistent_failure_domains,
        generated_pool_scale_policy, plan_compute_with_state, validate_compute_pool_selection,
    },
    instance_catalog::{self, WorkloadRequirements},
    CapacityGroup, CapacityGroupScalePolicy, ComputeCluster, ComputePoolSelection, ComputeSettings,
    Container, Daemon, DeploymentConfig, MachineProfile, Network, PermissionSetReference, Platform,
    ResourceEntry, ResourceLifecycle, ResourceRef, Stack, StackState,
};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tracing::{debug, info};

/// Mutation that auto-generates ComputeCluster resources for Container workloads.
///
/// This ensures every Container has a cluster to run on. If the stack contains
/// Container resources but no ComputeCluster, a default cluster is created
/// with a "general" capacity group whose instance type and machine profile are
/// computed from the containers' resource requirements.
pub struct ComputeClusterMutation;

#[async_trait]
impl StackMutation for ComputeClusterMutation {
    fn description(&self) -> &'static str {
        "Auto-generate ComputeCluster resources for Container workloads"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        if stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<ComputeCluster>()
                .is_some_and(|cluster| {
                    cluster.node_permissions.is_some()
                        || cluster.node_permissions_platforms.is_some()
                })
        }) {
            return true;
        }
        if stack_state.platform == Platform::Kubernetes {
            return false;
        }

        if matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) && stack.resources.values().any(|entry| {
            entry.config.downcast_ref::<Container>().is_some()
                || entry.config.downcast_ref::<ComputeCluster>().is_some()
        }) && config
            .stack_settings
            .compute
            .as_ref()
            .is_some_and(|settings| {
                !settings.containers.is_empty()
                    || settings
                        .pools
                        .values()
                        .any(|selection| selection.machine().is_none())
            })
        {
            // Resource updates must recompute automatic machine choices even
            // when the previously prepared cluster already has a machine type.
            return true;
        }

        let has_containers = stack
            .resources
            .values()
            .any(|entry| entry.config.resource_type().as_ref() == "container");
        // Also fire for daemons that reference a managed compute cluster on cloud
        // platforms. Local daemons ignore `.cluster(...)`, so a daemon-only
        // local stack must not grow a Docker-backed ComputeCluster during
        // deployment preflights.
        let daemon_cluster_ids =
            referenced_daemon_clusters_for_platform(stack, stack_state.platform);
        let has_daemon_cluster_ref = !daemon_cluster_ids.is_empty();
        if !has_containers && !has_daemon_cluster_ref {
            return false;
        }

        let has_cluster = stack
            .resources
            .values()
            .any(|entry| entry.config.resource_type().as_ref() == "compute-cluster");

        if !has_cluster {
            return true;
        }

        if matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) && stack.resources.values().any(|entry| {
            let Some(container) = entry.config.downcast_ref::<Container>() else {
                return false;
            };
            if !requires_persisted_pool(container) {
                return false;
            }
            if container.pool.is_none() && persisted_without_pool(stack_state, container) {
                return false;
            }
            let (Some(cluster_id), Some(pool)) = (&container.cluster, &container.pool) else {
                return true;
            };
            !stack
                .resources
                .get(cluster_id)
                .and_then(|entry| entry.config.downcast_ref::<ComputeCluster>())
                .is_some_and(|cluster| {
                    cluster
                        .capacity_groups
                        .iter()
                        .any(|group| group.group_id == *pool)
                })
        }) {
            // Stateful persistent workloads need a concrete group before runtime so
            // ordinal volumes and replicas use the same placement boundary.
            return true;
        }

        // Cluster exists — check if new containers need a missing capacity group.
        if !matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
            return false;
        }
        if let Some(cluster_entry) = stack
            .resources
            .values()
            .find(|e| e.config.resource_type().as_ref() == "compute-cluster")
        {
            if let Some(cluster) = cluster_entry.config.downcast_ref::<ComputeCluster>() {
                // Cloud capacity groups must be materialized from the selected
                // deployment compute settings before controllers see the
                // prepared stack.
                if cluster
                    .capacity_groups
                    .iter()
                    .any(|g| group_needs_materialization(g, stack_state.platform, config))
                {
                    return true;
                }
                let existing: Vec<&str> = cluster
                    .capacity_groups
                    .iter()
                    .map(|g| g.group_id.as_str())
                    .collect();
                for entry in stack.resources.values() {
                    if let Some(container) = entry.config.downcast_ref::<Container>() {
                        if container.pool.is_some() {
                            continue;
                        }
                        if !existing.contains(&needed_capacity_group(container)) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    async fn mutate(
        &self,
        stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        let mut resolved_config = config.clone();
        if matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
            let installed = installed_pool_selections(
                &stack,
                stack_state,
                config.stack_settings.compute.as_ref(),
            );
            let mut planning_settings = config.stack_settings.compute.clone();
            if !installed.is_empty() {
                planning_settings
                    .get_or_insert_default()
                    .pools
                    .extend(installed.clone());
            }
            let plan = plan_compute_with_state(
                &stack,
                stack_state.platform,
                planning_settings.as_ref(),
                Some(stack_state),
            )
            .context(crate::error::ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: "Could not resolve deployment compute choices".to_string(),
                resource_id: None,
            })?;
            let settings = resolved_config
                .stack_settings
                .compute
                .get_or_insert_default();
            for pool in plan.pools {
                if !pool.errors.is_empty() {
                    if let Some(kept) = installed.get(&pool.pool_id) {
                        return Err(AlienError::new(crate::error::ErrorData::SetupRequired {
                            message: format!(
                                "Select compute for {} capacity group '{}' in the installation setup. The deployment has no compute choice for it, and the installed {} x {} no longer fits this release: {}",
                                stack_state.platform,
                                pool.pool_id,
                                kept.max_size(),
                                kept.machine().unwrap_or_default(),
                                pool.errors.join("; "),
                            ),
                        }));
                    }
                    return Err(AlienError::new(
                        crate::error::ErrorData::StackMutationFailed {
                            mutation_name: self.description().to_string(),
                            message: pool.errors.join("; "),
                            resource_id: None,
                        },
                    ));
                }
                let mut selection = pool.selected;
                // Resolving capacity must not turn an omitted topology choice into
                // an explicit default. Existing pools retain their installed zones.
                let domains = config
                    .stack_settings
                    .compute
                    .as_ref()
                    .and_then(|settings| settings.pools.get(&pool.pool_id))
                    .and_then(ComputePoolSelection::failure_domains)
                    .cloned();
                match &mut selection {
                    ComputePoolSelection::Fixed { failure_domains, .. }
                    | ComputePoolSelection::Autoscale { failure_domains, .. } => {
                        *failure_domains = domains;
                    }
                }
                settings.pools.insert(pool.pool_id, selection);
            }
        }
        // Resolve only for this mutation. Persisted settings retain the absent
        // machine so the next resource update is planned automatically too.
        let config = &resolved_config;
        let stack = self
            .materialize_node_permissions(stack, stack_state.platform)
            .await?;
        let has_cluster = stack
            .resources
            .values()
            .any(|entry| entry.config.resource_type().as_ref() == "compute-cluster");

        let stack = if !has_cluster {
            self.create_cluster(stack, stack_state, config).await?
        } else {
            self.add_missing_capacity_groups(stack, stack_state, config)
                .await?
        };

        let stack = self.materialize_capacity_groups(stack, stack_state, config)?;
        self.materialize_persistent_container_pools(stack, stack_state)
    }
}

impl ComputeClusterMutation {
    async fn materialize_node_permissions(
        &self,
        mut stack: Stack,
        platform: Platform,
    ) -> Result<Stack> {
        if !stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<ComputeCluster>()
                .is_some_and(|cluster| {
                    cluster.node_permissions.is_some()
                        || cluster.node_permissions_platforms.is_some()
                })
        }) {
            return Ok(stack);
        }
        // Project declarations before target validation or dependency materialization.
        for entry in stack.resources.values_mut() {
            let Some(cluster) = entry.config.downcast_mut::<ComputeCluster>() else {
                continue;
            };
            let applies = node_permissions_apply(cluster, platform).map_err(|message| {
                AlienError::new(crate::error::ErrorData::StackMutationFailed {
                    mutation_name: self.description().to_string(),
                    message,
                    resource_id: Some(cluster.id.clone()),
                })
            })?;
            if !applies {
                cluster.node_permissions = None;
            }
            cluster.node_permissions_platforms = None;
        }
        // Use the same concrete-target and platform validation as compilation.
        let validation = PermissionSetsExistCheck.check(&stack, platform).await?;
        if !validation.success {
            return Err(AlienError::new(
                crate::error::ErrorData::StackMutationFailed {
                    mutation_name: self.description().to_string(),
                    message: validation.errors.join("; "),
                    resource_id: None,
                },
            ));
        }
        let target_refs: std::collections::HashMap<_, _> = stack
            .resources
            .iter()
            .map(|(id, entry)| {
                (
                    id.clone(),
                    ResourceRef::new(entry.config.resource_type(), id.clone()),
                )
            })
            .collect();
        for entry in stack.resources.values_mut() {
            let Some(cluster) = entry.config.downcast_mut::<ComputeCluster>() else {
                continue;
            };
            let Some(profile) = &mut cluster.node_permissions else {
                continue;
            };
            for (target_id, references) in &mut profile.0 {
                let target = target_refs.get(target_id).ok_or_else(|| {
                    AlienError::new(crate::error::ErrorData::StackMutationFailed {
                        mutation_name: self.description().to_string(),
                        message: format!("Node permission target '{target_id}' does not exist"),
                        resource_id: Some(cluster.id.clone()),
                    })
                })?;
                // Node grants are applied by the cluster after the target is ready.
                // The runner validates the complete graph after dependency wiring.
                if !entry.dependencies.contains(target) {
                    entry.dependencies.push(target.clone());
                }
                for reference in references {
                    let set = reference
                        .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                        .ok_or_else(|| {
                            AlienError::new(crate::error::ErrorData::StackMutationFailed {
                                mutation_name: self.description().to_string(),
                                message: format!(
                                    "Unknown node permission set '{}'",
                                    reference.id()
                                ),
                                resource_id: Some(cluster.id.clone()),
                            })
                        })?;
                    *reference = PermissionSetReference::Inline(set);
                }
            }
        }
        Ok(stack)
    }

    fn materialize_persistent_container_pools(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
    ) -> Result<Stack> {
        if !matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
            return Ok(stack);
        }

        let cluster_groups: std::collections::HashMap<String, std::collections::HashSet<String>> =
            stack
                .resources
                .iter()
                .filter_map(|(id, entry)| {
                    let cluster = entry.config.downcast_ref::<ComputeCluster>()?;
                    Some((
                        id.clone(),
                        cluster
                            .capacity_groups
                            .iter()
                            .map(|group| group.group_id.clone())
                            .collect(),
                    ))
                })
                .collect();

        for (container_id, entry) in &mut stack.resources {
            let Some(container) = entry.config.downcast_mut::<Container>() else {
                continue;
            };
            if !requires_persisted_pool(container)
                || (container.pool.is_none() && persisted_without_pool(stack_state, container))
            {
                continue;
            }
            let cluster_id = container.cluster.as_ref().ok_or_else(|| {
                AlienError::new(crate::error::ErrorData::StackMutationFailed {
                    mutation_name: "ComputeClusterMutation".to_string(),
                    message: "Stateful persistent container has no compute cluster".to_string(),
                    resource_id: Some(container_id.clone()),
                })
            })?;
            let groups = cluster_groups.get(cluster_id).ok_or_else(|| {
                AlienError::new(crate::error::ErrorData::StackMutationFailed {
                    mutation_name: "ComputeClusterMutation".to_string(),
                    message: format!("Referenced compute cluster '{cluster_id}' does not exist"),
                    resource_id: Some(container_id.clone()),
                })
            })?;
            let inferred_pool = needed_capacity_group(container).to_string();
            let pool = container.pool.get_or_insert(inferred_pool);
            if !groups.contains(pool) {
                return Err(AlienError::new(
                    crate::error::ErrorData::StackMutationFailed {
                        mutation_name: "ComputeClusterMutation".to_string(),
                        message: format!(
                            "Capacity group '{pool}' does not exist in compute cluster '{cluster_id}'"
                        ),
                        resource_id: Some(container_id.clone()),
                    },
                ));
            }
        }
        Ok(stack)
    }

    fn materialize_capacity_groups(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        if !matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
            for entry in stack.resources.values_mut() {
                let Some(cluster) = entry.config.downcast_mut::<ComputeCluster>() else {
                    continue;
                };
                for group in cluster.capacity_groups.iter_mut() {
                    group.instance_type = None;
                }
            }
            return Ok(stack);
        }

        let fresh_persistent_pools: std::collections::HashSet<String> = stack
            .resources
            .values()
            .filter_map(|entry| entry.config.downcast_ref::<Container>())
            .filter(|container| {
                requires_persisted_pool(container)
                    && !stack_state.resources.contains_key(&container.id)
            })
            .map(|container| {
                container
                    .pool
                    .clone()
                    .unwrap_or_else(|| needed_capacity_group(container).to_string())
            })
            .collect();

        for (cluster_id, entry) in &mut stack.resources {
            let Some(cluster) = entry.config.downcast_mut::<ComputeCluster>() else {
                continue;
            };
            for group in cluster.capacity_groups.iter_mut() {
                let scale = declared_scale_policy(group);
                materialize_group(group, stack_state.platform, config, &scale)?;
                let explicit_selection = config
                    .stack_settings
                    .compute
                    .as_ref()
                    .and_then(|settings| settings.pools.get(&group.group_id))
                    .and_then(|selection| selection.failure_domains())
                    .cloned();
                let existing_group = stack_state
                    .resources
                    .get(cluster_id)
                    .and_then(|state| state.config.downcast_ref::<ComputeCluster>())
                    .filter(|existing| {
                        existing
                            .capacity_groups
                            .iter()
                            .any(|existing_group| existing_group.group_id == group.group_id)
                    });
                // Without an explicit choice, a pool that already exists keeps the topology it
                // was created with, including a default it got at install. Dropping that default
                // here would make every later release differ from the installed setup-owned
                // cluster and ask for setup.
                if explicit_selection.is_none() {
                    if let Some(existing) = existing_group {
                        if let Some(spread) = existing.failure_domain_spread.get(&group.group_id) {
                            cluster
                                .failure_domain_spread
                                .entry(group.group_id.clone())
                                .or_insert(*spread);
                        }
                        if let Some(domains) =
                            existing.selected_failure_domains.get(&group.group_id)
                        {
                            cluster
                                .selected_failure_domains
                                .entry(group.group_id.clone())
                                .or_insert_with(|| domains.clone());
                        }
                        continue;
                    }
                }
                // A persistent pool that does not exist yet gets the planner's default. Leaving
                // it aggregate would spread its machines over every zone while its volumes are
                // created in one.
                let Some(selection) = explicit_selection.or_else(|| {
                    fresh_persistent_pools
                        .contains(&group.group_id)
                        .then(default_persistent_failure_domains)
                }) else {
                    continue;
                };
                let existing_group_is_aggregate = existing_group.is_some_and(|existing| {
                    !existing.failure_domain_spread.contains_key(&group.group_id)
                        && !existing
                            .selected_failure_domains
                            .contains_key(&group.group_id)
                });
                let is_implicit_single_domain_default = selection.spread == 1
                    && selection.selected_failure_domains.is_empty()
                    && !fresh_persistent_pools.contains(&group.group_id);
                if existing_group_is_aggregate && is_implicit_single_domain_default {
                    continue;
                }
                cluster
                    .failure_domain_spread
                    .insert(group.group_id.clone(), selection.spread);
                if !selection.selected_failure_domains.is_empty() {
                    cluster.selected_failure_domains.insert(
                        group.group_id.clone(),
                        selection.selected_failure_domains.clone(),
                    );
                }
            }
        }
        Ok(stack)
    }

    async fn create_cluster(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Auto-generating ComputeCluster for containers in stack");
        // If a daemon explicitly references a cluster on a cloud platform, use
        // its name so the .cluster("X") reference resolves. Local daemons ignore
        // the field, so local container auto-clusters keep the default name.
        let cluster_id = referenced_daemon_clusters_for_platform(&stack, stack_state.platform)
            .into_iter()
            .next()
            .unwrap_or_else(|| "compute".to_string());

        // Collect all containers that will target this auto-generated cluster
        // (containers without an explicit cluster assignment)
        let containers: Vec<&Container> = stack
            .resources
            .values()
            .filter_map(|entry| entry.config.downcast_ref::<Container>())
            .filter(|c| c.cluster.is_none())
            .collect();

        // Build capacity groups, categorized by hardware needs for cloud
        // platforms. Auto-generated clusters never carry a nested-virtualization
        // requirement — that's a hardware capability the customer has to opt
        // into by declaring a ComputeCluster explicitly with a capacity group
        // whose `nestedVirtualization: true` is set. The preflight does not
        // smuggle the flag in from workloads.
        let capacity_groups =
            build_categorized_capacity_groups(&containers, stack_state.platform, false, config)?;

        info!(
            platform = %stack_state.platform,
            groups = capacity_groups.len(),
            "Creating ComputeCluster with {} capacity group(s)",
            capacity_groups.len()
        );

        let mut cluster_builder = ComputeCluster::new(cluster_id.clone());
        for group in &capacity_groups {
            cluster_builder = cluster_builder.capacity_group(group.clone());
        }
        let cluster = cluster_builder.build();

        // Add network dependency if NetworkMutation created one (Phase 1 runs before Phase 2)
        let dependencies = match stack_state.platform {
            Platform::Aws | Platform::Gcp | Platform::Azure
                if stack.resources.contains_key("default-network") =>
            {
                vec![ResourceRef::new(
                    Network::RESOURCE_TYPE,
                    "default-network".to_string(),
                )]
            }
            _ => Vec::new(),
        };

        // The cluster boundary is setup-owned. Runtime management may still
        // scale or roll workers inside the setup-created boundary.
        let cluster_entry = ResourceEntry {
            enabled_when: None,
            config: alien_core::Resource::new(cluster),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies,
            remote_access: false,
        };

        stack.resources.insert(cluster_id.clone(), cluster_entry);

        // Update all Container resources to reference this cluster
        let container_ids: Vec<String> = stack
            .resources
            .iter()
            .filter(|(_, entry)| entry.config.resource_type().as_ref() == "container")
            .map(|(id, _)| id.clone())
            .collect();

        for container_id in &container_ids {
            if let Some(entry) = stack.resources.get_mut(container_id) {
                if let Some(container) = entry.config.downcast_mut::<Container>() {
                    if container.cluster.is_none() {
                        container.cluster = Some(cluster_id.clone());
                        // For cloud platforms with multiple capacity groups, assign pool
                        if container.pool.is_none()
                            && matches!(
                                stack_state.platform,
                                Platform::Aws | Platform::Gcp | Platform::Azure
                            )
                            && capacity_groups.len() > 1
                        {
                            container.pool = Some(needed_capacity_group(container).to_string());
                        }
                        debug!(
                            container_id = %container_id,
                            cluster_id = %cluster_id,
                            "Updated container to reference auto-generated cluster"
                        );
                    }
                }
            }
        }

        info!(
            cluster_id = %cluster_id,
            container_count = container_ids.len(),
            "Generated ComputeCluster '{}' for {} containers",
            cluster_id,
            container_ids.len()
        );

        Ok(stack)
    }

    async fn add_missing_capacity_groups(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        if !matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
            return Ok(stack);
        }

        let cluster_id = stack
            .resources
            .iter()
            .find(|(_, e)| e.config.resource_type().as_ref() == "compute-cluster")
            .map(|(id, _)| id.clone())
            .expect("should_run verified cluster exists");

        // Collect containers needing new groups
        let needed: Vec<(String, String)> = {
            let cluster_entry = stack.resources.get(&cluster_id).unwrap();
            let cluster = cluster_entry
                .config
                .downcast_ref::<ComputeCluster>()
                .unwrap();
            let existing: Vec<&str> = cluster
                .capacity_groups
                .iter()
                .map(|g| g.group_id.as_str())
                .collect();
            stack
                .resources
                .iter()
                .filter_map(|(cid, entry)| {
                    let c = entry.config.downcast_ref::<Container>()?;
                    if c.pool.is_some() || persisted_without_pool(stack_state, c) {
                        return None;
                    }
                    let g = needed_capacity_group(c).to_string();
                    if existing.contains(&g.as_str()) {
                        return None;
                    }
                    Some((cid.clone(), g))
                })
                .collect()
        };

        if needed.is_empty() {
            return Ok(stack);
        }

        let mut new_group_ids: Vec<String> = needed.iter().map(|(_, g)| g.clone()).collect();
        new_group_ids.sort();
        new_group_ids.dedup();

        // Newly-added capacity groups on an existing cluster do not inherit
        // nested-virtualization from any other group. If the customer needs
        // nested virt on a new pool, they must declare it on the cluster
        // explicitly. Existing customer-declared groups keep whatever
        // `nested_virtualization` setting they were created with.
        let mut new_groups: Vec<CapacityGroup> = Vec::new();
        for group_id in &new_group_ids {
            let group_containers: Vec<&Container> = stack
                .resources
                .values()
                .filter_map(|e| e.config.downcast_ref::<Container>())
                .filter(|c| {
                    c.pool.is_none()
                        && !persisted_without_pool(stack_state, c)
                        && needed_capacity_group(c) == group_id.as_str()
                })
                .collect();
            let group = build_capacity_group_for_id(
                group_id,
                &group_containers,
                stack_state.platform,
                false,
                config,
            )?;
            info!(group_id = %group_id, instance_type = ?group.instance_type, "Adding new capacity group");
            new_groups.push(group);
        }

        {
            let cluster_entry = stack.resources.get_mut(&cluster_id).unwrap();
            let cluster = cluster_entry
                .config
                .downcast_mut::<ComputeCluster>()
                .unwrap();
            for group in new_groups {
                cluster.capacity_groups.push(group);
            }
        }

        for (container_id, group_id) in &needed {
            if let Some(entry) = stack.resources.get_mut(container_id) {
                if let Some(c) = entry.config.downcast_mut::<Container>() {
                    if c.pool.is_none() {
                        c.pool = Some(group_id.clone());
                        debug!(container_id = %container_id, pool = %group_id, "Assigned to new capacity group");
                    }
                }
            }
        }

        info!(cluster_id = %cluster_id, new_groups = new_group_ids.len(), "Added capacity groups");
        Ok(stack)
    }
}

/// Collect cluster names referenced by daemons in the stack via `.cluster(...)`.
/// Returns an empty Vec if no daemons reference any cluster.
fn referenced_daemon_clusters(stack: &Stack) -> Vec<String> {
    let mut clusters: Vec<String> = Vec::new();
    for entry in stack.resources.values() {
        if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
            if let Some(ref c) = daemon.cluster {
                if !clusters.contains(c) {
                    clusters.push(c.clone());
                }
            }
        }
    }
    clusters
}

fn referenced_daemon_clusters_for_platform(stack: &Stack, platform: Platform) -> Vec<String> {
    if platform == Platform::Local {
        Vec::new()
    } else {
        referenced_daemon_clusters(stack)
    }
}

/// Determine which capacity group a container needs based on its hardware requirements.
fn needed_capacity_group(container: &Container) -> &'static str {
    if requires_persisted_pool(container) {
        return "stateful";
    }
    if container.gpu.is_some() {
        return "gpu";
    }
    if let Some(ref s) = container.ephemeral_storage {
        if let Ok(bytes) = instance_catalog::parse_memory_bytes(s) {
            const THRESH: u64 = 200 * 1024 * 1024 * 1024;
            if bytes > THRESH {
                return "storage";
            }
        }
    }
    "general"
}

fn requires_persisted_pool(container: &Container) -> bool {
    container.stateful && container.persistent_storage.is_some()
}

fn persisted_without_pool(stack_state: &StackState, container: &Container) -> bool {
    stack_state
        .resources
        .get(&container.id)
        .and_then(|state| state.config.downcast_ref::<Container>())
        .is_some_and(|persisted| persisted.pool.is_none())
}

/// Selections that keep installed cloud pools as they are, for pools the deployment stores no
/// compute choice for and the release names no machine for.
///
/// A pool installed before compute choices were stored has a machine but no choice. Planning it
/// like a fresh pool would apply whatever the catalog recommends today, so an unchanged release
/// could replace its machines without setup. The installed machine and counts are the choice the
/// deployment runs on; the planner validates them against the release like any stored choice.
/// A stored choice without a machine is the deployment's Automatic intent and stays planned.
fn installed_pool_selections(
    stack: &Stack,
    stack_state: &StackState,
    settings: Option<&ComputeSettings>,
) -> BTreeMap<String, ComputePoolSelection> {
    let release_groups: Vec<&CapacityGroup> = stack
        .resources
        .values()
        .filter_map(|entry| entry.config.downcast_ref::<ComputeCluster>())
        .flat_map(|cluster| &cluster.capacity_groups)
        .collect();
    stack_state
        .resources
        .values()
        .filter_map(|state| state.config.downcast_ref::<ComputeCluster>())
        .flat_map(|cluster| &cluster.capacity_groups)
        .filter(|group| {
            settings.is_none_or(|settings| !settings.pools.contains_key(&group.group_id))
                && !release_groups.iter().any(|release_group| {
                    release_group.group_id == group.group_id
                        && release_group.instance_type.is_some()
                })
        })
        .filter_map(|group| {
            let machine = Some(group.instance_type.clone()?);
            let selection = if group.min_size == group.max_size {
                ComputePoolSelection::Fixed {
                    machines: group.max_size,
                    machine,
                    failure_domains: None,
                }
            } else {
                ComputePoolSelection::Autoscale {
                    min: group.min_size,
                    max: group.max_size,
                    machine,
                    failure_domains: None,
                }
            };
            Some((group.group_id.clone(), selection))
        })
        .collect()
}

fn group_needs_materialization(
    group: &CapacityGroup,
    platform: Platform,
    config: &DeploymentConfig,
) -> bool {
    if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        return group.instance_type.is_some();
    }

    let Some(settings) = &config.stack_settings.compute else {
        return group.instance_type.is_none() || group.profile.is_none();
    };
    let Some(selection) = settings.pools.get(&group.group_id) else {
        return group.instance_type.is_none() || group.profile.is_none();
    };
    group.instance_type.as_deref() != selection.machine()
        || group.min_size != selection.min_size()
        || group.max_size != selection.max_size()
        || group.profile.is_none()
}

/// The scale bounds a capacity group carries: its declared policy, or the exact
/// counts of an older stack that only has `minSize` and `maxSize`.
fn declared_scale_policy(group: &CapacityGroup) -> CapacityGroupScalePolicy {
    group.scale_policy.clone().unwrap_or_else(|| {
        CapacityGroupScalePolicy::from_selected_bounds(group.min_size, group.max_size)
    })
}

/// Materializes `group` from the deployment's selection for it and returns that selection.
fn materialize_group<'c>(
    group: &mut CapacityGroup,
    platform: Platform,
    config: &'c DeploymentConfig,
    scale: &CapacityGroupScalePolicy,
) -> Result<&'c ComputePoolSelection> {
    let selection = config
        .stack_settings
        .compute
        .as_ref()
        .and_then(|settings| settings.pools.get(&group.group_id))
        .ok_or_else(|| {
            AlienError::new(crate::error::ErrorData::SetupRequired {
                message: format!(
                    "Select compute for {} capacity group '{}' in the installation setup.",
                    platform, group.group_id
                ),
            })
        })?;
    materialize_selection_within(group, platform, selection, scale)?;
    Ok(selection)
}

/// Use the same validation and profile derivation for declared machine changes
/// as for deployment compute selections.
pub(crate) fn materialize_selected_group(
    group: &mut CapacityGroup,
    platform: Platform,
    selection: &ComputePoolSelection,
) -> Result<()> {
    let scale = declared_scale_policy(group);
    materialize_selection_within(group, platform, selection, &scale)
}

fn materialize_selection_within(
    group: &mut CapacityGroup,
    platform: Platform,
    selection: &ComputePoolSelection,
    scale: &CapacityGroupScalePolicy,
) -> Result<()> {
    selection.validate().map_err(|message| {
        AlienError::new(crate::error::ErrorData::StackMutationFailed {
            mutation_name: "ComputeClusterMutation".to_string(),
            message: format!(
                "Invalid compute selection for '{}': {message}",
                group.group_id
            ),
            resource_id: None,
        })
    })?;
    let requirements = capacity_group_requirements(group);
    let errors =
        validate_compute_pool_selection(platform, &group.group_id, selection, &requirements, scale);
    if !errors.is_empty() {
        return Err(AlienError::new(
            crate::error::ErrorData::StackMutationFailed {
                mutation_name: "ComputeClusterMutation".to_string(),
                message: format!(
                    "Invalid compute selection for '{}': {}",
                    group.group_id,
                    errors.join("; ")
                ),
                resource_id: None,
            },
        ));
    }
    let machine = selection.machine().ok_or_else(|| {
        AlienError::new(crate::error::ErrorData::StackMutationFailed {
            mutation_name: "ComputeClusterMutation".to_string(),
            message: format!(
                "Compute selection for '{}' must include a provider machine on {}",
                group.group_id, platform
            ),
            resource_id: None,
        })
    })?;
    let spec = instance_catalog::find_instance_type(platform, machine).ok_or_else(|| {
        AlienError::new(crate::error::ErrorData::StackMutationFailed {
            mutation_name: "ComputeClusterMutation".to_string(),
            message: format!(
                "Unknown {} machine '{}' selected for capacity group '{}'",
                platform, machine, group.group_id
            ),
            resource_id: None,
        })
    })?;
    if group.instance_type.as_deref() != Some(machine) || group.profile.is_none() {
        group.profile =
            Some(spec.to_machine_profile_for_storage(requirements.max_ephemeral_storage_bytes));
    }
    group.instance_type = Some(machine.to_string());
    group.min_size = selection.min_size();
    group.max_size = selection.max_size();
    Ok(())
}

fn default_min_machines(requirements: &WorkloadRequirements) -> u32 {
    if requirements.total_cpu_at_desired > 0.0 || requirements.total_memory_bytes_at_desired > 0 {
        1
    } else {
        0
    }
}

fn default_max_machines(requirements: &WorkloadRequirements) -> u32 {
    let min = default_min_machines(requirements);
    let by_cpu =
        (requirements.total_cpu_at_max / requirements.max_cpu_per_container.max(1.0)).ceil() as u32;
    let by_mem = requirements
        .total_memory_bytes_at_max
        .div_ceil(requirements.max_memory_per_container.max(1)) as u32;
    min.max(by_cpu).max(by_mem).max(1)
}

/// Build the generated cluster's capacity groups: one per pool its containers run on. A
/// container's explicit `pool` names its group; otherwise its hardware needs pick one.
fn build_categorized_capacity_groups(
    containers: &[&Container],
    platform: Platform,
    needs_nested_virt: bool,
    config: &DeploymentConfig,
) -> Result<Vec<CapacityGroup>> {
    match platform {
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            // Installed clusters list the generated groups in this order; keep it so a release
            // prepares the same setup-owned cluster. Other explicit pools follow by name.
            const GENERATED_ORDER: [&str; 4] = ["general", "stateful", "storage", "gpu"];
            let rank = |pool: &str| {
                GENERATED_ORDER
                    .iter()
                    .position(|known| *known == pool)
                    .unwrap_or(GENERATED_ORDER.len())
            };
            let mut pools: BTreeMap<(usize, &str), Vec<&Container>> = BTreeMap::new();
            for c in containers {
                let pool = c
                    .pool
                    .as_deref()
                    .unwrap_or_else(|| needed_capacity_group(c));
                pools.entry((rank(pool), pool)).or_default().push(c);
            }
            if pools.is_empty() {
                pools.insert((rank("general"), "general"), vec![]);
            }
            pools
                .into_iter()
                .map(|((_, id), members)| {
                    build_capacity_group_for_id(id, &members, platform, needs_nested_virt, config)
                })
                .collect()
        }
        Platform::Local => Ok(vec![CapacityGroup {
            group_id: "general".to_string(),
            instance_type: None,
            profile: Some(MachineProfile {
                cpu: "4.0".to_string(),
                memory_bytes: 8 * 1024 * 1024 * 1024,
                ephemeral_storage_bytes: 50 * 1024 * 1024 * 1024,
                architecture: None,
                gpu: None,
            }),
            min_size: 1,
            max_size: 1,
            scale_policy: None,
            nested_virtualization: None,
        }]),
        Platform::Kubernetes | Platform::Machines | Platform::Test => Ok(vec![CapacityGroup {
            group_id: "general".to_string(),
            instance_type: None,
            profile: Some(MachineProfile {
                cpu: "4.0".to_string(),
                memory_bytes: 8 * 1024 * 1024 * 1024,
                ephemeral_storage_bytes: 50 * 1024 * 1024 * 1024,
                architecture: None,
                gpu: None,
            }),
            min_size: 0,
            max_size: 0,
            scale_policy: None,
            nested_virtualization: None,
        }]),
    }
}

/// Build a capacity group for a specific group_id.
fn build_capacity_group_for_id(
    group_id: &str,
    containers: &[&Container],
    platform: Platform,
    needs_nested_virt: bool,
    config: &DeploymentConfig,
) -> Result<CapacityGroup> {
    let mut requirements = if containers.is_empty() {
        WorkloadRequirements {
            total_cpu_at_desired: 1.0,
            total_memory_bytes_at_desired: 2 * 1024 * 1024 * 1024,
            total_cpu_at_max: 1.0,
            total_memory_bytes_at_max: 2 * 1024 * 1024 * 1024,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 2 * 1024 * 1024 * 1024,
            max_ephemeral_storage_bytes: 0,
            gpu: None,
            architecture: None,
            nested_virt: false,
        }
    } else {
        aggregate_workload_requirements(containers)?
    };
    requirements.nested_virt = needs_nested_virt;
    let mut group = CapacityGroup {
        group_id: group_id.to_string(),
        instance_type: None,
        // Preserve the portable workload requirements until materialization.
        // The selected provider machine profile replaces this value below.
        profile: Some(MachineProfile {
            cpu: requirements.max_cpu_per_container.to_string(),
            memory_bytes: requirements.max_memory_per_container,
            ephemeral_storage_bytes: requirements.max_ephemeral_storage_bytes,
            architecture: requirements.architecture,
            gpu: requirements.gpu.clone(),
        }),
        min_size: default_min_machines(&requirements),
        max_size: default_max_machines(&requirements),
        scale_policy: None,
        nested_virtualization: None,
    };
    if matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        // No source declaration bounds a generated pool; its counts are only the
        // recommendation, and the installer may choose others.
        let selected_max = config
            .stack_settings
            .compute
            .as_ref()
            .and_then(|settings| settings.pools.get(group_id))
            .map(ComputePoolSelection::max_size);
        let scale = generated_pool_scale_policy(group.min_size, group.max_size, selected_max);
        let selection = materialize_group(&mut group, platform, config, &scale)?;
        if !containers.is_empty() {
            check_pool_capacity(platform, group_id, selection, &requirements).map_err(
                |message| {
                    AlienError::new(crate::error::ErrorData::StackMutationFailed {
                        mutation_name: "ComputeClusterMutation".to_string(),
                        message,
                        resource_id: None,
                    })
                },
            )?;
        }
    } else {
        group.profile = Some(MachineProfile {
            cpu: format!(
                "{}.0",
                requirements.max_cpu_per_container.max(1.0).ceil() as u32
            ),
            memory_bytes: requirements
                .max_memory_per_container
                .max(2 * 1024 * 1024 * 1024),
            ephemeral_storage_bytes: requirements
                .max_ephemeral_storage_bytes
                .max(20 * 1024 * 1024 * 1024),
            architecture: requirements.architecture,
            gpu: requirements.gpu,
        });
    }
    Ok(group)
}

/// Aggregate resource requirements from all containers into a single WorkloadRequirements.
fn aggregate_workload_requirements(containers: &[&Container]) -> Result<WorkloadRequirements> {
    let mut total_cpu_at_desired: f64 = 0.0;
    let mut total_memory_bytes_at_desired: u64 = 0;
    let mut total_cpu_at_max: f64 = 0.0;
    let mut total_memory_bytes_at_max: u64 = 0;
    let mut max_cpu_per_container: f64 = 0.0;
    let mut max_memory_per_container: u64 = 0;
    let mut max_ephemeral: u64 = 0;
    let mut gpu_requirement: Option<alien_core::GpuSpec> = None;

    for container in containers {
        let max_replicas = container
            .autoscaling
            .as_ref()
            .map(|a| a.max)
            .or(container.replicas)
            .unwrap_or(1) as f64;
        let desired_replicas = container
            .autoscaling
            .as_ref()
            .map(|a| a.desired)
            .or(container.replicas)
            .unwrap_or(1) as f64;

        let cpu_per_replica =
            instance_catalog::parse_cpu(&container.cpu.desired).map_err(|msg| {
                AlienError::new(crate::error::ErrorData::StackMutationFailed {
                    mutation_name: "ComputeClusterMutation".to_string(),
                    message: format!(
                        "container '{}': failed to parse CPU '{}': {msg}",
                        container.id, container.cpu.desired
                    ),
                    resource_id: Some(container.id.clone()),
                })
            })?;

        let mem_per_replica = instance_catalog::parse_memory_bytes(&container.memory.desired)
            .map_err(|msg| {
                AlienError::new(crate::error::ErrorData::StackMutationFailed {
                    mutation_name: "ComputeClusterMutation".to_string(),
                    message: format!(
                        "container '{}': failed to parse memory '{}': {msg}",
                        container.id, container.memory.desired
                    ),
                    resource_id: Some(container.id.clone()),
                })
            })?;

        total_cpu_at_desired += cpu_per_replica * desired_replicas;
        total_memory_bytes_at_desired += (mem_per_replica as f64 * desired_replicas) as u64;
        total_cpu_at_max += cpu_per_replica * max_replicas;
        total_memory_bytes_at_max += (mem_per_replica as f64 * max_replicas) as u64;

        // Track the largest single container (for instance sizing)
        if cpu_per_replica > max_cpu_per_container {
            max_cpu_per_container = cpu_per_replica;
        }
        if mem_per_replica > max_memory_per_container {
            max_memory_per_container = mem_per_replica;
        }

        // Track max ephemeral storage across all containers
        if let Some(ref storage_str) = container.ephemeral_storage {
            let storage_bytes =
                instance_catalog::parse_memory_bytes(storage_str).map_err(|msg| {
                    AlienError::new(crate::error::ErrorData::StackMutationFailed {
                        mutation_name: "ComputeClusterMutation".to_string(),
                        message: format!(
                            "container '{}': failed to parse ephemeral storage '{}': {msg}",
                            container.id, storage_str
                        ),
                        resource_id: Some(container.id.clone()),
                    })
                })?;
            max_ephemeral = max_ephemeral.max(storage_bytes);
        }

        // Capture GPU requirement (first container with GPU wins)
        if gpu_requirement.is_none() {
            if let Some(ref gpu) = container.gpu {
                gpu_requirement = Some(alien_core::GpuSpec {
                    gpu_type: gpu.gpu_type.clone(),
                    count: gpu.count,
                });
            }
        }
    }

    Ok(WorkloadRequirements {
        total_cpu_at_desired,
        total_memory_bytes_at_desired,
        total_cpu_at_max,
        total_memory_bytes_at_max,
        max_cpu_per_container,
        max_memory_per_container,
        max_ephemeral_storage_bytes: max_ephemeral,
        gpu: gpu_requirement,
        architecture: None,
        nested_virt: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        compute_planner::plan_compute, ComputeChoiceRange, ComputePoolSelection, ComputeSettings,
        ContainerAutoscaling, ContainerCode, DaemonCode, EnvironmentVariablesSnapshot,
        ExternalBindings, FailureDomainSelection, NetworkSettings, PersistentStorage, ResourceSpec,
        StackSettings, VolumeBackups,
    };
    use indexmap::IndexMap;

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn test_container(id: &str, cpu: &str, memory: &str) -> Container {
        Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: cpu.to_string(),
                desired: cpu.to_string(),
            })
            .memory(ResourceSpec {
                min: memory.to_string(),
                desired: memory.to_string(),
            })
            .permissions("test".to_string())
            .build()
    }

    fn node_dependency_stack() -> Stack {
        let storage = alien_core::Storage::new("objects".to_string()).build();
        let daemon = Daemon::new("app".to_string())
            .code(DaemonCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .cluster("compute".to_string())
            .permissions("reader".to_string())
            .link(&storage)
            .build();
        let mut stack = Stack::new("example".to_string())
            .add(
                Network::new("network".to_string())
                    .settings(NetworkSettings::Create {
                        cidr: None,
                        availability_zones: 2,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(storage, ResourceLifecycle::Frozen)
            .add(
                ComputeCluster::new("compute".to_string())
                    .node_permissions(
                        alien_core::PermissionProfile::new()
                            .resource("objects", ["storage/data-read"]),
                    )
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(daemon, ResourceLifecycle::Live)
            .permission(
                "reader",
                alien_core::PermissionProfile::new().resource("objects", ["storage/data-read"]),
            )
            .build();
        stack.resources.get_mut("compute").unwrap().dependencies =
            vec![ResourceRef::new(Network::RESOURCE_TYPE, "network")];
        stack
    }

    fn node_dependency_runner() -> crate::runner::PreflightRunner {
        let mut registry = crate::PreflightRegistry::new();
        registry.add_mutation(Box::new(ComputeClusterMutation));
        registry.add_mutation(Box::new(crate::mutations::ServiceAccountMutation));
        registry.add_mutation(Box::new(
            crate::mutations::ServiceAccountDependenciesMutation,
        ));
        crate::runner::PreflightRunner::with_registry(registry)
    }

    #[tokio::test]
    async fn node_platform_selector_projects_one_manifest_before_target_validation() {
        let mut stack = node_dependency_stack();
        stack.supported_platforms = Some(vec![Platform::Aws, Platform::Machines]);
        stack
            .resources
            .get_mut("compute")
            .unwrap()
            .config
            .downcast_mut::<ComputeCluster>()
            .unwrap()
            .node_permissions_platforms = Some(vec![Platform::Aws]);
        let app = stack.resources["app"].config.clone();
        let profiles = stack.permissions.clone();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        for platform in [Platform::Aws, Platform::Machines] {
            let check = PermissionSetsExistCheck
                .check(&stack, platform)
                .await
                .unwrap();
            assert!(check.success, "{:?}", check.errors);
            let prepared = node_dependency_runner()
                .apply_mutations(stack.clone(), &StackState::new(platform), &config)
                .await
                .unwrap();
            let cluster = prepared.resources["compute"]
                .config
                .downcast_ref::<ComputeCluster>()
                .unwrap();
            assert_eq!(cluster.id, "compute");
            assert_eq!(cluster.node_permissions_platforms, None);
            let mut expected_dependencies =
                vec![ResourceRef::new(Network::RESOURCE_TYPE, "network")];
            if platform == Platform::Aws {
                assert_eq!(
                    cluster.node_permissions.as_ref().unwrap().0["objects"],
                    vec![PermissionSetReference::Inline(
                        alien_permissions::get_permission_set("storage/data-read")
                            .unwrap()
                            .clone()
                    )]
                );
                expected_dependencies.push(ResourceRef::new(
                    alien_core::Storage::RESOURCE_TYPE,
                    "objects",
                ));
            } else {
                assert_eq!(cluster.node_permissions, None);
            }
            assert_eq!(
                prepared.resources["compute"].dependencies,
                expected_dependencies
            );
            assert_eq!(prepared.resources["app"].config, app);
            assert_eq!(prepared.permissions, profiles);
        }
        // An excluded node-only target need not exist on the other platform.
        stack
            .resources
            .get_mut("compute")
            .unwrap()
            .config
            .downcast_mut::<ComputeCluster>()
            .unwrap()
            .node_permissions =
            Some(alien_core::PermissionProfile::new().resource("missing", ["storage/data-read"]));
        assert!(
            PermissionSetsExistCheck
                .check(&stack, Platform::Machines)
                .await
                .unwrap()
                .success
        );
        let excluded = node_dependency_runner()
            .apply_mutations(stack.clone(), &StackState::new(Platform::Machines), &config)
            .await
            .unwrap();
        assert_eq!(
            excluded.resources["compute"].dependencies,
            vec![ResourceRef::new(Network::RESOURCE_TYPE, "network")]
        );
        assert!(
            !PermissionSetsExistCheck
                .check(&stack, Platform::Aws)
                .await
                .unwrap()
                .success
        );
        assert!(node_dependency_runner()
            .apply_mutations(stack, &StackState::new(Platform::Aws), &config)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn invalid_node_platform_selectors_fail_declaration_and_preparation() {
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        for (platforms, has_profile) in [
            (vec![], true),
            (vec![Platform::Aws, Platform::Aws], true),
            (vec![Platform::Machines], true),
            (vec![Platform::Kubernetes], true),
            (vec![Platform::Aws], false),
        ] {
            let mut stack = node_dependency_stack();
            let cluster = stack
                .resources
                .get_mut("compute")
                .unwrap()
                .config
                .downcast_mut::<ComputeCluster>()
                .unwrap();
            cluster.node_permissions_platforms = Some(platforms);
            if !has_profile {
                cluster.node_permissions = None;
            }
            let state = StackState::new(Platform::Machines);
            assert!(ComputeClusterMutation.should_run(&stack, &state, &config));
            let check = PermissionSetsExistCheck
                .check(&stack, state.platform)
                .await
                .unwrap();
            assert!(!check.success);
            assert!(check
                .errors
                .iter()
                .any(|error| error.contains("nodePermissionsPlatforms")));
            let error = node_dependency_runner()
                .apply_mutations(stack, &state, &config)
                .await
                .unwrap_err();
            assert!(format!("{error:?}").contains("nodePermissionsPlatforms"));
        }
        let unselected = node_dependency_stack();
        assert!(
            !PermissionSetsExistCheck
                .check(&unselected, Platform::Machines)
                .await
                .unwrap()
                .success
        );
        assert!(node_dependency_runner()
            .apply_mutations(unselected, &StackState::new(Platform::Machines), &config)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn node_targets_preserve_typed_dependencies_profiles_and_links() {
        let state = StackState::new(Platform::Gcp);
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        // External locations do not replace the declared resource's identity/type.
        config.external_bindings.insert(
            "objects",
            alien_core::ExternalBinding::Storage(alien_core::StorageBinding::gcs(
                "existing-objects",
            )),
        );
        let stack = node_dependency_stack();
        let profiles = stack.permissions.clone();
        let app = stack.resources["app"].config.clone();
        let runner = node_dependency_runner();
        let prepared = runner
            .apply_mutations(stack, &state, &config)
            .await
            .unwrap();
        assert_eq!(
            prepared.resources["compute"].dependencies,
            vec![
                ResourceRef::new(Network::RESOURCE_TYPE, "network"),
                ResourceRef::new(alien_core::Storage::RESOURCE_TYPE, "objects"),
            ]
        );
        assert_eq!(
            prepared.resources["objects"].dependencies,
            vec![ResourceRef::new(
                alien_core::ServiceAccount::RESOURCE_TYPE,
                "reader-sa"
            )]
        );
        assert!(prepared.resources["reader-sa"].dependencies.is_empty());
        assert_eq!(prepared.permissions, profiles);
        assert_eq!(prepared.resources["app"].config, app);
        let check = crate::compile_time::ValidResourceDependenciesCheck
            .check(&prepared, Platform::Gcp)
            .await
            .unwrap();
        assert!(check.success, "{:?}", check.errors);
        let repeated = runner
            .apply_mutations(prepared.clone(), &state, &config)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&repeated).unwrap(),
            serde_json::to_value(&prepared).unwrap()
        );
    }

    #[tokio::test]
    async fn node_target_cycle_is_rejected_by_post_mutation_dependency_validation() {
        let mut stack = node_dependency_stack();
        stack
            .resources
            .get_mut("compute")
            .unwrap()
            .config
            .downcast_mut::<ComputeCluster>()
            .unwrap()
            .node_permissions_platforms = Some(vec![Platform::Gcp]);
        stack
            .resources
            .get_mut("objects")
            .unwrap()
            .dependencies
            .push(ResourceRef::new(ComputeCluster::RESOURCE_TYPE, "compute"));
        assert!(
            crate::compile_time::ValidResourceDependenciesCheck
                .check(&stack, Platform::Gcp)
                .await
                .unwrap()
                .success
        );
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let error = node_dependency_runner()
            .apply_mutations(stack, &StackState::new(Platform::Gcp), &config)
            .await
            .expect_err("node target creates a real cycle");
        assert_eq!(error.code, "VALIDATION_FAILED");
        assert!(format!("{error:?}").contains("POST_MUTATION_DEPENDENCY_INVALID"));
    }

    #[tokio::test]
    async fn absent_node_grants_leave_existing_dependencies_unchanged() {
        let mut stack = node_dependency_stack();
        stack
            .resources
            .get_mut("compute")
            .unwrap()
            .config
            .downcast_mut::<ComputeCluster>()
            .unwrap()
            .node_permissions = None;
        let before = serde_json::to_value(&stack).unwrap();
        let after = ComputeClusterMutation
            .materialize_node_permissions(stack, Platform::Gcp)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(&after).unwrap(), before);
    }

    #[tokio::test]
    async fn explicit_node_grants_are_materialized_without_workloads() {
        let stack = Stack::new("example".to_string())
            .add(
                alien_core::Storage::new("objects".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ComputeCluster::new("compute".to_string())
                    .node_permissions(
                        alien_core::PermissionProfile::new()
                            .resource("objects", ["storage/data-read"]),
                    )
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = StackState {
            platform: Platform::Gcp,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        assert!(ComputeClusterMutation.should_run(&stack, &state, &config));
        let prepared = ComputeClusterMutation
            .mutate(stack.clone(), &state, &config)
            .await
            .unwrap();
        let cluster = prepared.resources["compute"]
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        let expected = alien_permissions::get_permission_set("storage/data-read")
            .unwrap()
            .clone();
        assert_eq!(
            cluster.node_permissions.as_ref().unwrap().0["objects"],
            vec![PermissionSetReference::Inline(expected)]
        );
        assert!(prepared.permissions.profiles.is_empty());
        assert!(ComputeClusterMutation
            .materialize_node_permissions(stack.clone(), Platform::Machines)
            .await
            .is_err());
        let mut missing = stack;
        missing.resources.shift_remove("objects");
        assert!(ComputeClusterMutation
            .materialize_node_permissions(missing, Platform::Gcp)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn persistent_stateful_container_gets_pool_with_single_capacity_group() {
        let mut container = Container::new("database".to_string())
            .code(ContainerCode::Image {
                image: "database:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .persistent_storage(PersistentStorage {
                size: "20Gi".to_string(),
                mount_path: "/data".to_string(),
                backups: VolumeBackups::default(),
            })
            .stateful(true)
            .replicas(3)
            .port(8080)
            .permissions("database".to_string())
            .build();
        container.ephemeral_storage = Some("100Gi".to_string());
        let stack = Stack::new("test-stack".to_string())
            .add(container, ResourceLifecycle::Live)
            .build();
        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let initial_plan =
            plan_compute(&stack, Platform::Aws, None).expect("compute plan should build");
        let pool_id = initial_plan.pools[0].pool_id.clone();
        assert_eq!(pool_id, "stateful");
        let recommendation = initial_plan.pools[0].recommended.clone();
        let mut installer_selection = recommendation.clone();
        match &mut installer_selection {
            ComputePoolSelection::Fixed {
                failure_domains, ..
            }
            | ComputePoolSelection::Autoscale {
                failure_domains, ..
            } => *failure_domains = None,
        }
        let installer_settings = ComputeSettings {
            containers: Default::default(),
            pools: [(pool_id.clone(), installer_selection)]
                .into_iter()
                .collect(),
        };
        let planned_selection = plan_compute(&stack, Platform::Aws, Some(&installer_settings))
            .expect("installer selection should preserve required topology")
            .pools[0]
            .selected
            .clone();
        assert_eq!(
            planned_selection
                .failure_domains()
                .map(|selection| selection.spread),
            Some(1)
        );
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings {
                compute: Some(ComputeSettings {
                    containers: Default::default(),
                    pools: [(pool_id.clone(), planned_selection)].into_iter().collect(),
                }),
                ..StackSettings::default()
            })
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ComputeClusterMutation;
        let result = mutation
            .mutate(stack, &stack_state, &config)
            .await
            .expect("persistent container should be planned");
        let container = result.resources["database"]
            .config
            .downcast_ref::<Container>()
            .expect("container should remain present");

        assert_eq!(container.pool.as_deref(), Some("stateful"));
        let cluster = result.resources[container.cluster.as_deref().unwrap()]
            .config
            .downcast_ref::<ComputeCluster>()
            .expect("assigned cluster should exist");
        assert!(cluster
            .capacity_groups
            .iter()
            .any(|group| Some(group.group_id.as_str()) == container.pool.as_deref()));
        let capacity_group = cluster
            .capacity_groups
            .iter()
            .find(|group| Some(group.group_id.as_str()) == container.pool.as_deref())
            .expect("container pool should be materialized");
        assert_eq!(
            capacity_group
                .profile
                .as_ref()
                .expect("materialized group should have a profile")
                .ephemeral_storage_bytes,
            100 * 1024 * 1024 * 1024,
        );
        assert_eq!(cluster.failure_domain_spread.get("stateful"), Some(&1));

        assert!(!mutation.should_run(&result, &stack_state, &config));
        let second = mutation
            .mutate(result.clone(), &stack_state, &config)
            .await
            .expect("a second pass should be a no-op");
        assert_eq!(
            serde_json::to_value(&second).unwrap(),
            serde_json::to_value(&result).unwrap()
        );

        let mut existing_aggregate = result.clone();
        existing_aggregate.resources["database"]
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .pool = None;
        let cluster_id = existing_aggregate.resources["database"]
            .config
            .downcast_ref::<Container>()
            .unwrap()
            .cluster
            .clone()
            .unwrap();
        let existing_cluster = existing_aggregate.resources[&cluster_id]
            .config
            .downcast_mut::<ComputeCluster>()
            .unwrap();
        existing_cluster.failure_domain_spread.clear();
        existing_cluster.selected_failure_domains.clear();
        let mut existing_state = stack_state.clone();
        for (id, entry) in &existing_aggregate.resources {
            existing_state.resources.insert(
                id.clone(),
                alien_core::StackResourceState::new_pending(
                    entry.config.resource_type().to_string(),
                    entry.config.clone(),
                    Some(entry.lifecycle),
                    entry.dependencies.clone(),
                ),
            );
        }
        assert!(!mutation.should_run(&existing_aggregate, &existing_state, &config));
        let redeployed = mutation
            .mutate(existing_aggregate.clone(), &existing_state, &config)
            .await
            .expect("existing aggregate deployment should remain compatible");
        assert_eq!(
            serde_json::to_value(&redeployed).unwrap(),
            serde_json::to_value(&existing_aggregate).unwrap(),
            "redeploy must not rewrite immutable pool or opt into topology"
        );

        let mut aggregate_general = existing_aggregate.clone();
        aggregate_general.resources[&cluster_id]
            .config
            .downcast_mut::<ComputeCluster>()
            .unwrap()
            .capacity_groups[0]
            .group_id = "general".to_string();
        let mut aggregate_state = stack_state.clone();
        for (id, entry) in &aggregate_general.resources {
            aggregate_state.resources.insert(
                id.clone(),
                alien_core::StackResourceState::new_pending(
                    entry.config.resource_type().to_string(),
                    entry.config.clone(),
                    Some(entry.lifecycle),
                    entry.dependencies.clone(),
                ),
            );
        }
        let mut with_new_stateful = aggregate_general;
        let mut new_entry = with_new_stateful.resources["database"].clone();
        let new_container = new_entry.config.downcast_mut::<Container>().unwrap();
        new_container.id = "database-new".to_string();
        new_container.pool = None;
        with_new_stateful
            .resources
            .insert("database-new".to_string(), new_entry);
        let mut migration_config = config.clone();
        let compute = migration_config.stack_settings.compute.as_mut().unwrap();
        let mut general_selection = compute.pools["stateful"].clone();
        match &mut general_selection {
            ComputePoolSelection::Fixed {
                failure_domains, ..
            }
            | ComputePoolSelection::Autoscale {
                failure_domains, ..
            } => *failure_domains = None,
        }
        compute
            .pools
            .insert("general".to_string(), general_selection);

        let expanded = mutation
            .mutate(with_new_stateful, &aggregate_state, &migration_config)
            .await
            .expect("new stateful workload should get a separate pool");
        assert_eq!(
            expanded.resources["database"]
                .config
                .downcast_ref::<Container>()
                .unwrap()
                .pool,
            None,
            "existing aggregate workload must remain unchanged"
        );
        assert_eq!(
            expanded.resources["database-new"]
                .config
                .downcast_ref::<Container>()
                .unwrap()
                .pool
                .as_deref(),
            Some("stateful")
        );
        let expanded_cluster = expanded.resources[&cluster_id]
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        assert!(expanded_cluster
            .capacity_groups
            .iter()
            .any(|group| group.group_id == "general"));
        assert!(expanded_cluster
            .capacity_groups
            .iter()
            .any(|group| group.group_id == "stateful"));
        assert!(!expanded_cluster
            .failure_domain_spread
            .contains_key("general"));
        assert_eq!(
            expanded_cluster.failure_domain_spread.get("stateful"),
            Some(&1)
        );

        let mut invalid = result;
        invalid.resources["database"]
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .pool = Some("missing".to_string());
        assert!(mutation.should_run(&invalid, &stack_state, &config));
        let error = mutation
            .mutate(invalid, &stack_state, &config)
            .await
            .expect_err("an explicit pool must belong to the referenced cluster");
        assert!(error.to_string().contains("Capacity group 'missing'"));
    }

    #[tokio::test]
    async fn local_persistent_container_uses_unconstrained_compute_without_pool() {
        let container = Container::new("database".to_string())
            .code(ContainerCode::Image {
                image: "database:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .persistent_storage(PersistentStorage {
                size: "20Gi".to_string(),
                mount_path: "/data".to_string(),
                backups: VolumeBackups::default(),
            })
            .stateful(true)
            .replicas(1)
            .permissions("database".to_string())
            .build();
        let stack = Stack::new("test-stack".to_string())
            .add(container, ResourceLifecycle::Live)
            .build();
        let stack_state = StackState {
            platform: Platform::Local,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ComputeClusterMutation;
        let result = mutation
            .mutate(stack, &stack_state, &config)
            .await
            .expect("local persistent container should pass compute preparation");
        let container = result.resources["database"]
            .config
            .downcast_ref::<Container>()
            .unwrap();
        assert_eq!(container.pool, None);
        let cluster = result.resources[container.cluster.as_deref().unwrap()]
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        assert_eq!(cluster.capacity_groups.len(), 1);
        assert_eq!(cluster.capacity_groups[0].group_id, "general");
        assert!(!mutation.should_run(&result, &stack_state, &config));
    }

    /// A deploy config that picks a machine for the stateful pool but says nothing about
    /// failure domains must still get one failure domain.
    /// Otherwise the AWS pool spans every zone while its EBS volume is created in one zone, and
    /// the replica can only be scheduled when the machine happens to land in that zone.
    #[tokio::test]
    async fn fresh_persistent_pool_without_failure_domain_choice_gets_single_domain() {
        let container = Container::new("database".to_string())
            .code(ContainerCode::Image {
                image: "database:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .persistent_storage(PersistentStorage {
                size: "20Gi".to_string(),
                mount_path: "/data".to_string(),
                backups: VolumeBackups::default(),
            })
            .stateful(true)
            .replicas(1)
            .port(8080)
            .permissions("database".to_string())
            .build();
        let stack = Stack::new("test-stack".to_string())
            .add(container, ResourceLifecycle::Live)
            .build();
        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let machine_without_domains = ComputeSettings {
            containers: Default::default(),
            pools: [(
                "stateful".to_string(),
                ComputePoolSelection::Fixed {
                    machines: 1,
                    machine: Some("t4g.medium".to_string()),
                    failure_domains: None,
                },
            )]
            .into_iter()
            .collect(),
        };

        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings {
                compute: Some(machine_without_domains),
                ..StackSettings::default()
            })
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let result = ComputeClusterMutation
            .mutate(stack, &stack_state, &config)
            .await
            .expect("persistent container should be planned");
        let container = result.resources["database"]
            .config
            .downcast_ref::<Container>()
            .expect("container should remain present");
        assert_eq!(container.pool.as_deref(), Some("stateful"));
        let cluster = result.resources[container.cluster.as_deref().unwrap()]
            .config
            .downcast_ref::<ComputeCluster>()
            .expect("assigned cluster should exist");
        assert_eq!(cluster.failure_domain_spread.get("stateful"), Some(&1));
        assert!(
            !cluster.selected_failure_domains.contains_key("stateful"),
            "the provider picks the concrete zone"
        );
    }

    #[test]
    fn explicit_domain_pool_does_not_materialize_unrelated_persisted_aggregate_pool() {
        let capacity_group = |group_id: &str| CapacityGroup {
            group_id: group_id.to_string(),
            instance_type: Some("m7i.large".to_string()),
            profile: None,
            min_size: 1,
            max_size: 1,
            scale_policy: None,
            nested_virtualization: None,
        };
        let mut cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(capacity_group("general"))
            .capacity_group(capacity_group("stateful"))
            .build();
        cluster
            .failure_domain_spread
            .insert("stateful".to_string(), 1);
        cluster
            .selected_failure_domains
            .insert("stateful".to_string(), vec!["us-east-1b".to_string()]);
        let stack = Stack::new("test-stack".to_string())
            .add(cluster.clone(), ResourceLifecycle::Frozen)
            .build();

        let mut persisted_cluster = cluster;
        persisted_cluster
            .selected_failure_domains
            .insert("stateful".to_string(), vec!["us-east-1a".to_string()]);
        let persisted_stack = Stack::new("persisted-stack".to_string())
            .add(persisted_cluster, ResourceLifecycle::Frozen)
            .build();
        let persisted_entry = &persisted_stack.resources["compute"];
        let mut stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        stack_state.resources.insert(
            "compute".to_string(),
            alien_core::StackResourceState::new_pending(
                ComputeCluster::RESOURCE_TYPE.to_string(),
                persisted_entry.config.clone(),
                Some(ResourceLifecycle::Frozen),
                Vec::new(),
            ),
        );

        let selection = |selected_failure_domains| ComputePoolSelection::Fixed {
            machines: 1,
            machine: Some("m7i.large".to_string()),
            failure_domains: Some(FailureDomainSelection {
                spread: 1,
                selected_failure_domains,
            }),
        };
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings {
                compute: Some(ComputeSettings {
                    containers: Default::default(),
                    pools: [
                        ("general".to_string(), selection(Vec::new())),
                        (
                            "stateful".to_string(),
                            selection(vec!["us-east-1b".to_string()]),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                }),
                ..StackSettings::default()
            })
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let materialized = ComputeClusterMutation
            .materialize_capacity_groups(stack, &stack_state, &config)
            .expect("mixed aggregate and domain-aware pools should materialize independently");
        let cluster = materialized.resources["compute"]
            .config
            .downcast_ref::<ComputeCluster>()
            .expect("compute cluster should remain present");

        assert!(!cluster.failure_domain_spread.contains_key("general"));
        assert!(!cluster.selected_failure_domains.contains_key("general"));
        assert_eq!(cluster.failure_domain_spread.get("stateful"), Some(&1));
        assert_eq!(cluster.selected_failure_domains["stateful"], ["us-east-1b"]);
    }

    fn deployment_config_with_compute_pool(
        machine: &str,
        min_size: u32,
        max_size: u32,
    ) -> DeploymentConfig {
        deployment_config_with_compute_pools(&[("general", machine, min_size, max_size)])
    }

    fn deployment_config_with_compute_pools(
        selections: &[(&str, &str, u32, u32)],
    ) -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings {
                compute: Some(ComputeSettings {
                    containers: Default::default(),
                    pools: selections
                        .iter()
                        .map(|(pool_id, machine, min_size, max_size)| {
                            let selection = if min_size == max_size {
                                ComputePoolSelection::Fixed {
                                    machines: *min_size,
                                    machine: Some((*machine).to_string()),
                                    failure_domains: None,
                                }
                            } else {
                                ComputePoolSelection::Autoscale {
                                    min: *min_size,
                                    max: *max_size,
                                    machine: Some((*machine).to_string()),
                                    failure_domains: None,
                                }
                            };
                            ((*pool_id).to_string(), selection)
                        })
                        .collect(),
                }),
                ..StackSettings::default()
            })
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    #[test]
    fn test_production_sized_workload_selects_small_ha_fleet() {
        let mut api = test_container("api", "1", "2Gi");
        api.replicas = Some(2);
        let mut ingest = test_container("ingest", "0.75", "2Gi");
        ingest.autoscaling = Some(ContainerAutoscaling {
            min: 2,
            desired: 2,
            max: 4,
            target_cpu_percent: Some(60.0),
            target_memory_percent: Some(80.0),
            target_http_in_flight_per_replica: Some(50),
            max_http_p95_latency_ms: None,
        });
        let mut query = test_container("query", "1", "4Gi");
        query.autoscaling = Some(ContainerAutoscaling {
            min: 2,
            desired: 2,
            max: 4,
            target_cpu_percent: Some(60.0),
            target_memory_percent: Some(75.0),
            target_http_in_flight_per_replica: Some(10),
            max_http_p95_latency_ms: None,
        });
        let mut index = test_container("index-worker", "1", "4Gi");
        index.replicas = Some(1);
        index.ephemeral_storage = Some("20Gi".to_string());

        let containers = vec![api, ingest, query, index];
        let refs: Vec<&Container> = containers.iter().collect();

        for (platform, expected_instance) in [
            (Platform::Aws, "m7i.xlarge"),
            (Platform::Gcp, "n2-standard-4"),
            (Platform::Azure, "Standard_D4s_v5"),
        ] {
            let config = deployment_config_with_compute_pool(expected_instance, 1, 10);
            let group =
                build_capacity_group_for_id("general", &refs, platform, false, &config).unwrap();
            assert_eq!(group.instance_type.as_deref(), Some(expected_instance));
            assert_eq!(group.min_size, 1);
            assert_eq!(group.max_size, 10);
        }
    }

    #[test]
    fn small_workload_accepts_planner_recommended_machine() {
        let mut container = test_container("transaction-service", "0.25", "256Mi");
        container.replicas = Some(2);
        let stack = Stack::new("tcp-transaction".to_string())
            .add(container.clone(), ResourceLifecycle::Live)
            .build();
        let plan = plan_compute(&stack, Platform::Aws, None).expect("compute plan should build");
        let recommendation = plan
            .pools
            .first()
            .expect("general pool should be planned")
            .recommended
            .clone();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings {
                compute: Some(ComputeSettings {
                    containers: Default::default(),
                    pools: [("general".to_string(), recommendation.clone())]
                        .into_iter()
                        .collect(),
                }),
                ..StackSettings::default()
            })
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let group =
            build_capacity_group_for_id("general", &[&container], Platform::Aws, false, &config)
                .expect("planner recommendation should materialize");

        assert_eq!(group.instance_type.as_deref(), recommendation.machine());
    }

    #[tokio::test]
    async fn test_should_run_with_containers_but_no_cluster() {
        let mut resources = IndexMap::new();

        // Add a container without a cluster (None = auto-assign)
        let container = Container::new("api".to_string())
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

        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        assert!(mutation.should_run(&stack, &stack_state, &config));
    }

    #[tokio::test]
    async fn test_should_not_run_with_existing_cluster() {
        let mut resources = IndexMap::new();

        // Add a cluster
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7i.large".to_string()),
                profile: Some(MachineProfile {
                    cpu: "2.0".to_string(),
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    architecture: None,
                    gpu: None,
                }),
                min_size: 1,
                max_size: 10,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build();

        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        // Add a container
        let container = Container::new("api".to_string())
            .cluster("compute".to_string())
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

        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        assert!(!mutation.should_run(&stack, &stack_state, &config));
        let declared_profile = stack.resources["compute"]
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap()
            .capacity_groups[0]
            .profile
            .clone();
        let mut prepared = mutation.mutate(stack, &stack_state, &config).await.unwrap();
        let cluster = prepared.resources["compute"]
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        assert_eq!(
            cluster.capacity_groups[0].instance_type.as_deref(),
            Some("m7i.large")
        );
        assert_eq!(cluster.capacity_groups[0].profile, declared_profile);
        prepared
            .resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .resource_choices = Some(alien_core::ContainerResourceChoices {
            cpu: Some(alien_core::ResourceChoiceRange {
                min: "0.5".into(),
                max: "4".into(),
                default: "1".into(),
            }),
            memory: None,
        });
        let mut selected_config = config.clone();
        selected_config.stack_settings.compute = Some(
            serde_json::from_value(serde_json::json!({"containers":{"api":{"cpu":1.5}}})).unwrap(),
        );
        let resized = mutation
            .mutate(prepared.clone(), &stack_state, &selected_config)
            .await
            .unwrap();
        assert_eq!(
            resized.resources["compute"]
                .config
                .downcast_ref::<ComputeCluster>()
                .unwrap()
                .capacity_groups[0]
                .profile,
            declared_profile
        );
        selected_config.stack_settings.compute = Some(
            serde_json::from_value(serde_json::json!({"containers":{"api":{"cpu":4}}})).unwrap(),
        );
        assert!(mutation.should_run(&prepared, &stack_state, &selected_config));
        let error = mutation
            .mutate(prepared, &stack_state, &selected_config)
            .await
            .unwrap_err();
        assert_eq!(error.code, "STACK_MUTATION_FAILED");
    }

    #[tokio::test]
    async fn test_should_not_run_for_local_daemon_cluster_ref() {
        let mut resources = IndexMap::new();

        let daemon = Daemon::new("host-loader".to_string())
            .cluster("host-runtime".to_string())
            .permissions("loader".to_string())
            .code(alien_core::DaemonCode::Image {
                image: "registry.example.com/host-loader:latest".to_string(),
            })
            .build();

        resources.insert(
            "host-loader".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(daemon),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Local,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[tokio::test]
    async fn test_mutate_creates_cluster_and_updates_containers() {
        let mut resources = IndexMap::new();

        // Add a container without a cluster (None = auto-assign)
        let container = Container::new("api".to_string())
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

        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m7i.large", 1, 1);
        let result = mutation.mutate(stack, &stack_state, &config).await;

        assert!(result.is_ok());
        let mutated_stack = result.unwrap();

        // Check that cluster was created
        assert!(mutated_stack.resources.contains_key("compute"));

        // Check that container was updated to reference the auto-generated cluster
        let container_entry = mutated_stack.resources.get("api").unwrap();
        let container = container_entry.config.downcast_ref::<Container>().unwrap();
        assert_eq!(container.cluster, Some("compute".to_string()));

        // Check that capacity group has instance_type and profile populated
        let cluster_entry = mutated_stack.resources.get("compute").unwrap();
        let cluster = cluster_entry
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        let group = &cluster.capacity_groups[0];
        assert!(group.instance_type.is_some(), "instance_type should be set");
        assert!(group.profile.is_some(), "profile should be set");
        let profile = group.profile.as_ref().unwrap();
        assert!(!profile.cpu.is_empty(), "profile CPU should not be empty");
        assert!(profile.memory_bytes > 0, "profile memory should be > 0");
        assert!(
            profile.ephemeral_storage_bytes > 0,
            "profile ephemeral storage should be > 0"
        );
    }

    /// A stack whose source declares no pool: one container that autoscales from
    /// one to two replicas, so the planner recommends autoscale 1-2.
    fn generated_pool_stack() -> Stack {
        let mut gw = test_container("gw", "1", "2Gi");
        gw.autoscaling = Some(ContainerAutoscaling {
            min: 1,
            desired: 1,
            max: 2,
            target_cpu_percent: Some(70.0),
            target_memory_percent: None,
            target_http_in_flight_per_replica: None,
            max_http_p95_latency_ms: None,
        });
        Stack::new("generated-pool".to_string())
            .add(gw, ResourceLifecycle::Live)
            .build()
    }

    fn general_selection(selection: ComputePoolSelection) -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings {
                compute: Some(ComputeSettings {
                    containers: Default::default(),
                    pools: [("general".to_string(), selection)].into_iter().collect(),
                }),
                ..StackSettings::default()
            })
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    async fn prepare_generated_pool(selection: ComputePoolSelection) -> Result<CapacityGroup> {
        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let stack = ComputeClusterMutation
            .mutate(
                generated_pool_stack(),
                &stack_state,
                &general_selection(selection),
            )
            .await?;
        let cluster = stack
            .resources
            .get("compute")
            .and_then(|entry| entry.config.downcast_ref::<ComputeCluster>())
            .expect("the mutation should generate the compute cluster");
        assert_eq!(cluster.capacity_groups.len(), 1);
        Ok(cluster.capacity_groups[0].clone())
    }

    #[tokio::test]
    async fn generated_pool_accepts_any_installer_count_within_limits() {
        let plan = plan_compute(&generated_pool_stack(), Platform::Aws, None)
            .expect("compute plan should build");
        let recommended = plan.pools[0].recommended.clone();
        assert_eq!(
            (recommended.min_size(), recommended.max_size()),
            (1, 2),
            "fixture should recommend autoscale 1-2"
        );
        let machine = recommended.machine().map(ToString::to_string);

        let fixed = |machines| ComputePoolSelection::Fixed {
            machines,
            machine: machine.clone(),
            failure_domains: None,
        };
        let autoscale = |min, max| ComputePoolSelection::Autoscale {
            min,
            max,
            machine: machine.clone(),
            failure_domains: None,
        };
        for (selection, expected) in [
            (fixed(1), (1, 1)),
            (fixed(2), (2, 2)),
            (autoscale(1, 1), (1, 1)),
            (autoscale(2, 2), (2, 2)),
            (autoscale(1, 2), (1, 2)),
            (autoscale(2, 10), (2, 10)),
            (autoscale(1, 100), (1, 100)),
            // No upper bound: only the workload minimum and capacity are checked.
            (autoscale(1, 150), (1, 150)),
            (fixed(150), (150, 150)),
        ] {
            let group = prepare_generated_pool(selection.clone())
                .await
                .unwrap_or_else(|error| panic!("{selection:?} should be accepted: {error}"));
            assert_eq!((group.min_size, group.max_size), expected, "{selection:?}");
            assert_eq!(group.instance_type, machine, "{selection:?}");
        }

        for (selection, message) in [
            (
                autoscale(0, 2),
                "autoscale minimum 0 is outside the allowed range 1-100",
            ),
            (
                fixed(0),
                "fixed compute pools must select at least one machine",
            ),
        ] {
            let error = prepare_generated_pool(selection.clone())
                .await
                .expect_err("a count below the workload minimum must be rejected");
            assert!(
                error.message.contains(message),
                "{selection:?}: {}",
                error.message
            );
        }
    }

    /// The `gw` container of a release, with half a vCPU and 1 GiB per replica.
    fn gw_release(replicas: ContainerReplicas) -> Stack {
        let mut gw = test_container("gw", "0.5", "1Gi");
        match replicas {
            ContainerReplicas::Fixed(count) => gw.replicas = Some(count),
            ContainerReplicas::Autoscale(min, max) => {
                gw.autoscaling = Some(ContainerAutoscaling {
                    min,
                    desired: min,
                    max,
                    target_cpu_percent: Some(70.0),
                    target_memory_percent: None,
                    target_http_in_flight_per_replica: None,
                    max_http_p95_latency_ms: None,
                })
            }
        }
        Stack::new("replica-change".to_string())
            .add(gw, ResourceLifecycle::Live)
            .build()
    }

    #[derive(Clone, Copy, Debug)]
    enum ContainerReplicas {
        Fixed(u32),
        Autoscale(u32, u32),
    }

    async fn prepare_release(
        stack: Stack,
        selection: &ComputePoolSelection,
    ) -> Result<CapacityGroup> {
        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let stack = ComputeClusterMutation
            .mutate(stack, &stack_state, &general_selection(selection.clone()))
            .await?;
        let cluster = stack
            .resources
            .get("compute")
            .and_then(|entry| entry.config.downcast_ref::<ComputeCluster>())
            .expect("the mutation should generate the compute cluster");
        Ok(cluster.capacity_groups[0].clone())
    }

    /// A release that changes replica counts keeps the deployer's saved pool choice, in both
    /// directions, for fixed and autoscaling pools. The planner's own recommendation moves
    /// with the replicas, and used to be the only accepted choice.
    #[tokio::test]
    async fn replica_changes_keep_the_saved_pool_choice() {
        let fixed_one = ComputePoolSelection::Fixed {
            machines: 1,
            machine: Some("m7g.large".to_string()),
            failure_domains: None,
        };
        let autoscale_one_two = ComputePoolSelection::Autoscale {
            min: 1,
            max: 2,
            machine: Some("m7g.large".to_string()),
            failure_domains: None,
        };
        let releases = [
            ContainerReplicas::Fixed(1),
            // Scale up: fixed replicas and autoscaling replicas past the saved maximum.
            ContainerReplicas::Fixed(2),
            ContainerReplicas::Autoscale(1, 3),
            ContainerReplicas::Autoscale(2, 6),
            // Scale back down.
            ContainerReplicas::Autoscale(1, 2),
            ContainerReplicas::Fixed(1),
        ];
        for (saved, expected) in [(&fixed_one, (1, 1)), (&autoscale_one_two, (1, 2))] {
            for replicas in releases {
                let plan = plan_compute(&gw_release(replicas), Platform::Aws, None)
                    .expect("compute plan should build");
                let recommended = &plan.pools[0].recommended;
                let group = prepare_release(gw_release(replicas), saved)
                    .await
                    .unwrap_or_else(|error| {
                        panic!(
                            "{replicas:?} (recommended {}-{}) with saved {saved:?}: {error}",
                            recommended.min_size(),
                            recommended.max_size()
                        )
                    });
                assert_eq!(
                    (group.min_size, group.max_size),
                    expected,
                    "{replicas:?} with saved {saved:?}"
                );
            }
        }
    }

    /// A saved choice above the planner's sizing cap stays valid when a later release changes
    /// the replica count, at any size.
    #[tokio::test]
    async fn large_saved_pool_survives_releases_that_change_replicas() {
        for saved_max in [12, 150] {
            let saved = ComputePoolSelection::Autoscale {
                min: 1,
                max: saved_max,
                machine: Some("m7g.large".to_string()),
                failure_domains: None,
            };
            for replicas in [saved_max, saved_max - 1, 3, 400] {
                let group = prepare_release(
                    gw_release(ContainerReplicas::Autoscale(1, replicas)),
                    &saved,
                )
                .await
                .unwrap_or_else(|error| {
                    panic!("saved 1-{saved_max}, {replicas} replicas: {error}")
                });
                assert_eq!(
                    (group.min_size, group.max_size),
                    (1, saved_max),
                    "saved 1-{saved_max}, {replicas} replicas"
                );
            }
        }

        // A recommendation above the ceiling is clamped, so it stays a valid choice.
        let plan = plan_compute(
            &gw_release(ContainerReplicas::Autoscale(1, 250)),
            Platform::Aws,
            None,
        )
        .expect("compute plan should build");
        assert_eq!(plan.pools[0].recommended.max_size(), 100);
        assert!(
            plan.pools[0].errors.is_empty(),
            "{:?}",
            plan.pools[0].errors
        );
    }

    /// A pool installed while the deployment stored no compute choice keeps its machine and
    /// counts on later releases, instead of taking whatever the catalog recommends now, and asks
    /// for setup once a release outgrows it. A stored choice without a machine stays Automatic.
    #[tokio::test]
    async fn installed_pool_without_a_compute_choice_keeps_its_machine() {
        let release = |cpu: &str, memory: &str| {
            Stack::new("installed-pool".to_string())
                .add(test_container("web", cpu, memory), ResourceLifecycle::Live)
                .build()
        };
        let config = |compute: Option<ComputeSettings>| {
            DeploymentConfig::builder()
                .stack_settings(StackSettings {
                    compute,
                    ..StackSettings::default()
                })
                .environment_variables(empty_env_snapshot())
                .allow_frozen_changes(false)
                .external_bindings(ExternalBindings::default())
                .build()
        };
        let one_machine = |machine: Option<&str>| ComputeSettings {
            containers: Default::default(),
            pools: [(
                "general".to_string(),
                ComputePoolSelection::Fixed {
                    machines: 1,
                    machine: machine.map(ToString::to_string),
                    failure_domains: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        let fresh_state = StackState {
            platform: Platform::Gcp,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        // The pool was installed on one e2-small, which today's catalog no longer recommends.
        let installed = ComputeClusterMutation
            .mutate(
                release("0.5", "512Mi"),
                &fresh_state,
                &config(Some(one_machine(Some("e2-small")))),
            )
            .await
            .expect("installing on e2-small should prepare");
        let recommended = plan_compute(&release("0.5", "512Mi"), Platform::Gcp, None)
            .expect("compute plan should build")
            .pools[0]
            .recommended
            .machine()
            .map(ToString::to_string);
        assert_ne!(
            recommended.as_deref(),
            Some("e2-small"),
            "fixture needs a recommendation that differs from the installed machine"
        );
        let mut stack_state = fresh_state.clone();
        for (id, entry) in &installed.resources {
            stack_state.resources.insert(
                id.clone(),
                alien_core::StackResourceState::new_pending(
                    entry.config.resource_type().to_string(),
                    entry.config.clone(),
                    Some(entry.lifecycle),
                    entry.dependencies.clone(),
                ),
            );
        }

        // Unchanged release, no stored choice: the prepared stack is the installed one.
        let unchanged = ComputeClusterMutation
            .mutate(release("0.5", "512Mi"), &stack_state, &config(None))
            .await
            .expect("an unchanged release should keep the installed pool");
        assert_eq!(
            serde_json::to_value(&unchanged).unwrap(),
            serde_json::to_value(&installed).unwrap(),
            "an unchanged release must not change the installed machine or counts"
        );

        // A release the installed machine cannot hold needs a compute choice from setup.
        let error = ComputeClusterMutation
            .mutate(release("2", "4Gi"), &stack_state, &config(None))
            .await
            .expect_err("a release that outgrows the installed machine must ask for setup");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED", "{}", error.message);
        assert!(
            error
                .message
                .contains("Select compute for gcp capacity group 'general'"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("the installed 1 x e2-small"),
            "{}",
            error.message
        );

        // A stored choice without a machine is Automatic: it follows the recommendation.
        let automatic = ComputeClusterMutation
            .mutate(
                release("0.5", "512Mi"),
                &stack_state,
                &config(Some(one_machine(None))),
            )
            .await
            .expect("an Automatic choice should prepare");
        let group = &automatic.resources["compute"]
            .config
            .downcast_ref::<ComputeCluster>()
            .expect("the compute cluster should be prepared")
            .capacity_groups[0];
        assert_eq!(group.instance_type, recommended);
        assert_eq!((group.min_size, group.max_size), (1, 1));
    }

    #[tokio::test]
    async fn replica_change_beyond_the_saved_fleet_is_refused_with_its_size() {
        let fixed_one = ComputePoolSelection::Fixed {
            machines: 1,
            machine: Some("m7g.large".to_string()),
            failure_domains: None,
        };
        // m7g.large has 1.5 vCPU available after host reserve; five replicas request 2.5.
        let error = prepare_release(gw_release(ContainerReplicas::Fixed(5)), &fixed_one)
            .await
            .expect_err("five replicas cannot fit one machine");
        assert!(
            error.message.contains(
                "Pool 'general' is too small for its workloads: 1 x m7g.large has 1.50 vCPU"
            ),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("request 2.50 vCPU"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn test_mutate_uses_platform_specific_sizing() {
        // Test Local platform
        let mut resources = IndexMap::new();
        let container = Container::new("api".to_string())
            // No .cluster() call = None = auto-assign
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

        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Local,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m7i.large", 1, 1);
        let result = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that Local platform gets min=1, max=1 with synthetic profile
        let cluster_entry = result.resources.get("compute").unwrap();
        let cluster = cluster_entry
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        let group = &cluster.capacity_groups[0];
        assert_eq!(group.min_size, 1);
        assert_eq!(group.max_size, 1);
        assert_eq!(group.instance_type, None);
        assert!(
            group.profile.is_some(),
            "Local platform should have a synthetic profile"
        );
    }

    #[tokio::test]
    async fn test_mutate_does_not_update_explicit_cluster() {
        let mut resources = IndexMap::new();

        // Add a container with an explicit cluster reference
        let container = Container::new("api".to_string())
            .cluster("my-custom-cluster".to_string()) // Explicit cluster
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

        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m7i.large", 1, 1);
        let result = mutation.mutate(stack, &stack_state, &config).await;

        assert!(result.is_ok());
        let mutated_stack = result.unwrap();

        // Check that cluster was created (mutation still runs)
        assert!(mutated_stack.resources.contains_key("compute"));

        // Check that container's explicit cluster was NOT changed
        let container_entry = mutated_stack.resources.get("api").unwrap();
        let container = container_entry.config.downcast_ref::<Container>().unwrap();
        assert_eq!(container.cluster, Some("my-custom-cluster".to_string()));
    }

    #[tokio::test]
    async fn test_mutate_adds_network_dependency_when_network_exists() {
        let mut resources = IndexMap::new();

        // Add a network resource (as created by NetworkMutation in Phase 1)
        let network = Network::new("default-network".to_string())
            .settings(NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            })
            .build();
        resources.insert(
            "default-network".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(network),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        // Add a container
        let container = Container::new("api".to_string())
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
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m7i.large", 1, 1);
        let result = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Cluster should depend on default-network since it exists
        let cluster_entry = result.resources.get("compute").unwrap();
        assert_eq!(cluster_entry.dependencies.len(), 1);
        assert_eq!(cluster_entry.dependencies[0].id, "default-network");
    }

    #[tokio::test]
    async fn test_mutate_no_network_dependency_when_network_absent() {
        // When default-network doesn't exist in the stack (e.g., testing the mutation
        // in isolation), the cluster should NOT add a dangling dependency.
        let mut resources = IndexMap::new();

        let container = Container::new("api".to_string())
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
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Gcp, // Cloud platform
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("n2-standard-2", 1, 1);
        let result = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Cluster should NOT depend on default-network since it doesn't exist
        let cluster_entry = result.resources.get("compute").unwrap();
        assert!(
            cluster_entry.dependencies.is_empty(),
            "Cluster should not have a dangling dependency on default-network"
        );
    }

    #[tokio::test]
    async fn test_mutate_populates_profile_for_all_cloud_platforms() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let mut resources = IndexMap::new();
            let container = Container::new("api".to_string())
                .code(ContainerCode::Image {
                    image: "test:latest".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "1".to_string(),
                    desired: "2".to_string(),
                })
                .memory(ResourceSpec {
                    min: "2Gi".to_string(),
                    desired: "4Gi".to_string(),
                })
                .port(8080)
                .permissions("test".to_string())
                .build();

            resources.insert(
                "api".to_string(),
                ResourceEntry {
                    config: alien_core::Resource::new(container),
                    lifecycle: ResourceLifecycle::Live,
                    dependencies: Vec::new(),
                    remote_access: false,
                    enabled_when: None,
                },
            );

            let stack = Stack {
                dynamic_container_repositories: Vec::new(),
                dynamic_container_image_resources: Vec::new(),
                operations: None,
                id: "test-stack".to_string(),
                resources,
                permissions: alien_core::permissions::PermissionsConfig::default(),
                supported_platforms: None,
                inputs: vec![],
            };

            let stack_state = StackState {
                platform,
                resources: Default::default(),
                resource_prefix: "test".to_string(),
            };

            let machine = match platform {
                Platform::Aws => "m7i.xlarge",
                Platform::Gcp => "n2-standard-4",
                Platform::Azure => "Standard_D4s_v5",
                _ => unreachable!("test only covers cloud platforms"),
            };
            let mutation = ComputeClusterMutation;
            let config = deployment_config_with_compute_pool(machine, 1, 1);
            let result = mutation
                .mutate(stack, &stack_state, &config)
                .await
                .unwrap_or_else(|e| panic!("mutation failed for {platform}: {e:?}"));

            let cluster_entry = result.resources.get("compute").unwrap();
            let cluster = cluster_entry
                .config
                .downcast_ref::<ComputeCluster>()
                .unwrap();
            let group = &cluster.capacity_groups[0];

            assert!(
                group.instance_type.is_some(),
                "instance_type should be set for {platform}"
            );
            assert!(
                group.profile.is_some(),
                "profile should be set for {platform}"
            );
            assert!(
                group.min_size >= 1,
                "min_size should be >= 1 for {platform}"
            );
            assert!(
                group.max_size >= group.min_size,
                "max_size should be >= min_size for {platform}"
            );
        }
    }

    #[tokio::test]
    async fn test_should_run_when_gpu_container_added_to_existing_cluster() {
        // Simulates: user adds a GPU container to a stack that already has a cluster
        // with only a "general" capacity group. The mutation should re-run to add "gpu".
        let mut resources = IndexMap::new();

        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7i.large".to_string()),
                profile: Some(MachineProfile {
                    cpu: "2.0".to_string(),
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    architecture: None,
                    gpu: None,
                }),
                min_size: 1,
                max_size: 10,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build();
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        // GPU container without explicit pool — mutation must detect missing "gpu" group
        let gpu_container = Container::new("ml-worker".to_string())
            .code(ContainerCode::Image {
                image: "ml:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "2".to_string(),
                desired: "4".to_string(),
            })
            .memory(ResourceSpec {
                min: "8Gi".to_string(),
                desired: "16Gi".to_string(),
            })
            .gpu(alien_core::ContainerGpuSpec {
                gpu_type: "nvidia-a100".to_string(),
                count: 1,
            })
            .permissions("ml".to_string())
            .build();
        resources.insert(
            "ml-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(gpu_container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };
        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let config = deployment_config_with_compute_pools(&[
            ("general", "m7i.large", 1, 10),
            ("gpu", "p4d.24xlarge", 1, 1),
        ]);

        let mutation = ComputeClusterMutation;
        assert!(
            mutation.should_run(&stack, &stack_state, &config),
            "should_run must return true when GPU container needs a missing 'gpu' capacity group"
        );

        let result = mutation.mutate(stack, &stack_state, &config).await.unwrap();
        let cluster_entry = result.resources.get("compute").unwrap();
        let cluster = cluster_entry
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        let group_ids: Vec<&str> = cluster
            .capacity_groups
            .iter()
            .map(|g| g.group_id.as_str())
            .collect();
        assert!(
            group_ids.contains(&"gpu"),
            "cluster should have a 'gpu' capacity group after mutation, got: {:?}",
            group_ids
        );

        let ml_entry = result.resources.get("ml-worker").unwrap();
        let ml = ml_entry.config.downcast_ref::<Container>().unwrap();
        assert_eq!(
            ml.pool.as_deref(),
            Some("gpu"),
            "GPU container should be assigned to 'gpu' pool"
        );
    }

    #[tokio::test]
    async fn test_mutate_aggregates_multiple_containers() {
        use alien_core::ContainerAutoscaling;

        let mut resources = IndexMap::new();

        // Container 1: small API with autoscaling
        let api = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "api:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "2Gi".to_string(),
            })
            .autoscaling(ContainerAutoscaling {
                min: 2,
                desired: 2,
                max: 10,
                target_cpu_percent: None,
                target_memory_percent: None,
                target_http_in_flight_per_replica: None,
                max_http_p95_latency_ms: None,
            })
            .port(8080)
            .permissions("api".to_string())
            .build();

        // Container 2: worker with fixed replicas
        let worker = Container::new("worker".to_string())
            .code(ContainerCode::Image {
                image: "worker:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "2".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "4Gi".to_string(),
            })
            .replicas(3)
            .port(9090)
            .permissions("worker".to_string())
            .build();

        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(api),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m7i.xlarge", 1, 8);
        let result = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        let cluster_entry = result.resources.get("compute").unwrap();
        let cluster = cluster_entry
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        let group = &cluster.capacity_groups[0];

        // Total workload: api (1 CPU * 10 replicas = 10 CPU) + worker (2 CPU * 3 replicas = 6 CPU) = 16 CPU
        // Should NOT select burstable (>= 2 CPU total)
        assert!(group.instance_type.is_some());
        assert!(group.profile.is_some());
        // Both containers referenced this cluster
        for id in ["api", "worker"] {
            let entry = result.resources.get(id).unwrap();
            let c = entry.config.downcast_ref::<Container>().unwrap();
            assert_eq!(
                c.cluster,
                Some("compute".to_string()),
                "container {id} should reference compute cluster"
            );
        }
    }

    /// Auto-generated clusters never carry a `nested_virtualization`
    /// constraint — the daemon's old `.nested_virtualization()` smuggle
    /// flag is gone. Customers who need nested virt must declare a
    /// ComputeCluster explicitly with a capacity group that sets it.
    /// This test guards the negative case to catch any future inference
    /// from drifting back in.
    #[tokio::test]
    async fn test_auto_generated_cluster_leaves_groups_unconstrained() {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "api:latest".to_string(),
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
            .permissions("api".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };
        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m7i.large", 1, 1);

        let result = mutation.mutate(stack, &stack_state, &config).await.unwrap();
        let cluster = result
            .resources
            .get("compute")
            .unwrap()
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();

        for group in &cluster.capacity_groups {
            assert_eq!(
                group.nested_virtualization, None,
                "auto-generated group '{}' should not be constrained",
                group.group_id
            );
        }
    }

    /// Customer-declared ComputeCluster with a capacity group that opts
    /// into `nested_virtualization` survives the preflight unchanged.
    /// The preflight must not blow away or rewrite customer-supplied
    /// settings on an already-declared cluster.
    #[tokio::test]
    async fn test_customer_declared_capacity_group_nested_virt_survives() {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "api:latest".to_string(),
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
            .permissions("api".to_string())
            .build();

        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m8i.xlarge".to_string()),
                profile: Some(MachineProfile {
                    cpu: "4.0".to_string(),
                    memory_bytes: 16 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    architecture: None,
                    gpu: None,
                }),
                min_size: 1,
                max_size: 1,
                scale_policy: None,
                nested_virtualization: Some(true),
            })
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };
        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m8i.xlarge", 1, 1);

        let result = mutation.mutate(stack, &stack_state, &config).await.unwrap();
        let cluster = result
            .resources
            .get("compute")
            .unwrap()
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();

        let group = cluster
            .capacity_groups
            .iter()
            .find(|g| g.group_id == "general")
            .expect("customer-declared 'general' group must survive");
        assert_eq!(
            group.nested_virtualization,
            Some(true),
            "customer-declared nested_virtualization must not be overwritten"
        );
        assert_eq!(
            group.instance_type.as_deref(),
            Some("m8i.xlarge"),
            "customer-declared instance_type must not be overwritten"
        );
    }

    #[tokio::test]
    async fn test_daemon_explicit_cluster_materializes_deployment_compute_selection() {
        let daemon = Daemon::new("loader".to_string())
            .code(DaemonCode::Image {
                image: "loader:latest".to_string(),
            })
            .cluster("custom-runtime".to_string())
            .permissions("loader".to_string())
            .build();

        let cluster = ComputeCluster::new("custom-runtime".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 0,
                max_size: 0,
                scale_policy: Some(CapacityGroupScalePolicy::Fixed {
                    machines: ComputeChoiceRange {
                        min: 0,
                        max: 5,
                        default: 0,
                    },
                }),
                nested_virtualization: None,
            })
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "loader".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(daemon),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "custom-runtime".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };
        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };
        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m8i.xlarge", 2, 2);

        assert!(
            mutation.should_run(&stack, &stack_state, &config),
            "explicit daemon clusters must run when provider compute settings need materialization"
        );

        let result = mutation.mutate(stack, &stack_state, &config).await.unwrap();
        let cluster = result
            .resources
            .get("custom-runtime")
            .unwrap()
            .config
            .downcast_ref::<ComputeCluster>()
            .unwrap();
        let group = cluster
            .capacity_groups
            .iter()
            .find(|g| g.group_id == "general")
            .expect("general capacity group must survive");

        assert_eq!(group.instance_type.as_deref(), Some("m8i.xlarge"));
        assert!(group.profile.is_some());
        assert_eq!(group.min_size, 2);
        assert_eq!(group.max_size, 2);
    }
}
