//! Deployment-time compute planner.
//!
//! The planner turns portable stack requirements plus a target platform into a
//! renderable set of recommended deployment choices. It does not mutate the
//! stack and does not require database access.

use crate::{
    instance_catalog::{self, WorkloadRequirements},
    ComputePoolSelection, Container, Daemon, ErrorData, GpuSpec, MachineProfile, Platform,
    ResourceSpec, Stack,
};
use alien_error::{AlienError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Full compute plan for one stack/platform pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputePlan {
    /// Planned pools in stable pool-id order.
    pub pools: Vec<ComputePoolPlan>,
}

/// Planner output for one compute pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputePoolPlan {
    /// Pool ID from the stack or derived default.
    pub pool_id: String,
    /// Workloads assigned to this pool.
    pub workloads: Vec<String>,
    /// Aggregated requirements used for machine selection.
    pub requirements: MachineProfile,
    /// Recommended or user-selected deployment choice.
    pub selected: ComputePoolSelection,
    /// Planner-recommended default.
    pub recommended: ComputePoolSelection,
    /// Valid cloud machine choices. Empty for local and Kubernetes.
    pub machines: Vec<ComputeMachineOption>,
    /// Validation errors for supplied deployment settings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// One concrete provider machine option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ComputeMachineOption {
    /// Provider machine name.
    pub machine: String,
    /// Machine hardware profile.
    pub profile: MachineProfile,
    /// Whether this machine is the planner's default recommendation.
    pub recommended: bool,
}

/// Compute a deterministic deployment-time plan.
pub fn plan_compute(
    stack: &Stack,
    platform: Platform,
    selected_settings: Option<&crate::ComputeSettings>,
) -> Result<ComputePlan, ErrorData> {
    let mut groups = collect_workload_groups(stack)?;
    merge_explicit_compute_groups(stack, &mut groups)?;

    let mut pool_ids: Vec<String> = groups.keys().cloned().collect();
    pool_ids.sort();

    let mut pools = Vec::new();
    for pool_id in pool_ids {
        let group = groups.remove(&pool_id).expect("pool id came from map keys");
        let requirements = group.requirements;
        let selected = selected_settings.and_then(|settings| settings.pools.get(&pool_id));
        let recommended =
            recommended_selection(platform, &requirements, group.min_size, group.max_size)?;
        let selected_choice = selected.cloned().unwrap_or_else(|| recommended.clone());
        let errors = validate_selection(platform, &pool_id, &selected_choice, &requirements);
        let machines = machine_options(platform, &requirements, selected_choice.machine())?;

        pools.push(ComputePoolPlan {
            pool_id,
            workloads: group.workloads,
            requirements: requirements_to_profile(&requirements),
            selected: selected_choice,
            recommended,
            machines,
            errors,
        });
    }

    Ok(ComputePlan { pools })
}

#[derive(Debug, Clone)]
struct PlannedGroup {
    workloads: Vec<String>,
    requirements: WorkloadRequirements,
    min_size: u32,
    max_size: u32,
}

fn collect_workload_groups(stack: &Stack) -> Result<HashMap<String, PlannedGroup>, ErrorData> {
    let mut groups: HashMap<String, Vec<Workload>> = HashMap::new();

    for entry in stack.resources.values() {
        if let Some(container) = entry.config.downcast_ref::<Container>() {
            groups
                .entry(
                    container
                        .pool
                        .clone()
                        .unwrap_or_else(|| needed_container_pool(container).to_string()),
                )
                .or_default()
                .push(Workload::from_container(container)?);
        }
        if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
            if daemon.cluster.is_some() {
                groups
                    .entry(daemon.pool.clone().unwrap_or_else(|| "general".to_string()))
                    .or_default()
                    .push(Workload::from_daemon(daemon)?);
            }
        }
    }

    let mut planned = HashMap::new();
    for (pool_id, workloads) in groups {
        let requirements = aggregate_workloads(&workloads);
        planned.insert(
            pool_id,
            PlannedGroup {
                workloads: workloads.into_iter().map(|w| w.id).collect(),
                min_size: default_min_machines(&requirements),
                max_size: default_max_machines(&requirements),
                requirements,
            },
        );
    }
    if planned.is_empty() {
        let requirements = default_requirements();
        planned.insert(
            "general".to_string(),
            PlannedGroup {
                workloads: Vec::new(),
                min_size: 1,
                max_size: 1,
                requirements,
            },
        );
    }
    Ok(planned)
}

