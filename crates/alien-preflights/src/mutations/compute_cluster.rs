//! ComputeCluster mutation that auto-generates ComputeCluster resources for Container workloads.
//!
//! When containers are defined in the stack without an explicit ComputeCluster,
//! this mutation creates a default cluster to host them. It analyzes container
//! resource requirements and selects an appropriate instance type and machine profile
//! using the instance catalog.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    compute_planner::{capacity_group_requirements, validate_compute_pool_selection},
    instance_catalog::{self, WorkloadRequirements},
    CapacityGroup, CapacityGroupScalePolicy, ComputeCluster, Container, Daemon, DeploymentConfig,
    MachineProfile, Network, Platform, ResourceEntry, ResourceLifecycle, ResourceRef, Stack,
    StackState,
};
use alien_error::AlienError;
use async_trait::async_trait;
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
        if stack_state.platform == Platform::Kubernetes {
            return false;
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

        self.materialize_capacity_groups(stack, stack_state, config)
    }
}

impl ComputeClusterMutation {
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

        for entry in stack.resources.values_mut() {
            let Some(cluster) = entry.config.downcast_mut::<ComputeCluster>() else {
                continue;
            };
            for group in cluster.capacity_groups.iter_mut() {
                materialize_group(group, stack_state.platform, config)?;
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
                    if c.pool.is_some() {
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
                .filter(|c| c.pool.is_none() && needed_capacity_group(c) == group_id.as_str())
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

fn materialize_group(
    group: &mut CapacityGroup,
    platform: Platform,
    config: &DeploymentConfig,
) -> Result<()> {
    let settings = config.stack_settings.compute.as_ref().ok_or_else(|| {
        AlienError::new(crate::error::ErrorData::StackMutationFailed {
            mutation_name: "ComputeClusterMutation".to_string(),
            message: format!(
                "Compute selection is required for {} capacity group '{}'",
                platform, group.group_id
            ),
            resource_id: None,
        })
    })?;
    let selection = settings.pools.get(&group.group_id).ok_or_else(|| {
        AlienError::new(crate::error::ErrorData::StackMutationFailed {
            mutation_name: "ComputeClusterMutation".to_string(),
            message: format!(
                "Missing compute selection for capacity group '{}'",
                group.group_id
            ),
            resource_id: None,
        })
    })?;
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
    let scale = group.scale_policy.clone().unwrap_or_else(|| {
        CapacityGroupScalePolicy::from_selected_bounds(group.min_size, group.max_size)
    });
    let requirements = capacity_group_requirements(group);
    let errors = validate_compute_pool_selection(
        platform,
        &group.group_id,
        selection,
        &requirements,
        &scale,
    );
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
    group.instance_type = Some(machine.to_string());
    group.profile = Some(spec.to_machine_profile());
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

/// Build capacity groups categorized by hardware type.
fn build_categorized_capacity_groups(
    containers: &[&Container],
    platform: Platform,
    needs_nested_virt: bool,
    config: &DeploymentConfig,
) -> Result<Vec<CapacityGroup>> {
    match platform {
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            let mut general: Vec<&Container> = vec![];
            let mut storage: Vec<&Container> = vec![];
            let mut gpu: Vec<&Container> = vec![];
            for c in containers {
                match needed_capacity_group(c) {
                    "gpu" => gpu.push(c),
                    "storage" => storage.push(c),
                    _ => general.push(c),
                }
            }
            let mut groups = vec![];
            if !general.is_empty() || (gpu.is_empty() && storage.is_empty()) {
                groups.push(build_capacity_group_for_id(
                    "general",
                    &general,
                    platform,
                    needs_nested_virt,
                    config,
                )?);
            }
            if !storage.is_empty() {
                groups.push(build_capacity_group_for_id(
                    "storage",
                    &storage,
                    platform,
                    needs_nested_virt,
                    config,
                )?);
            }
            if !gpu.is_empty() {
                groups.push(build_capacity_group_for_id(
                    "gpu",
                    &gpu,
                    platform,
                    needs_nested_virt,
                    config,
                )?);
            }
            Ok(groups)
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
    let effective = if group_id == "gpu" && requirements.gpu.is_none() {
        WorkloadRequirements {
            gpu: Some(alien_core::GpuSpec {
                gpu_type: "any".to_string(),
                count: 1,
            }),
            ..requirements
        }
    } else {
        requirements
    };
    let mut group = CapacityGroup {
        group_id: group_id.to_string(),
        instance_type: None,
        profile: None,
        min_size: default_min_machines(&effective),
        max_size: default_max_machines(&effective),
        scale_policy: None,
        nested_virtualization: None,
    };
    if matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        materialize_group(&mut group, platform, config)?;
    } else {
        group.profile = Some(MachineProfile {
            cpu: format!(
                "{}.0",
                effective.max_cpu_per_container.max(1.0).ceil() as u32
            ),
            memory_bytes: effective
                .max_memory_per_container
                .max(2 * 1024 * 1024 * 1024),
            ephemeral_storage_bytes: effective
                .max_ephemeral_storage_bytes
                .max(20 * 1024 * 1024 * 1024),
            architecture: effective.architecture,
            gpu: effective.gpu,
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
        ComputeChoiceRange, ComputePoolSelection, ComputeSettings, ContainerAutoscaling,
        ContainerCode, DaemonCode, EnvironmentVariablesSnapshot, ExternalBindings, NetworkSettings,
        ResourceSpec, StackSettings,
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
                    pools: selections
                        .iter()
                        .map(|(pool_id, machine, min_size, max_size)| {
                            let selection = if min_size == max_size {
                                ComputePoolSelection::Fixed {
                                    machines: *min_size,
                                    machine: Some((*machine).to_string()),
                                }
                            } else {
                                ComputePoolSelection::Autoscale {
                                    min: *min_size,
                                    max: *max_size,
                                    machine: Some((*machine).to_string()),
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
            (Platform::Aws, "m7g.xlarge"),
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
            },
        );

        let stack = Stack {
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
                instance_type: Some("m7g.large".to_string()),
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
            },
        );

        let stack = Stack {
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
            },
        );

        let stack = Stack {
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
            },
        );

        let stack = Stack {
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
        let config = deployment_config_with_compute_pool("m7g.large", 1, 1);
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
            },
        );

        let stack = Stack {
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
        let config = deployment_config_with_compute_pool("m7g.large", 1, 1);
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
            },
        );

        let stack = Stack {
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
        let config = deployment_config_with_compute_pool("m7g.large", 1, 1);
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
            },
        );

        let stack = Stack {
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
        let config = deployment_config_with_compute_pool("m7g.large", 1, 1);
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
            },
        );

        let stack = Stack {
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
                },
            );

            let stack = Stack {
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
                Platform::Aws => "m7g.large",
                Platform::Gcp => "n2-standard-2",
                Platform::Azure => "Standard_D2s_v5",
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
                instance_type: Some("m7g.large".to_string()),
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
            },
        );

        let stack = Stack {
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
            ("general", "m7g.large", 1, 10),
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
            },
        );
        resources.insert(
            "worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
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

        let stack_state = StackState {
            platform: Platform::Aws,
            resources: Default::default(),
            resource_prefix: "test".to_string(),
        };

        let mutation = ComputeClusterMutation;
        let config = deployment_config_with_compute_pool("m7g.xlarge", 1, 8);
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
            },
        );

        let stack = Stack {
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
        let config = deployment_config_with_compute_pool("m7g.large", 1, 1);

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
            },
        );
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
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
            },
        );
        resources.insert(
            "custom-runtime".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
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
