//! Deployment-time compute planner.
//!
//! The planner turns portable stack requirements plus a target platform into a
//! renderable set of recommended deployment choices. It does not mutate the
//! stack and does not require database access.

use crate::{
    instance_catalog::{self, WorkloadRequirements},
    CapacityGroup, CapacityGroupScalePolicy, ComputeChoiceRange, ComputePoolSelection, Container,
    Daemon, ErrorData, GpuSpec, MachineProfile, Platform, ResourceSpec, Stack,
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
    /// Allowed scale policy declared by source or derived for generated pools.
    pub scale: CapacityGroupScalePolicy,
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
        let recommended = recommended_selection(platform, &requirements, &group.scale)?;
        let selected_choice = selected.cloned().unwrap_or_else(|| recommended.clone());
        let errors = validate_compute_pool_selection(
            platform,
            &pool_id,
            &selected_choice,
            &requirements,
            &group.scale,
        );
        let machines = machine_options(platform, &requirements, selected_choice.machine())?;

        pools.push(ComputePoolPlan {
            pool_id,
            workloads: group.workloads,
            requirements: requirements_to_profile(&requirements),
            scale: group.scale,
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
    scale: CapacityGroupScalePolicy,
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
        let min_size = default_min_machines(&requirements);
        let max_size = default_max_machines(&requirements);
        planned.insert(
            pool_id,
            PlannedGroup {
                workloads: workloads.into_iter().map(|w| w.id).collect(),
                scale: CapacityGroupScalePolicy::from_selected_bounds(min_size, max_size),
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
                scale: CapacityGroupScalePolicy::from_selected_bounds(1, 1),
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
            let explicit_requirements = profile_to_requirements(
                group.profile.as_ref(),
                group.nested_virtualization.unwrap_or(false),
            );
            let scale = group.scale_policy.clone().unwrap_or_else(|| {
                CapacityGroupScalePolicy::from_selected_bounds(group.min_size, group.max_size)
            });
            groups
                .entry(group.group_id.clone())
                .and_modify(|planned| {
                    merge_requirements(&mut planned.requirements, &explicit_requirements);
                    planned.scale = merge_scale_policy(&planned.scale, &scale);
                })
                .or_insert_with(|| PlannedGroup {
                    workloads: Vec::new(),
                    scale,
                    requirements: explicit_requirements,
                });
        }
    }
    Ok(())
}

fn recommended_selection(
    platform: Platform,
    requirements: &WorkloadRequirements,
    scale: &CapacityGroupScalePolicy,
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
        Platform::Local | Platform::Kubernetes | Platform::Machines | Platform::Test => None,
    };

    match scale {
        CapacityGroupScalePolicy::Fixed { machines } => Ok(ComputePoolSelection::Fixed {
            machines: machines.default.max(1),
            machine,
            failure_domains: None,
        }),
        CapacityGroupScalePolicy::Autoscale { min, max } => Ok(ComputePoolSelection::Autoscale {
            min: min.default,
            max: max.default.max(min.default),
            machine,
            failure_domains: None,
        }),
    }
}

/// Validate one selected compute pool against platform machine requirements and
/// source-declared scale bounds.
pub fn validate_compute_pool_selection(
    platform: Platform,
    pool_id: &str,
    selection: &ComputePoolSelection,
    requirements: &WorkloadRequirements,
    scale: &CapacityGroupScalePolicy,
) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(message) = selection.validate() {
        errors.push(message);
    }
    if let Err(message) = validate_selection_against_scale(selection, scale) {
        errors.push(format!("Pool '{pool_id}' {message}"));
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

/// Convert a capacity group declaration into planner requirements.
pub fn capacity_group_requirements(group: &CapacityGroup) -> WorkloadRequirements {
    profile_to_requirements(
        group.profile.as_ref(),
        group.nested_virtualization.unwrap_or(false),
    )
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
    if spec.architecture
        != requirements
            .architecture
            .unwrap_or(instance_catalog::Architecture::X86_64)
    {
        return false;
    }
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
        architecture: None,
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
        architecture: profile.architecture,
        nested_virt,
    }
}

fn merge_requirements(existing: &mut WorkloadRequirements, declared: &WorkloadRequirements) {
    existing.total_cpu_at_desired = existing
        .total_cpu_at_desired
        .max(declared.total_cpu_at_desired);
    existing.total_memory_bytes_at_desired = existing
        .total_memory_bytes_at_desired
        .max(declared.total_memory_bytes_at_desired);
    existing.total_cpu_at_max = existing.total_cpu_at_max.max(declared.total_cpu_at_max);
    existing.total_memory_bytes_at_max = existing
        .total_memory_bytes_at_max
        .max(declared.total_memory_bytes_at_max);
    existing.max_cpu_per_container = existing
        .max_cpu_per_container
        .max(declared.max_cpu_per_container);
    existing.max_memory_per_container = existing
        .max_memory_per_container
        .max(declared.max_memory_per_container);
    existing.max_ephemeral_storage_bytes = existing
        .max_ephemeral_storage_bytes
        .max(declared.max_ephemeral_storage_bytes);
    if existing.gpu.is_none() {
        existing.gpu = declared.gpu.clone();
    }
    existing.nested_virt |= declared.nested_virt;
    if existing.architecture.is_none() {
        existing.architecture = declared.architecture;
    }
}

fn requirements_to_profile(requirements: &WorkloadRequirements) -> MachineProfile {
    MachineProfile {
        cpu: requirements.max_cpu_per_container.to_string(),
        memory_bytes: requirements.max_memory_per_container,
        ephemeral_storage_bytes: requirements.max_ephemeral_storage_bytes,
        architecture: requirements.architecture,
        gpu: requirements.gpu.clone(),
    }
}

fn merge_scale_policy(
    existing: &CapacityGroupScalePolicy,
    declared: &CapacityGroupScalePolicy,
) -> CapacityGroupScalePolicy {
    match (existing, declared) {
        (
            CapacityGroupScalePolicy::Fixed {
                machines: existing_machines,
            },
            CapacityGroupScalePolicy::Fixed {
                machines: declared_machines,
            },
        ) => CapacityGroupScalePolicy::Fixed {
            machines: merge_choice_range(existing_machines, declared_machines),
        },
        (_, declared) => declared.clone(),
    }
}

fn merge_choice_range(
    existing: &ComputeChoiceRange,
    declared: &ComputeChoiceRange,
) -> ComputeChoiceRange {
    ComputeChoiceRange {
        min: existing.min.max(declared.min),
        max: existing.max.max(declared.max),
        default: declared.default,
    }
}

fn validate_selection_against_scale(
    selection: &ComputePoolSelection,
    scale: &CapacityGroupScalePolicy,
) -> std::result::Result<(), String> {
    match (selection, scale) {
        (
            ComputePoolSelection::Fixed { machines, .. },
            CapacityGroupScalePolicy::Fixed { machines: allowed },
        ) => {
            if allowed.contains(*machines) {
                Ok(())
            } else {
                Err(format!(
                    "fixed machine count {machines} is outside the allowed range {}-{}",
                    allowed.min, allowed.max
                ))
            }
        }
        (
            ComputePoolSelection::Autoscale { min, max, .. },
            CapacityGroupScalePolicy::Autoscale {
                min: allowed_min,
                max: allowed_max,
            },
        ) => {
            if !allowed_min.contains(*min) {
                return Err(format!(
                    "autoscale minimum {min} is outside the allowed range {}-{}",
                    allowed_min.min, allowed_min.max
                ));
            }
            if !allowed_max.contains(*max) {
                return Err(format!(
                    "autoscale maximum {max} is outside the allowed range {}-{}",
                    allowed_max.min, allowed_max.max
                ));
            }
            Ok(())
        }
        (ComputePoolSelection::Fixed { .. }, CapacityGroupScalePolicy::Autoscale { .. }) => {
            Err("must use autoscale mode".to_string())
        }
        (ComputePoolSelection::Autoscale { .. }, CapacityGroupScalePolicy::Fixed { .. }) => {
            Err("must use fixed mode".to_string())
        }
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
        instance_catalog::Architecture, CapacityGroup, CapacityGroupScalePolicy,
        ComputeChoiceRange, ComputeCluster, ComputeSettings, ContainerCode, DaemonCode, Resource,
        ResourceEntry, ResourceLifecycle, Stack,
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
            inputs: vec![],
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
                ComputePoolSelection::Fixed {
                    machines: 1,
                    machine: Some("m7i.xlarge".to_string()),
                    failure_domains: None,
                },
            )]
            .into_iter()
            .collect(),
        };

        let plan = plan_compute(&stack, Platform::Aws, Some(&settings)).expect("plan should build");

        let pool = plan.pools.first().expect("general pool should exist");
        assert_eq!(pool.selected.machine(), Some("m7i.xlarge"));
        assert!(pool.errors.is_empty());
    }

    #[test]
    fn explicit_capacity_group_requirements_are_merged_with_workloads() {
        let mut stack = stack_with_container();
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: Some(MachineProfile {
                    cpu: "4".to_string(),
                    memory_bytes: 16 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    architecture: None,
                    gpu: None,
                }),
                min_size: 2,
                max_size: 5,
                scale_policy: None,
                nested_virtualization: Some(true),
            })
            .build();
        stack.resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let plan = plan_compute(&stack, Platform::Aws, None).expect("plan should build");

        let pool = plan.pools.first().expect("general pool should exist");
        let machine = pool
            .selected
            .machine()
            .expect("AWS selection should include a machine");
        let spec = instance_catalog::find_instance_type(Platform::Aws, machine)
            .expect("selected machine should exist in the catalog");
        assert!(spec.is_nested_virt_capable());
        assert_eq!(pool.selected.min_size(), 2);
        assert_eq!(pool.selected.max_size(), 5);
        assert!(pool.errors.is_empty());
    }

    #[test]
    fn nested_x86_fixed_range_pool_preserves_bounds_and_rejects_graviton() {
        let daemon = Daemon::new("vm-runtime-loader".to_string())
            .code(DaemonCode::Image {
                image: "example.com/vm-runtime:latest".to_string(),
            })
            .cluster("vm-runtime".to_string())
            .cpu(ResourceSpec {
                min: "2".to_string(),
                desired: "2".to_string(),
            })
            .memory(ResourceSpec {
                min: "4Gi".to_string(),
                desired: "4Gi".to_string(),
            })
            .permissions("loader".to_string())
            .build();
        let cluster = ComputeCluster::new("vm-runtime".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: Some(MachineProfile {
                    cpu: "4".to_string(),
                    memory_bytes: 16 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    architecture: Some(Architecture::X86_64),
                    gpu: None,
                }),
                min_size: 2,
                max_size: 2,
                scale_policy: Some(CapacityGroupScalePolicy::Fixed {
                    machines: ComputeChoiceRange {
                        min: 1,
                        max: 5,
                        default: 2,
                    },
                }),
                nested_virtualization: Some(true),
            })
            .build();
        let stack = Stack {
            id: "vm-runtime".to_string(),
            resources: [
                (
                    "vm-runtime-loader".to_string(),
                    ResourceEntry {
                        config: Resource::new(daemon),
                        lifecycle: ResourceLifecycle::Live,
                        dependencies: Vec::new(),
                        remote_access: false,
                    },
                ),
                (
                    "vm-runtime".to_string(),
                    ResourceEntry {
                        config: Resource::new(cluster),
                        lifecycle: ResourceLifecycle::Frozen,
                        dependencies: Vec::new(),
                        remote_access: false,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            permissions: crate::permissions::PermissionsConfig::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        let plan = plan_compute(&stack, Platform::Aws, None).expect("plan should build");
        let pool = plan.pools.first().expect("general pool should exist");
        assert_eq!(pool.recommended.machine(), Some("m8i.2xlarge"));
        assert_eq!(pool.recommended.min_size(), 2);
        assert_eq!(pool.recommended.max_size(), 2);
        assert_eq!(
            pool.scale,
            CapacityGroupScalePolicy::Fixed {
                machines: ComputeChoiceRange {
                    min: 1,
                    max: 5,
                    default: 2,
                },
            }
        );
        assert!(!pool
            .machines
            .iter()
            .any(|option| option.machine == "m7g.2xlarge"));

        let invalid_settings = ComputeSettings {
            pools: [(
                "general".to_string(),
                ComputePoolSelection::Fixed {
                    machines: 2,
                    machine: Some("m7g.2xlarge".to_string()),
                    failure_domains: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        let invalid_plan = plan_compute(&stack, Platform::Aws, Some(&invalid_settings))
            .expect("plan should build");
        assert!(!invalid_plan.pools[0].errors.is_empty());
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