fn merge_explicit_compute_groups(
    stack: &Stack,
    groups: &mut HashMap<String, PlannedGroup>,
) -> Result<(), ErrorData> {
    for entry in stack.resources.values() {
        let Some(cluster) = entry.config.downcast_ref::<crate::ComputeCluster>() else {
            continue;
        };
        for group in &cluster.capacity_groups {
            groups.entry(group.group_id.clone()).or_insert_with(|| {
                let requirements = profile_to_requirements(
                    group.profile.as_ref(),
                    group.nested_virtualization.unwrap_or(false),
                );
                PlannedGroup {
                    workloads: Vec::new(),
                    min_size: group.min_size,
                    max_size: group.max_size,
                    requirements,
                }
            });
        }
    }
    Ok(())
}

fn recommended_selection(
    platform: Platform,
    requirements: &WorkloadRequirements,
    min_size: u32,
    max_size: u32,
) -> Result<ComputePoolSelection, ErrorData> {
    let machine = match platform {
        Platform::Aws | Platform::Gcp | Platform::Azure => Some(
            instance_catalog::select_instance_type(platform, requirements)
                .map_err(|message| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("Failed to select {platform} machine: {message}"),
                    })
                })?
                .instance_type
                .to_string(),
        ),
        Platform::Local | Platform::Kubernetes | Platform::Test => None,
    };

    if min_size == max_size {
        Ok(ComputePoolSelection::Fixed {
            machines: min_size.max(1),
            machine,
        })
    } else {
        Ok(ComputePoolSelection::Autoscale {
            min: min_size,
            max: max_size.max(min_size),
            machine,
        })
    }
}

fn validate_selection(
    platform: Platform,
    pool_id: &str,
    selection: &ComputePoolSelection,
    requirements: &WorkloadRequirements,
) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(message) = selection.validate() {
        errors.push(message);
    }
    if matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        match selection.machine() {
            Some(machine) => match instance_catalog::find_instance_type(platform, machine) {
                Some(spec) => {
                    if !instance_satisfies(spec, requirements) {
                        errors.push(format!(
                            "{} machine '{}' does not satisfy pool '{}' requirements",
                            platform, machine, pool_id
                        ));
                    }
                }
                None => errors.push(format!(
                    "Unknown {} machine '{}' for pool '{}'",
                    platform, machine, pool_id
                )),
            },
            None => errors.push(format!(
                "Pool '{}' requires a provider machine on {}",
                pool_id, platform
            )),
        }
    }
    errors
}

fn machine_options(
    platform: Platform,
    requirements: &WorkloadRequirements,
    selected_machine: Option<&str>,
) -> Result<Vec<ComputeMachineOption>, ErrorData> {
    if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        return Ok(Vec::new());
    }
    let recommended = instance_catalog::select_instance_type(platform, requirements)
        .map_err(|message| {
            AlienError::new(ErrorData::GenericError {
                message: format!("Failed to select {platform} machine: {message}"),
            })
        })?
        .instance_type
        .to_string();

    let mut options: Vec<ComputeMachineOption> = instance_catalog::catalog_for_platform(platform)
        .into_iter()
        .filter(|spec| instance_satisfies(spec, requirements))
        .map(|spec| ComputeMachineOption {
            machine: spec.name.to_string(),
            profile: spec.to_machine_profile(),
            recommended: spec.name == recommended || Some(spec.name) == selected_machine,
        })
        .collect();
    options.sort_by(|a, b| a.machine.cmp(&b.machine));
    Ok(options)
}

fn instance_satisfies(
    spec: &instance_catalog::InstanceTypeSpec,
    requirements: &WorkloadRequirements,
) -> bool {
    if requirements.nested_virt && !spec.is_nested_virt_capable() {
        return false;
    }
    if spec.vcpu < requirements.max_cpu_per_container.ceil() as u32 {
        return false;
    }
    if spec.memory_bytes < requirements.max_memory_per_container {
        return false;
    }
    if spec.ephemeral_storage_bytes < requirements.max_ephemeral_storage_bytes {
        return false;
    }
    match (&requirements.gpu, spec.gpu) {
        (Some(required), Some(actual)) => {
            (required.gpu_type == "any" || required.gpu_type == actual.gpu_type)
                && actual.count >= required.count
        }
        (Some(_), None) => false,
        (None, _) => true,
    }
}

#[derive(Debug, Clone)]
struct Workload {
    id: String,
    cpu: f64,
    memory_bytes: u64,
    desired_replicas: f64,
    max_replicas: f64,
    ephemeral_storage_bytes: u64,
    gpu: Option<GpuSpec>,
}

impl Workload {
    fn from_container(container: &Container) -> Result<Self, ErrorData> {
        let cpu = parse_cpu(&container.id, &container.cpu)?;
        let memory_bytes = parse_memory(&container.id, &container.memory)?;
        let desired_replicas = container
            .autoscaling
            .as_ref()
            .map(|a| a.desired)
            .or(container.replicas)
            .unwrap_or(1) as f64;
        let max_replicas = container
            .autoscaling
            .as_ref()
            .map(|a| a.max)
            .or(container.replicas)
            .unwrap_or(1) as f64;
        let ephemeral_storage_bytes = container
            .ephemeral_storage
            .as_deref()
            .map(instance_catalog::parse_memory_bytes)
            .transpose()
            .map_err(|message| {
                AlienError::new(ErrorData::GenericError {
                    message: format!(
                        "Failed to parse ephemeral storage for '{}': {message}",
                        container.id
                    ),
                })
            })?
            .unwrap_or(0);

        Ok(Self {
            id: container.id.clone(),
            cpu,
            memory_bytes,
            desired_replicas,
            max_replicas,
            ephemeral_storage_bytes,
            gpu: container.gpu.as_ref().map(|gpu| GpuSpec {
                gpu_type: gpu.gpu_type.clone(),
                count: gpu.count,
            }),
        })
    }

    fn from_daemon(daemon: &Daemon) -> Result<Self, ErrorData> {
        Ok(Self {
            id: daemon.id.clone(),
            cpu: parse_cpu(&daemon.id, &daemon.cpu)?,
            memory_bytes: parse_memory(&daemon.id, &daemon.memory)?,
            desired_replicas: 1.0,
            max_replicas: 1.0,
            ephemeral_storage_bytes: 0,
            gpu: None,
        })
    }
}

fn parse_cpu(resource_id: &str, spec: &ResourceSpec) -> Result<f64, ErrorData> {
    instance_catalog::parse_cpu(&spec.desired).map_err(|message| {
        AlienError::new(ErrorData::GenericError {
            message: format!(
                "Failed to parse CPU requirement '{}' for '{}': {message}",
                spec.desired, resource_id
            ),
        })
    })
}

fn parse_memory(resource_id: &str, spec: &ResourceSpec) -> Result<u64, ErrorData> {
    instance_catalog::parse_memory_bytes(&spec.desired).map_err(|message| {
        AlienError::new(ErrorData::GenericError {
            message: format!(
                "Failed to parse memory requirement '{}' for '{}': {message}",
                spec.desired, resource_id
            ),
        })
    })
}

fn aggregate_workloads(workloads: &[Workload]) -> WorkloadRequirements {
    let mut requirements = default_requirements();
    requirements.total_cpu_at_desired = 0.0;
    requirements.total_memory_bytes_at_desired = 0;
    requirements.total_cpu_at_max = 0.0;
    requirements.total_memory_bytes_at_max = 0;
    requirements.max_cpu_per_container = 0.0;
    requirements.max_memory_per_container = 0;
    requirements.max_ephemeral_storage_bytes = 0;
    requirements.gpu = None;

    for workload in workloads {
        requirements.total_cpu_at_desired += workload.cpu * workload.desired_replicas;
        requirements.total_cpu_at_max += workload.cpu * workload.max_replicas;
        requirements.total_memory_bytes_at_desired +=
            (workload.memory_bytes as f64 * workload.desired_replicas) as u64;
        requirements.total_memory_bytes_at_max +=
            (workload.memory_bytes as f64 * workload.max_replicas) as u64;
        requirements.max_cpu_per_container = requirements.max_cpu_per_container.max(workload.cpu);
        requirements.max_memory_per_container = requirements
            .max_memory_per_container
            .max(workload.memory_bytes);
        requirements.max_ephemeral_storage_bytes = requirements
            .max_ephemeral_storage_bytes
            .max(workload.ephemeral_storage_bytes);
        if requirements.gpu.is_none() {
            requirements.gpu = workload.gpu.clone();
        }
    }
    requirements
}

fn default_requirements() -> WorkloadRequirements {
    WorkloadRequirements {
        total_cpu_at_desired: 1.0,
        total_memory_bytes_at_desired: 2 * 1024 * 1024 * 1024,
        total_cpu_at_max: 1.0,
        total_memory_bytes_at_max: 2 * 1024 * 1024 * 1024,
        max_cpu_per_container: 1.0,
        max_memory_per_container: 2 * 1024 * 1024 * 1024,
        max_ephemeral_storage_bytes: 0,
        gpu: None,
        nested_virt: false,
    }
}

fn profile_to_requirements(
    profile: Option<&MachineProfile>,
    nested_virt: bool,
) -> WorkloadRequirements {
    let Some(profile) = profile else {
        return WorkloadRequirements {
            nested_virt,
            ..default_requirements()
        };
    };
    let cpu = instance_catalog::parse_cpu(&profile.cpu).unwrap_or(1.0);
    WorkloadRequirements {
        total_cpu_at_desired: cpu,
        total_memory_bytes_at_desired: profile.memory_bytes,
        total_cpu_at_max: cpu,
        total_memory_bytes_at_max: profile.memory_bytes,
        max_cpu_per_container: cpu,
        max_memory_per_container: profile.memory_bytes,
        max_ephemeral_storage_bytes: profile.ephemeral_storage_bytes,
        gpu: profile.gpu.clone(),
        nested_virt,
    }
}

fn requirements_to_profile(requirements: &WorkloadRequirements) -> MachineProfile {
    MachineProfile {
        cpu: requirements.max_cpu_per_container.to_string(),
        memory_bytes: requirements.max_memory_per_container,
        ephemeral_storage_bytes: requirements.max_ephemeral_storage_bytes,
        gpu: requirements.gpu.clone(),
    }
}

fn needed_container_pool(container: &Container) -> &'static str {
    if container.gpu.is_some() {
        return "gpu";
    }
    if let Some(storage) = &container.ephemeral_storage {
        if instance_catalog::parse_memory_bytes(storage).unwrap_or(0) > 200 * 1024 * 1024 * 1024 {
            return "storage";
        }
    }
    "general"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ComputeSettings, ContainerCode, Resource, ResourceEntry, ResourceLifecycle, Stack,
    };

    fn stack_with_container() -> Stack {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "api:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "2".to_string(),
            })
            .memory(ResourceSpec {
                min: "2Gi".to_string(),
                desired: "4Gi".to_string(),
            })
            .permissions("api".to_string())
            .build();
        Stack {
            id: "test".to_string(),
            resources: [(
                "api".to_string(),
                ResourceEntry {
                    config: Resource::new(container),
                    lifecycle: ResourceLifecycle::Live,
                    dependencies: Vec::new(),
                    remote_access: false,
                },
            )]
            .into_iter()
            .collect(),
            permissions: crate::permissions::PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    #[test]
    fn cloud_plan_recommends_provider_machine_without_mutating_selection() {
        let stack = stack_with_container();

        let plan = plan_compute(&stack, Platform::Aws, None).expect("plan should build");

        let pool = plan.pools.first().expect("general pool should exist");
        assert_eq!(pool.pool_id, "general");
        assert_eq!(pool.workloads, vec!["api"]);
        assert!(pool.selected.machine().is_some());
        assert!(pool.machines.iter().any(|machine| machine.recommended));
    }

    #[test]
    fn selected_machine_is_preserved_as_static_deployment_choice() {
        let stack = stack_with_container();
        let settings = ComputeSettings {
            pools: [(
                "general".to_string(),
                ComputePoolSelection::Autoscale {
                    min: 2,
                    max: 4,
                    machine: Some("m7g.xlarge".to_string()),
                },
            )]
            .into_iter()
            .collect(),
        };

        let plan = plan_compute(&stack, Platform::Aws, Some(&settings)).expect("plan should build");

        let pool = plan.pools.first().expect("general pool should exist");
        assert_eq!(pool.selected.machine(), Some("m7g.xlarge"));
        assert!(pool.errors.is_empty());
    }

    #[test]
    fn local_plan_has_no_provider_machine_choices() {
        let stack = stack_with_container();

        let plan = plan_compute(&stack, Platform::Local, None).expect("plan should build");

        let pool = plan.pools.first().expect("general pool should exist");
        assert_eq!(pool.selected.machine(), None);
        assert!(pool.machines.is_empty());
        assert!(pool.errors.is_empty());
    }
}
