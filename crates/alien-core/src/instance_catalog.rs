//! Instance type catalog and selection algorithm for cloud compute infrastructure.
//!
//! This module provides:
//! - A static catalog of known instance types across AWS, GCP, and Azure
//! - Resource quantity parsing (CPU strings, Kubernetes-style memory/storage quantities)
//! - An algorithm to select the optimal instance type for a given workload
//!
//! The catalog is the single source of truth for instance type specifications.
//! It is used by the preflights system to automatically populate `CapacityGroup.instance_type`
//! and `CapacityGroup.profile` based on the containers in a stack.

use crate::{GpuSpec, MachineProfile, Platform};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Resource quantity parsing
// ---------------------------------------------------------------------------

/// Parse a CPU quantity string to f64.
///
/// Accepts plain numbers ("1", "0.5", "2.0") and millicore suffixes ("500m" = 0.5).
pub fn parse_cpu(s: &str) -> Result<f64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty CPU string".to_string());
    }

    if let Some(millis) = s.strip_suffix('m') {
        let v: f64 = millis
            .parse()
            .map_err(|_| format!("invalid CPU millicore value: '{s}'"))?;
        Ok(v / 1000.0)
    } else {
        s.parse().map_err(|_| format!("invalid CPU value: '{s}'"))
    }
}

/// Parse a memory or storage quantity string to bytes.
///
/// Supports Kubernetes-style binary suffixes (Ki, Mi, Gi, Ti) and
/// decimal suffixes (k, M, G, T). Plain numbers are interpreted as bytes.
pub fn parse_memory_bytes(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty memory/storage string".to_string());
    }

    for (suffix, multiplier) in [
        ("Ti", 1024_f64.powi(4)),
        ("Gi", 1024_f64.powi(3)),
        ("Mi", 1024_f64.powi(2)),
        ("Ki", 1024_f64),
        ("T", 1_000_000_000_000_f64),
        ("G", 1_000_000_000_f64),
        ("M", 1_000_000_f64),
        ("k", 1_000_f64),
    ] {
        if let Some(number) = s.strip_suffix(suffix) {
            let value: f64 = number
                .parse()
                .map_err(|_| format!("invalid memory value: '{s}'"))?;
            let bytes = value * multiplier;
            // Float-to-integer casts saturate negatives, infinity and NaN instead of
            // returning an error. Reject them before a sizing request reaches a provider.
            if !bytes.is_finite() || bytes < 0.0 || bytes >= u64::MAX as f64 {
                return Err(format!("memory value is out of range: '{s}'"));
            }
            return Ok(bytes as u64);
        }
    }
    s.parse()
        .map_err(|_| format!("invalid memory value: '{s}'"))
}

// ---------------------------------------------------------------------------
// Instance type catalog
// ---------------------------------------------------------------------------

/// Instance family classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum InstanceFamily {
    Burstable,
    GeneralPurpose,
    ComputeOptimized,
    MemoryOptimized,
    StorageOptimized,
    GpuCompute,
}

/// CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    Arm64,
    X86_64,
}

/// Default machine architecture for images built for a managed cloud.
pub fn default_architecture(platform: Platform) -> Option<Architecture> {
    match platform {
        Platform::Aws => Some(Architecture::Arm64),
        Platform::Gcp | Platform::Azure => Some(Architecture::X86_64),
        Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => None,
    }
}

/// Static GPU specification for catalog entries (no heap allocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogGpu {
    pub gpu_type: &'static str,
    pub count: u32,
}

/// A known instance type with its hardware specifications.
///
/// All fields are compile-time constants. The catalog is a flat array of these.
#[derive(Debug, Clone)]
pub struct InstanceTypeSpec {
    pub name: &'static str,
    pub platform: Platform,
    /// The category the machine's series belongs to.
    pub family: InstanceFamily,
    /// The provider series the machine belongs to (`m8i`, `n2-standard`, `Dsv5`).
    pub series: &'static str,
    /// How the series is shown to people choosing a machine.
    pub label: &'static str,
    pub architecture: Architecture,
    /// vCPU count (hardware total)
    pub vcpu: u32,
    /// Memory in bytes (hardware total)
    pub memory_bytes: u64,
    /// Ephemeral storage in bytes (hardware total, NVMe for storage-optimized)
    pub ephemeral_storage_bytes: u64,
    /// Local instance disk the provider reports for the machine, in bytes; 0 when it has none.
    pub local_disk_bytes: u64,
    /// The provider's machine type for this row. It differs from `name` only for a catalog
    /// row that adds an accelerator to a machine type, such as `n1-standard-4-t4`.
    pub machine_type: &'static str,
    /// GPU specification (for GPU instances)
    pub gpu: Option<CatalogGpu>,
    /// Whether the machine exposes nested virtualization (VT-x/EPT) to guest VMs.
    pub nested_virt: bool,
    /// Whether automatic selection may pick the machine. The set is fixed so that adding a
    /// machine to the catalog never changes what an existing stack gets; any other machine is
    /// used only when someone chooses it.
    pub auto_select: bool,
}

impl InstanceTypeSpec {
    /// Whether ephemeral storage is a provider disk that can be sized at
    /// deployment time instead of fixed local instance storage.
    pub fn has_configurable_ephemeral_storage(&self) -> bool {
        matches!(
            self.family,
            InstanceFamily::Burstable
                | InstanceFamily::GeneralPurpose
                | InstanceFamily::ComputeOptimized
                | InstanceFamily::MemoryOptimized
        )
    }

    /// Whether this instance type supports nested virtualization.
    ///
    /// GCP still requires the instance template to opt in; Azure exposes the
    /// capability automatically on supported VM sizes.
    pub fn is_nested_virt_capable(&self) -> bool {
        self.nested_virt
    }

    /// Convert this catalog entry into a `MachineProfile` for use in `CapacityGroup`.
    pub fn to_machine_profile(&self) -> MachineProfile {
        MachineProfile {
            cpu: format!("{}.0", self.vcpu),
            memory_bytes: self.memory_bytes,
            ephemeral_storage_bytes: self.ephemeral_storage_bytes,
            architecture: Some(self.architecture),
            gpu: self.gpu.map(|g| GpuSpec {
                gpu_type: g.gpu_type.to_string(),
                count: g.count,
            }),
            data_disk: None,
        }
    }

    /// Convert this entry to the profile controllers should provision for a
    /// workload. Configurable cloud disks grow to the requested capacity;
    /// fixed local disks retain their catalog capacity.
    pub fn to_machine_profile_for_storage(&self, requested_bytes: u64) -> MachineProfile {
        let mut profile = self.to_machine_profile();
        if self.has_configurable_ephemeral_storage() {
            profile.ephemeral_storage_bytes = profile.ephemeral_storage_bytes.max(requested_bytes);
        }
        profile
    }
}

// Helpers for readable byte constants
const KI: u64 = 1024;
const MI: u64 = KI * 1024;
const GI: u64 = MI * 1024;
const TI: u64 = GI * 1024;

/// Maximum workload storage that the current cloud controllers can turn into
/// their provider-backed root disk after adding 25% filesystem headroom and
/// 12 GiB of runtime overhead. Keep these bounds in sync with the provider
/// controller contract; rejecting here is preferable to failing after cloud
/// resources have already been created.
pub fn max_configurable_ephemeral_storage_bytes(platform: Platform) -> Option<u64> {
    let disk_limit_gib = match platform {
        // AWS gp3 and GCP persistent disks currently top out at 64 TiB.
        Platform::Aws | Platform::Gcp => 64 * 1024,
        // Azure managed OS disks currently top out at 4,095 GiB.
        Platform::Azure => 4_095,
        Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => {
            return None;
        }
    };

    // Controllers first round the requested bytes up to a whole GiB, then
    // round the 25% headroom up again. Returning a fractional-GiB bound would
    // therefore admit `bound + 1 byte` and materialize a disk one GiB over the
    // provider limit.
    let max_requested_gib = (disk_limit_gib - 12) * 4 / 5;
    Some(max_requested_gib * GI)
}

/// The instance type catalog, generated from the clouds' machine APIs by
/// `scripts/instance-catalog/generate.py` from `instance-catalog.toml`.
mod catalog;
use catalog::CATALOG;

// ---------------------------------------------------------------------------
// Catalog lookup
// ---------------------------------------------------------------------------

/// Get all instance types for a given platform.
pub fn catalog_for_platform(platform: Platform) -> Vec<&'static InstanceTypeSpec> {
    CATALOG
        .iter()
        .filter(|spec| spec.platform == platform)
        .collect()
}

/// Find a specific instance type by name and platform.
pub fn find_instance_type(platform: Platform, name: &str) -> Option<&'static InstanceTypeSpec> {
    CATALOG
        .iter()
        .find(|spec| spec.platform == platform && spec.name == name)
}

/// Whether a capacity group may move from AWS machine `old` to `new` without setup: both are
/// catalog machines of one CPU architecture, so the stack's images still run on the new one.
pub fn is_same_architecture_aws_machine(old: &str, new: &str) -> bool {
    is_same_architecture_machine(Platform::Aws, old, new)
}

/// Whether two catalog machines belong to the given cloud and share a CPU architecture.
pub fn is_same_architecture_machine(platform: Platform, old: &str, new: &str) -> bool {
    matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
        && find_instance_type(platform, old)
            .zip(find_instance_type(platform, new))
            .is_some_and(|(old, new)| old.architecture == new.architecture)
}

// ---------------------------------------------------------------------------
// Instance type selection
// ---------------------------------------------------------------------------

/// Aggregated resource requirements from all containers in a capacity group.
#[derive(Debug, Clone)]
pub struct WorkloadRequirements {
    /// Total CPU needed at desired scale (sum of desired CPU * desired_replicas per container)
    pub total_cpu_at_desired: f64,
    /// Total memory needed at desired scale (sum of desired memory * desired_replicas per container)
    pub total_memory_bytes_at_desired: u64,
    /// Total CPU needed at maximum scale (sum of desired CPU * max_replicas per container)
    pub total_cpu_at_max: f64,
    /// Total memory needed at maximum scale (sum of desired memory * max_replicas per container)
    pub total_memory_bytes_at_max: u64,
    /// Largest CPU request among all individual containers (single replica)
    pub max_cpu_per_container: f64,
    /// Largest memory request among all individual containers (single replica)
    pub max_memory_per_container: u64,
    /// Maximum ephemeral storage any single container requires
    pub max_ephemeral_storage_bytes: u64,
    /// GPU requirement (if any container needs GPU)
    pub gpu: Option<GpuSpec>,
    /// Required CPU architecture, when source explicitly constrains it.
    pub architecture: Option<Architecture>,
    /// If true, only instance types that expose nested virtualization (VT-x/EPT)
    /// to guest VMs are eligible. Required by workloads that run QEMU/KVM
    /// inside a container.
    pub nested_virt: bool,
}

/// Result of instance type selection.
#[derive(Debug, Clone)]
pub struct InstanceSelection {
    /// Selected instance type name (e.g., "m7g.2xlarge")
    pub instance_type: &'static str,
    /// Machine profile derived from the instance type
    pub profile: MachineProfile,
    /// Recommended minimum number of machines
    pub min_machines: u32,
    /// Recommended maximum number of machines
    pub max_machines: u32,
}

/// Ephemeral storage threshold above which storage-optimized instances are selected.
const STORAGE_OPTIMIZED_THRESHOLD: u64 = 200 * GI;

/// Maximum number of machines per cluster.
const MAX_MACHINES_PER_CLUSTER: u32 = 10;

/// Preferred vCPU cap for non-GPU/non-storage workloads. A larger indivisible
/// allocation or bounded fleet can require a bigger machine.
const MAX_STANDARD_VCPU: u32 = 8;

/// Runtime CPU reserved for system processes on each managed container machine.
const SYSTEM_RESERVE_CPU: f64 = 0.5;

/// Runtime planning headroom for total desired/max workload.
const WORKLOAD_HEADROOM_FACTOR: f64 = 1.15;

/// Select the best instance type for a workload on a given platform.
///
/// The algorithm:
/// 1. GPU workloads: Match by GPU type, find smallest instance with enough GPUs.
/// 2. Storage-heavy workloads (>200Gi ephemeral): Use storage-optimized instances.
/// 3. All other workloads: Fit the allocation with system reserve and headroom,
///    preferring at most 8 vCPUs unless the allocation requires more. Use the
///    workload's preferred family, with general-purpose fallback when needed.
///
/// Returns an error if no suitable instance type is found.
pub fn select_instance_type(
    platform: Platform,
    requirements: &WorkloadRequirements,
) -> Result<InstanceSelection, String> {
    select_instance_type_where(platform, requirements, &|_| true)
}

/// A machine that automatic selection may pick, restricted by a caller's filter.
type MachineFilter<'a> = &'a dyn Fn(&InstanceTypeSpec) -> bool;

/// Like [`select_instance_type`], but only from the series that have room for `machines`
/// machines. `has_room` answers for one machine of the series; the catalog's series order
/// is the preference order, so the first series with room wins. When no series has room,
/// the result is the plain selection, so the caller's quota check reports the refusal.
/// Pass the same `machines` the pool launches with at first setup.
pub fn select_instance_type_with_room(
    platform: Platform,
    requirements: &WorkloadRequirements,
    machines: u32,
    has_room: &dyn Fn(&InstanceTypeSpec, u32) -> bool,
) -> Result<InstanceSelection, String> {
    for series in auto_select_series(platform) {
        let in_series = |spec: &InstanceTypeSpec| spec.series == series;
        let Ok(selection) = select_instance_type_where(platform, requirements, &in_series) else {
            continue;
        };
        let fits = find_instance_type(platform, selection.instance_type)
            .is_some_and(|spec| has_room(spec, machines));
        if fits {
            return Ok(selection);
        }
    }
    select_instance_type(platform, requirements)
}

/// The series automatic selection may pick from, in catalog (preference) order.
pub fn auto_select_series(platform: Platform) -> Vec<&'static str> {
    let mut series: Vec<&'static str> = Vec::new();
    for spec in CATALOG
        .iter()
        .filter(|spec| spec.auto_select && spec.platform == platform)
    {
        if !series.contains(&spec.series) {
            series.push(spec.series);
        }
    }
    series
}

fn select_instance_type_where(
    platform: Platform,
    requirements: &WorkloadRequirements,
    allowed: MachineFilter,
) -> Result<InstanceSelection, String> {
    // An unset architecture accepts either one. Nested virtualization and GPU
    // models can exist on only one architecture of a platform (AWS nested
    // virtualization is x86_64 only), so try the platform default first and
    // then the other architecture.
    if requirements.architecture.is_none()
        && (requirements.nested_virt || requirements.gpu.is_some())
    {
        if let Some(default) = default_architecture(platform) {
            let other = match default {
                Architecture::Arm64 => Architecture::X86_64,
                Architecture::X86_64 => Architecture::Arm64,
            };
            let with_architecture = |architecture| WorkloadRequirements {
                architecture: Some(architecture),
                ..requirements.clone()
            };
            let selection = select_instance_type_for_architecture(
                platform,
                &with_architecture(default),
                allowed,
            );
            if selection.is_ok() {
                return selection;
            }
            // Report both reasons: the default architecture often lacks the
            // feature entirely, while the other one says what to change.
            return select_instance_type_for_architecture(
                platform,
                &with_architecture(other),
                allowed,
            )
            .map_err(|other_error| {
                let default_error = selection.err().unwrap_or_default();
                format!("{default:?}: {default_error}; {other:?}: {other_error}")
            });
        }
    }
    select_instance_type_for_architecture(platform, requirements, allowed)
}

fn select_instance_type_for_architecture(
    platform: Platform,
    requirements: &WorkloadRequirements,
    allowed: MachineFilter,
) -> Result<InstanceSelection, String> {
    let family = select_family(requirements);
    let family = if requirements.nested_virt && family == InstanceFamily::Burstable {
        InstanceFamily::GeneralPurpose
    } else {
        family
    };
    let selection = select_instance_type_in_family(platform, requirements, family, allowed);
    if selection.is_err() && family == InstanceFamily::Burstable {
        return select_instance_type_in_family(
            platform,
            requirements,
            InstanceFamily::GeneralPurpose,
            allowed,
        );
    }
    selection
}

fn select_instance_type_in_family(
    platform: Platform,
    requirements: &WorkloadRequirements,
    family: InstanceFamily,
    allowed: MachineFilter,
) -> Result<InstanceSelection, String> {
    let architecture = requirements
        .architecture
        .or_else(|| default_architecture(platform))
        .ok_or_else(|| format!("platform {platform} has no default compute architecture"))?;

    let candidates: Vec<&InstanceTypeSpec> = CATALOG
        .iter()
        .filter(|spec| spec.auto_select && spec.platform == platform && spec.family == family)
        .filter(|spec| allowed(spec))
        .filter(|spec| {
            if requirements.nested_virt {
                spec.is_nested_virt_capable()
            } else {
                platform != Platform::Aws || !spec.is_nested_virt_capable()
            }
        })
        .filter(|spec| spec.architecture == architecture)
        .collect();

    // A storage-heavy workload may have no fixed-local candidate for the
    // requested architecture/nested-virtualization contract. That is exactly
    // when we must consider a provider-backed disk on a general-purpose VM;
    // rejecting here made the fallback below unreachable.
    if candidates.is_empty() && family != InstanceFamily::StorageOptimized {
        let family_has_other_architecture = CATALOG.iter().any(|spec| {
            spec.auto_select
                && spec.platform == platform
                && spec.family == family
                && if requirements.nested_virt {
                    spec.is_nested_virt_capable()
                } else {
                    platform != Platform::Aws || !spec.is_nested_virt_capable()
                }
        });
        if family_has_other_architecture {
            return Err(format!(
                "architecture {architecture:?} is unavailable for this workload on platform {platform}"
            ));
        }
        return Err(if requirements.nested_virt {
            format!(
                "no nested-virt-capable {family:?} instance types in catalog for platform {platform}"
            )
        } else {
            format!("no {family:?} instance types in catalog for platform {platform}")
        });
    }

    // For GPU workloads, filter by GPU type
    let candidates = if let Some(ref gpu) = requirements.gpu {
        let filtered: Vec<&InstanceTypeSpec> = candidates
            .into_iter()
            .filter(|spec| {
                spec.gpu.as_ref().map_or(false, |g| {
                    g.gpu_type == gpu.gpu_type && g.count >= gpu.count
                })
            })
            .collect();
        if filtered.is_empty() {
            return Err(format!(
                "no instance type for GPU type '{}' x{} on platform {platform}",
                gpu.gpu_type, gpu.count
            ));
        }
        filtered
    } else {
        candidates
    };

    // For storage workloads, filter by ephemeral storage capacity
    let candidates = if family == InstanceFamily::StorageOptimized {
        let filtered: Vec<&InstanceTypeSpec> = candidates
            .into_iter()
            .filter(|spec| spec.ephemeral_storage_bytes >= requirements.max_ephemeral_storage_bytes)
            .collect();
        if filtered.is_empty() {
            // Fixed-local NVMe is preferred for large requests, but provider
            // disks on ordinary cloud machines remain configurable. Fall back
            // when no fixed-local catalog entry can satisfy the request.
            CATALOG
                .iter()
                .filter(|spec| {
                    spec.auto_select
                        && spec.platform == platform
                        && spec.family == InstanceFamily::GeneralPurpose
                        && allowed(spec)
                        && spec.has_configurable_ephemeral_storage()
                        && max_configurable_ephemeral_storage_bytes(platform)
                            .is_some_and(|max| requirements.max_ephemeral_storage_bytes <= max)
                })
                .filter(|spec| {
                    if requirements.nested_virt {
                        spec.is_nested_virt_capable()
                    } else {
                        platform != Platform::Aws || !spec.is_nested_virt_capable()
                    }
                })
                .filter(|spec| spec.architecture == architecture)
                .collect()
        } else {
            filtered
        }
    } else {
        candidates
    };

    if candidates.is_empty() {
        return Err(format!(
            "architecture {architecture:?} is unavailable for this workload on platform {platform}"
        ));
    }

    // More machines cannot split one replica. Every candidate must fit its
    // largest allocation after host reserve and workload headroom.
    let candidates: Vec<_> = candidates
        .into_iter()
        .filter(|spec| {
            allocatable_cpu(spec) >= requirements.max_cpu_per_container * WORKLOAD_HEADROOM_FACTOR
                && allocatable_memory_bytes(spec) as f64
                    >= requirements.max_memory_per_container as f64 * WORKLOAD_HEADROOM_FACTOR
        })
        .collect();
    let smallest_replica_machine = candidates.iter().map(|spec| spec.vcpu).min()
        .ok_or_else(|| format!("no {platform} machine can fit one replica with system reserve and workload headroom"))?;

    // Apply the policy for the candidates we will actually select from. A
    // storage-heavy request can fall back from fixed-local storage machines to
    // general-purpose machines with provider-backed disks; those machines must
    // retain the normal horizontal-scaling cap.
    let effective_family = candidates[0].family;
    let vcpu_cap = if effective_family == InstanceFamily::GpuCompute
        || effective_family == InstanceFamily::StorageOptimized
    {
        u32::MAX
    } else {
        MAX_STANDARD_VCPU.max(smallest_replica_machine)
    };

    let desired_target_machines = desired_target_machines(requirements);
    let target_cpu = requirements
        .max_cpu_per_container
        .max(requirements.total_cpu_at_desired / desired_target_machines as f64)
        * WORKLOAD_HEADROOM_FACTOR;
    let target_memory = (requirements.max_memory_per_container as f64)
        .max(requirements.total_memory_bytes_at_desired as f64 / desired_target_machines as f64)
        * WORKLOAD_HEADROOM_FACTOR;

    // Find the smallest instance whose allocatable capacity meets the workload
    // target after host reserve and workload headroom. Machine count already
    // accounts for multiple replicas; requiring space for an arbitrary second
    // copy here would size the same demand twice.
    let selected = candidates
        .iter()
        .filter(|spec| {
            spec.vcpu <= vcpu_cap
                && allocatable_cpu(spec) >= target_cpu
                && allocatable_memory_bytes(spec) as f64 >= target_memory
        })
        .min_by_key(|spec| spec.vcpu)
        .or_else(|| {
            // If nothing fits within the cap, pick the largest instance under the cap
            // `max_by_key` keeps the last of equal machines; the earlier series wins ties.
            candidates
                .iter()
                .rev()
                .filter(|spec| spec.vcpu <= vcpu_cap)
                .max_by_key(|spec| spec.vcpu)
        })
        .or_else(|| {
            // Last resort: pick the smallest available instance (for GPU/storage)
            candidates.iter().min_by_key(|spec| spec.vcpu)
        })
        .ok_or_else(|| format!("no instance types available for platform {platform}"))?;

    // Calculate machine counts
    let max_machines = compute_max_machines(requirements, selected);
    let min_machines = compute_min_machines(requirements, selected, max_machines);

    Ok(InstanceSelection {
        instance_type: selected.name,
        profile: selected.to_machine_profile_for_storage(requirements.max_ephemeral_storage_bytes),
        min_machines,
        max_machines,
    })
}

/// Select instance family based on workload characteristics.
///
/// Uses GeneralPurpose for all standard workloads — widely available across
/// regions and cost-effective. Only specialized workloads (GPU, large ephemeral
/// storage) get specialized families. Very small workloads get burstable.
pub fn select_family(requirements: &WorkloadRequirements) -> InstanceFamily {
    // GPU workloads always get GPU instances
    if requirements.gpu.is_some() {
        return InstanceFamily::GpuCompute;
    }

    // Large ephemeral storage needs NVMe (storage-optimized)
    if requirements.max_ephemeral_storage_bytes > STORAGE_OPTIMIZED_THRESHOLD {
        return InstanceFamily::StorageOptimized;
    }

    // Very small workloads use burstable instances
    if requirements.total_cpu_at_max < 2.0 {
        return InstanceFamily::Burstable;
    }

    // All other workloads use GeneralPurpose — available everywhere, good pricing
    InstanceFamily::GeneralPurpose
}

/// Calculate maximum machines needed to fit the workload with headroom.
fn compute_max_machines(requirements: &WorkloadRequirements, instance: &InstanceTypeSpec) -> u32 {
    let cpu_with_headroom = requirements.total_cpu_at_max * WORKLOAD_HEADROOM_FACTOR;
    let cpu_machines = (cpu_with_headroom / allocatable_cpu(instance)).ceil() as u32;

    let mem_with_headroom =
        requirements.total_memory_bytes_at_max as f64 * WORKLOAD_HEADROOM_FACTOR;
    let mem_machines =
        (mem_with_headroom / allocatable_memory_bytes(instance) as f64).ceil() as u32;

    // Take the larger of CPU-based and memory-based, clamped to cluster limit
    cpu_machines
        .max(mem_machines)
        .max(1)
        .min(MAX_MACHINES_PER_CLUSTER)
}

/// Calculate minimum machines for HA.
fn compute_min_machines(
    requirements: &WorkloadRequirements,
    instance: &InstanceTypeSpec,
    max_machines: u32,
) -> u32 {
    let cpu_with_headroom = requirements.total_cpu_at_desired * WORKLOAD_HEADROOM_FACTOR;
    let cpu_machines = (cpu_with_headroom / allocatable_cpu(instance)).ceil() as u32;

    let mem_with_headroom =
        requirements.total_memory_bytes_at_desired as f64 * WORKLOAD_HEADROOM_FACTOR;
    let mem_machines =
        (mem_with_headroom / allocatable_memory_bytes(instance) as f64).ceil() as u32;

    cpu_machines
        .max(mem_machines)
        .max(1)
        .min(2)
        .min(max_machines)
}

fn desired_target_machines(requirements: &WorkloadRequirements) -> u32 {
    if requirements.total_cpu_at_desired >= 2.0
        || requirements.total_memory_bytes_at_desired >= 4 * GI
    {
        2
    } else {
        1
    }
}

pub(crate) fn allocatable_cpu(instance: &InstanceTypeSpec) -> f64 {
    (instance.vcpu as f64 - SYSTEM_RESERVE_CPU).max(0.25)
}

pub(crate) fn allocatable_memory_bytes(instance: &InstanceTypeSpec) -> u64 {
    instance
        .memory_bytes
        .saturating_sub(system_reserve_memory_bytes(instance.memory_bytes))
        .max(256 * MI)
}

fn system_reserve_memory_bytes(memory_bytes: u64) -> u64 {
    if memory_bytes < 4 * GI {
        256 * MI
    } else if memory_bytes < 16 * GI {
        512 * MI
    } else {
        GI
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BinaryTarget;

    // -- Parsing tests --

    #[test]
    fn test_parse_cpu_plain() {
        assert_eq!(parse_cpu("1").unwrap(), 1.0);
        assert_eq!(parse_cpu("0.5").unwrap(), 0.5);
        assert_eq!(parse_cpu("2.0").unwrap(), 2.0);
        assert_eq!(parse_cpu("16").unwrap(), 16.0);
    }

    #[test]
    fn test_parse_cpu_millicore() {
        assert_eq!(parse_cpu("500m").unwrap(), 0.5);
        assert_eq!(parse_cpu("250m").unwrap(), 0.25);
        assert_eq!(parse_cpu("1000m").unwrap(), 1.0);
        assert_eq!(parse_cpu("100m").unwrap(), 0.1);
    }

    #[test]
    fn test_parse_cpu_invalid() {
        assert!(parse_cpu("").is_err());
        assert!(parse_cpu("abc").is_err());
        assert!(parse_cpu("m").is_err());
    }

    #[test]
    fn test_parse_memory_binary_suffixes() {
        assert_eq!(parse_memory_bytes("1Ki").unwrap(), 1024);
        assert_eq!(parse_memory_bytes("1Mi").unwrap(), 1024 * 1024);
        assert_eq!(parse_memory_bytes("1Gi").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_memory_bytes("4Gi").unwrap(), 4 * 1024 * 1024 * 1024);
        assert_eq!(parse_memory_bytes("512Mi").unwrap(), 512 * 1024 * 1024);
        assert_eq!(
            parse_memory_bytes("1Ti").unwrap(),
            1024u64 * 1024 * 1024 * 1024
        );
    }

    #[test]
    fn test_parse_memory_decimal_suffixes() {
        assert_eq!(parse_memory_bytes("1k").unwrap(), 1000);
        assert_eq!(parse_memory_bytes("1M").unwrap(), 1_000_000);
        assert_eq!(parse_memory_bytes("1G").unwrap(), 1_000_000_000);
        assert_eq!(parse_memory_bytes("1T").unwrap(), 1_000_000_000_000);
    }

    #[test]
    fn test_parse_memory_plain_bytes() {
        assert_eq!(parse_memory_bytes("1024").unwrap(), 1024);
        assert_eq!(parse_memory_bytes("0").unwrap(), 0);
    }

    #[test]
    fn test_parse_memory_invalid() {
        assert!(parse_memory_bytes("").is_err());
        assert!(parse_memory_bytes("abc").is_err());
        assert!(parse_memory_bytes("Gi").is_err());
    }

    #[test]
    fn test_parse_memory_fractional() {
        assert_eq!(parse_memory_bytes("0.5Gi").unwrap(), GI / 2);
        assert_eq!(parse_memory_bytes("1.5Gi").unwrap(), GI + GI / 2);
    }

    // -- Catalog lookup tests --

    #[test]
    fn test_catalog_has_entries_for_all_cloud_platforms() {
        assert!(!catalog_for_platform(Platform::Aws).is_empty());
        assert!(!catalog_for_platform(Platform::Gcp).is_empty());
        assert!(!catalog_for_platform(Platform::Azure).is_empty());
    }

    #[test]
    fn test_catalog_no_entries_for_non_cloud_platforms() {
        assert!(catalog_for_platform(Platform::Local).is_empty());
        assert!(catalog_for_platform(Platform::Kubernetes).is_empty());
    }

    #[test]
    fn test_find_known_instance_type() {
        let spec =
            find_instance_type(Platform::Aws, "m7g.2xlarge").expect("should find m7g.2xlarge");
        assert_eq!(spec.vcpu, 8);
        assert_eq!(spec.memory_bytes, 32 * GI);
        assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
    }

    #[test]
    fn test_find_aws_c8i_nested_virt_instance_type() {
        let spec = find_instance_type(Platform::Aws, "c8i.large").expect("should find c8i.large");
        assert_eq!(spec.vcpu, 2);
        assert_eq!(spec.memory_bytes, 4 * GI);
        assert_eq!(spec.family, InstanceFamily::ComputeOptimized);
        assert_eq!(spec.architecture, Architecture::X86_64);
        assert!(spec.is_nested_virt_capable());
    }

    #[test]
    fn test_find_unknown_instance_type() {
        assert!(find_instance_type(Platform::Aws, "nonexistent.xlarge").is_none());
    }

    #[test]
    fn test_find_wrong_platform() {
        assert!(find_instance_type(Platform::Gcp, "m7g.2xlarge").is_none());
    }

    #[test]
    fn test_to_machine_profile() {
        let spec = find_instance_type(Platform::Aws, "m7g.2xlarge").unwrap();
        let profile = spec.to_machine_profile();
        assert_eq!(profile.cpu, "8.0");
        assert_eq!(profile.memory_bytes, 32 * GI);
        assert_eq!(profile.ephemeral_storage_bytes, 20 * GI);
        assert!(profile.gpu.is_none());
    }

    #[test]
    fn test_to_machine_profile_with_gpu() {
        let spec = find_instance_type(Platform::Aws, "p4d.24xlarge").unwrap();
        let profile = spec.to_machine_profile();
        let gpu = profile.gpu.as_ref().expect("should have GPU");
        assert_eq!(gpu.gpu_type, "nvidia-a100");
        assert_eq!(gpu.count, 8);
    }

    // -- Selection algorithm tests --

    #[test]
    fn small_cpu_with_large_memory_falls_back_from_burstable() {
        let requirements = WorkloadRequirements {
            total_cpu_at_desired: 0.5,
            total_memory_bytes_at_desired: 8 * GI,
            total_cpu_at_max: 0.5,
            total_memory_bytes_at_max: 8 * GI,
            max_cpu_per_container: 0.5,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 0,
            gpu: None,
            architecture: Some(Architecture::X86_64),
            nested_virt: false,
        };
        let selected = select_instance_type(Platform::Gcp, &requirements).unwrap();
        let machine = find_instance_type(Platform::Gcp, selected.instance_type).unwrap();
        assert!(
            allocatable_memory_bytes(machine) as f64 >= 8.0 * GI as f64 * WORKLOAD_HEADROOM_FACTOR
        );
    }

    #[test]
    fn test_select_burstable_for_small_workload() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 1.0,
            total_memory_bytes_at_desired: 2 * GI,
            total_cpu_at_max: 1.0,
            total_memory_bytes_at_max: 2 * GI,
            max_cpu_per_container: 0.5,
            max_memory_per_container: 1 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::Burstable);
    }

    #[test]
    fn test_selects_smallest_burstable_machine_with_real_headroom() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 1.0,
            total_memory_bytes_at_desired: 2 * GI,
            total_cpu_at_max: 1.0,
            total_memory_bytes_at_max: 2 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 2 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };

        let selection = select_instance_type(Platform::Aws, &req).unwrap();

        assert_eq!(selection.instance_type, "t4g.medium");
        assert_eq!(selection.min_machines, 1);
        assert_eq!(selection.max_machines, 1);
    }

    #[test]
    fn test_select_general_purpose_for_standard_workload() {
        // Standard workloads always get GeneralPurpose regardless of CPU:memory ratio
        let req = WorkloadRequirements {
            total_cpu_at_desired: 20.0,
            total_memory_bytes_at_desired: 80 * GI,
            total_cpu_at_max: 20.0,
            total_memory_bytes_at_max: 80 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
    }

    #[test]
    fn test_select_general_purpose_even_for_cpu_heavy() {
        // CPU-heavy workloads still get GeneralPurpose (no more ComputeOptimized auto-select)
        let req = WorkloadRequirements {
            total_cpu_at_desired: 20.0,
            total_memory_bytes_at_desired: 20 * GI,
            total_cpu_at_max: 20.0,
            total_memory_bytes_at_max: 20 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 2 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
    }

    #[test]
    fn test_select_storage_optimized_for_large_ephemeral() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 8.0,
            total_memory_bytes_at_desired: 32 * GI,
            total_cpu_at_max: 8.0,
            total_memory_bytes_at_max: 32 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 500 * GI,
            gpu: None,
            architecture: Some(Architecture::X86_64),
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::StorageOptimized);
    }

    #[test]
    fn test_select_configurable_storage_above_fixed_local_catalog() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 8.0,
            total_memory_bytes_at_desired: 32 * GI,
            total_cpu_at_max: 8.0,
            total_memory_bytes_at_max: 32 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 8_000 * GI,
            gpu: None,
            architecture: Some(Architecture::X86_64),
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
        assert_eq!(sel.profile.ephemeral_storage_bytes, 8_000 * GI);
    }

    #[test]
    fn test_configurable_storage_fallback_retains_standard_vcpu_cap() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 70.0,
            total_memory_bytes_at_desired: 140 * GI,
            total_cpu_at_max: 70.0,
            total_memory_bytes_at_max: 140 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 8_000 * GI,
            gpu: None,
            architecture: Some(Architecture::X86_64),
            nested_virt: false,
        };

        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
        assert!(spec.vcpu <= MAX_STANDARD_VCPU);
        assert!(sel.max_machines > 1);
    }

    #[test]
    fn test_nested_virtualization_storage_falls_back_to_configurable_disk() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let req = WorkloadRequirements {
                total_cpu_at_desired: 2.0,
                total_memory_bytes_at_desired: 8 * GI,
                total_cpu_at_max: 2.0,
                total_memory_bytes_at_max: 8 * GI,
                max_cpu_per_container: 2.0,
                max_memory_per_container: 8 * GI,
                max_ephemeral_storage_bytes: 500 * GI,
                gpu: None,
                architecture: Some(Architecture::X86_64),
                nested_virt: true,
            };

            let sel = select_instance_type(platform, &req)
                .unwrap_or_else(|error| panic!("{platform} should support fallback: {error}"));
            let spec = find_instance_type(platform, sel.instance_type).unwrap();
            assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
            assert!(spec.is_nested_virt_capable());
            assert_eq!(sel.profile.ephemeral_storage_bytes, 500 * GI);
        }
    }

    #[test]
    fn unset_architecture_resolves_to_the_one_offering_nested_virtualization() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 2.0,
            total_memory_bytes_at_desired: 4 * GI,
            total_cpu_at_max: 2.0,
            total_memory_bytes_at_max: 4 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 20 * GI,
            gpu: None,
            architecture: None,
            nested_virt: true,
        };

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let selection = select_instance_type(platform, &req)
                .unwrap_or_else(|message| panic!("{platform}: {message}"));
            let spec = find_instance_type(platform, selection.instance_type)
                .expect("selected machine should exist in the catalog");
            assert!(spec.is_nested_virt_capable(), "{platform}: {}", spec.name);
            assert_eq!(spec.architecture, Architecture::X86_64, "{platform}");
            assert_eq!(selection.profile.architecture, Some(Architecture::X86_64));
        }

        // When neither architecture fits, the error says why for each, so a
        // sizing problem is not reported as a missing architecture.
        let oversized = WorkloadRequirements {
            max_cpu_per_container: 1000.0,
            total_cpu_at_desired: 1000.0,
            total_cpu_at_max: 1000.0,
            ..req.clone()
        };
        let message = select_instance_type(Platform::Aws, &oversized)
            .expect_err("no AWS machine fits 1000 vCPU per replica");
        assert!(
            message.starts_with("Arm64: architecture Arm64 is unavailable"),
            "{message}"
        );
        assert!(
            message.contains("; X86_64: no aws machine can fit one replica"),
            "{message}"
        );

        // An explicit architecture is a constraint, not a preference.
        let arm = WorkloadRequirements {
            architecture: Some(Architecture::Arm64),
            ..req
        };
        assert!(select_instance_type(Platform::Aws, &arm).is_err());
    }

    #[test]
    fn test_configurable_disk_rejects_capacity_above_provider_limit() {
        for (platform, requested) in [
            (Platform::Aws, 100 * TI),
            (Platform::Gcp, 100 * TI),
            (Platform::Azure, 8_000 * GI),
        ] {
            let req = WorkloadRequirements {
                total_cpu_at_desired: 2.0,
                total_memory_bytes_at_desired: 8 * GI,
                total_cpu_at_max: 2.0,
                total_memory_bytes_at_max: 8 * GI,
                max_cpu_per_container: 2.0,
                max_memory_per_container: 8 * GI,
                max_ephemeral_storage_bytes: requested,
                gpu: None,
                architecture: Some(Architecture::X86_64),
                nested_virt: true,
            };

            assert!(select_instance_type(platform, &req).is_err());
        }
    }

    #[test]
    fn test_configurable_disk_limits_account_for_controller_rounding() {
        for (platform, disk_limit_gib) in [
            (Platform::Aws, 64 * 1024),
            (Platform::Gcp, 64 * 1024),
            (Platform::Azure, 4_095),
        ] {
            let max = max_configurable_ephemeral_storage_bytes(platform).unwrap();
            let materialized_disk_gib = |requested_bytes: u64| {
                let requested_gib = requested_bytes.div_ceil(GI);
                (requested_gib * 5).div_ceil(4) + 12
            };

            assert!(materialized_disk_gib(max) <= disk_limit_gib);
            assert!(materialized_disk_gib(max + 1) > disk_limit_gib);
        }
    }

    #[test]
    fn test_gpu_local_storage_is_not_treated_as_resizable() {
        let spec = find_instance_type(Platform::Aws, "g5.xlarge").unwrap();
        assert_eq!(spec.family, InstanceFamily::GpuCompute);
        assert!(!spec.has_configurable_ephemeral_storage());
    }

    #[test]
    fn test_select_gpu_instance() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 8.0,
            total_memory_bytes_at_desired: 32 * GI,
            total_cpu_at_max: 8.0,
            total_memory_bytes_at_max: 32 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 16 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: Some(GpuSpec {
                gpu_type: "nvidia-a100".to_string(),
                count: 1,
            }),
            architecture: Some(Architecture::X86_64),
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::GpuCompute);
        assert!(spec.gpu.is_some());
    }

    #[test]
    fn test_select_uses_each_cloud_image_target_architecture() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 4.0,
            total_memory_bytes_at_desired: 16 * GI,
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 16 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let sel = select_instance_type(platform, &req)
                .unwrap_or_else(|error| panic!("selection failed for {platform}: {error}"));
            let spec = find_instance_type(platform, sel.instance_type)
                .expect("selected machine should exist in the catalog");
            assert_eq!(
                Some(spec.architecture),
                default_architecture(platform),
                "machine architecture must match the image target for {platform}"
            );
        }
    }

    #[test]
    fn test_machine_count_reasonable() {
        // Single container: 1 CPU, 2Gi, maxReplicas=20
        let req = WorkloadRequirements {
            total_cpu_at_desired: 20.0,
            total_memory_bytes_at_desired: 40 * GI,
            total_cpu_at_max: 20.0,
            total_memory_bytes_at_max: 40 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 2 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        assert!(sel.min_machines >= 1);
        assert!(sel.max_machines <= MAX_MACHINES_PER_CLUSTER);
        assert!(sel.max_machines >= sel.min_machines);
    }

    #[test]
    fn test_instance_size_capped_at_8_vcpu() {
        // Many small replicas prefer more machines over larger instances.
        let req = WorkloadRequirements {
            total_cpu_at_desired: 70.0,
            total_memory_bytes_at_desired: 140 * GI,
            total_cpu_at_max: 70.0,
            total_memory_bytes_at_max: 140 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Gcp, &req).unwrap();
        let spec = find_instance_type(Platform::Gcp, sel.instance_type).unwrap();
        assert!(
            spec.vcpu <= MAX_STANDARD_VCPU,
            "selected {} with {} vCPUs, expected <= {}",
            spec.name,
            spec.vcpu,
            MAX_STANDARD_VCPU
        );
        assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
        // Should scale horizontally instead
        assert!(sel.max_machines > 1);
    }

    #[test]
    fn test_larger_autoscaled_workload_gets_reasonable_instance() {
        // Simulates a larger autoscaled workload: 4 containers, each 2 CPU / 4 GiB
        // maxReplicas: 10, 10, 10, 5
        let req = WorkloadRequirements {
            total_cpu_at_desired: 70.0,
            total_memory_bytes_at_desired: 140 * GI,
            total_cpu_at_max: 70.0,              // 2*10 + 2*10 + 2*10 + 2*5
            total_memory_bytes_at_max: 140 * GI, // 4*10 + 4*10 + 4*10 + 4*5
            max_cpu_per_container: 2.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 20 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Gcp, &req).unwrap();
        // Should pick n2-standard-8 (8 vCPU, 32 GiB) — NOT c3-standard-44
        assert_eq!(sel.instance_type, "n2-standard-8");
        assert!(sel.max_machines >= 2);
    }

    /// When `nested_virt` is set on the workload, the selector must
    /// restrict to nested-virt-capable families. On AWS that means an m8i
    /// (or other 8th-gen Intel) entry, never a Graviton (`*7g`, `t4g`) or
    /// burstable. Without this filter the launch template gets created
    /// with `CpuOptions.NestedVirtualization=enabled` paired with an
    /// instance type AWS rejects at RunInstances.
    #[test]
    fn test_select_aws_picks_m8i_when_nested_virt_required() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 4.0,
            total_memory_bytes_at_desired: 8 * GI,
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 8 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: Some(Architecture::X86_64),
            nested_virt: true,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        assert!(
            sel.instance_type.starts_with("m8i.")
                || sel.instance_type.starts_with("c8i.")
                || sel.instance_type.starts_with("r8i."),
            "expected an m8i/c8i/r8i instance, got {}",
            sel.instance_type
        );
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert!(spec.is_nested_virt_capable());
    }

    #[test]
    fn test_select_gcp_picks_n2_when_nested_virt_required() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 4.0,
            total_memory_bytes_at_desired: 8 * GI,
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 8 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 0,
            architecture: Some(Architecture::X86_64),
            gpu: None,
            nested_virt: true,
        };

        let selection = select_instance_type(Platform::Gcp, &req).unwrap();
        assert_eq!(selection.instance_type, "n2-standard-8");
        assert!(find_instance_type(Platform::Gcp, selection.instance_type)
            .unwrap()
            .is_nested_virt_capable());
    }

    #[test]
    fn test_select_azure_picks_dsv5_when_nested_virt_required() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 4.0,
            total_memory_bytes_at_desired: 8 * GI,
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 8 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 0,
            architecture: Some(Architecture::X86_64),
            gpu: None,
            nested_virt: true,
        };

        let selection = select_instance_type(Platform::Azure, &req).unwrap();
        assert_eq!(selection.instance_type, "Standard_D8s_v5");
        assert!(find_instance_type(Platform::Azure, selection.instance_type)
            .unwrap()
            .is_nested_virt_capable());
    }

    #[test]
    fn test_select_aws_defaults_to_image_target_architecture() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 4.0,
            total_memory_bytes_at_desired: 8 * GI,
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 8 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.architecture, Architecture::Arm64);
    }

    #[test]
    fn test_cloud_defaults_match_image_target_architectures() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let target = BinaryTarget::defaults_for_platform(platform)
                .into_iter()
                .next()
                .expect("managed cloud should have a default image target");
            let image_architecture = match target.oci_arch() {
                "arm64" => Architecture::Arm64,
                "amd64" => Architecture::X86_64,
                architecture => {
                    panic!("unsupported managed-cloud image architecture {architecture}")
                }
            };

            assert_eq!(default_architecture(platform), Some(image_architecture));
        }
    }

    /// ARM remains available when the workload or capacity profile declares it.
    #[test]
    fn test_select_aws_uses_graviton_for_explicit_arm64() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 4.0,
            total_memory_bytes_at_desired: 8 * GI,
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 8 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: Some(Architecture::Arm64),
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.architecture, Architecture::Arm64);
    }

    #[test]
    fn test_select_rejects_explicit_architecture_missing_from_cloud_catalog() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 1.0,
            total_memory_bytes_at_desired: 2 * GI,
            total_cpu_at_max: 1.0,
            total_memory_bytes_at_max: 2 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 2 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: Some(Architecture::Arm64),
            nested_virt: false,
        };

        let error = select_instance_type(Platform::Gcp, &req)
            .expect_err("GCP catalog has no ARM64 machine");

        assert!(error.contains("architecture Arm64 is unavailable"));
    }

    #[test]
    fn test_profile_has_required_fields() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 4.0,
            total_memory_bytes_at_desired: 16 * GI,
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 16 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: false,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        assert!(!sel.profile.cpu.is_empty());
        assert!(sel.profile.memory_bytes > 0);
        assert!(sel.profile.ephemeral_storage_bytes > 0);
    }

    #[test]
    fn test_error_for_unsupported_gpu_type() {
        let req = WorkloadRequirements {
            total_cpu_at_desired: 8.0,
            total_memory_bytes_at_desired: 32 * GI,
            total_cpu_at_max: 8.0,
            total_memory_bytes_at_max: 32 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 16 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: Some(GpuSpec {
                gpu_type: "amd-mi300".to_string(),
                count: 1,
            }),
            architecture: None,
            nested_virt: false,
        };
        let result = select_instance_type(Platform::Aws, &req);
        assert!(result.is_err());
    }

    /// Workload shapes for the auto-selection matrix: name, CPU and memory per replica,
    /// desired and max replicas, ephemeral storage, GPU type and count.
    type SelectionShape = (
        &'static str,
        f64,
        u64,
        u32,
        u32,
        u64,
        Option<(&'static str, u32)>,
    );

    #[rustfmt::skip]
    const SELECTION_SHAPES: &[SelectionShape] = &[
        ("tiny", 0.25, 512 * MI, 1, 1, 0, None),
        ("burstable", 1.0, 2 * GI, 1, 1, 10 * GI, None),
        ("burstable-big-memory", 0.5, 8 * GI, 1, 1, 0, None),
        ("small", 1.0, 2 * GI, 2, 4, 10 * GI, None),
        ("medium", 4.0, 8 * GI, 1, 1, 10 * GI, None),
        ("general", 2.0, 8 * GI, 4, 10, 20 * GI, None),
        ("many-replicas", 2.0, 4 * GI, 35, 35, 20 * GI, None),
        ("large-replica", 12.0, 48 * GI, 1, 2, 20 * GI, None),
        ("huge-replica", 60.0, 240 * GI, 1, 1, 20 * GI, None),
        ("memory-heavy", 4.0, 60 * GI, 2, 4, 20 * GI, None),
        ("storage-300g", 2.0, 8 * GI, 2, 2, 300 * GI, None),
        ("storage-1t", 4.0, 16 * GI, 2, 4, 1000 * GI, None),
        ("storage-2t", 4.0, 16 * GI, 2, 4, 2000 * GI, None),
        ("storage-8t", 2.0, 8 * GI, 2, 2, 8000 * GI, None),
        ("storage-100t", 2.0, 8 * GI, 1, 1, 100 * TI, None),
        ("gpu-t4", 2.0, 8 * GI, 1, 1, 10 * GI, Some(("nvidia-t4", 1))),
        ("gpu-a10g", 2.0, 8 * GI, 1, 1, 10 * GI, Some(("nvidia-a10g", 1))),
        ("gpu-a100", 4.0, 16 * GI, 1, 1, 10 * GI, Some(("nvidia-a100", 1))),
        ("gpu-a100x8", 16.0, 128 * GI, 1, 1, 10 * GI, Some(("nvidia-a100", 8))),
        ("gpu-h100x8", 16.0, 128 * GI, 1, 1, 10 * GI, Some(("nvidia-h100", 8))),
    ];

    /// One line per case: what `select_instance_type` picks for every platform, architecture,
    /// nested-virtualization setting and workload shape above.
    fn render_selection_matrix() -> String {
        let mut table = String::new();
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            for architecture in [None, Some(Architecture::Arm64), Some(Architecture::X86_64)] {
                for nested_virt in [false, true] {
                    for &(shape, cpu, memory, desired, max, storage, gpu) in SELECTION_SHAPES {
                        let requirements = WorkloadRequirements {
                            total_cpu_at_desired: cpu * desired as f64,
                            total_memory_bytes_at_desired: memory * desired as u64,
                            total_cpu_at_max: cpu * max as f64,
                            total_memory_bytes_at_max: memory * max as u64,
                            max_cpu_per_container: cpu,
                            max_memory_per_container: memory,
                            max_ephemeral_storage_bytes: storage,
                            gpu: gpu.map(|(gpu_type, count)| GpuSpec {
                                gpu_type: gpu_type.to_string(),
                                count,
                            }),
                            architecture,
                            nested_virt,
                        };
                        let pick = match select_instance_type(platform, &requirements) {
                            Ok(selection) => format!(
                                "{} min={} max={} memory={} storage={}",
                                selection.instance_type,
                                selection.min_machines,
                                selection.max_machines,
                                selection.profile.memory_bytes,
                                selection.profile.ephemeral_storage_bytes
                            ),
                            Err(error) => format!("error: {error}"),
                        };
                        let architecture = architecture
                            .map_or("default".to_string(), |architecture| {
                                format!("{architecture:?}")
                            });
                        table.push_str(&format!(
                            "{platform} {architecture} nested={nested_virt} {shape} -> {pick}\n"
                        ));
                    }
                }
            }
        }
        table
    }

    /// Matrix cases whose pick differs from the baseline on purpose, with the pick they get.
    /// AWS reports an NVIDIA A10G on g5, so a T4 request has no AWS machine and an A10G
    /// request gets g5.
    #[rustfmt::skip]
    const ACCEPTED_PICK_CHANGES: &[(&str, &str)] = &[
        (
            "aws default nested=false gpu-t4",
            "error: Arm64: architecture Arm64 is unavailable for this workload on platform aws; \
             X86_64: no instance type for GPU type 'nvidia-t4' x1 on platform aws",
        ),
        (
            "aws default nested=false gpu-a10g",
            "g5.xlarge min=1 max=1 memory=17179869184 storage=268435456000",
        ),
        (
            "aws X86_64 nested=false gpu-t4",
            "error: no instance type for GPU type 'nvidia-t4' x1 on platform aws",
        ),
        (
            "aws X86_64 nested=false gpu-a10g",
            "g5.xlarge min=1 max=1 memory=17179869184 storage=268435456000",
        ),
    ];

    /// Automatic picks, with the memory and disk they provision, are what existing stacks get
    /// on their next deploy, so a catalog change must leave them alone. The baseline holds the
    /// picks every case gets; only the cases in `ACCEPTED_PICK_CHANGES` may differ.
    #[test]
    fn automatic_picks_match_the_baseline_except_accepted_changes() {
        let baseline = include_str!("instance_catalog/auto_selection_baseline.txt");
        let rendered = render_selection_matrix();
        assert_eq!(baseline.lines().count(), rendered.lines().count());
        fn split(line: &str) -> (&str, &str) {
            line.split_once(" -> ").unwrap_or((line, ""))
        }
        for (before, now) in baseline.lines().zip(rendered.lines()) {
            let ((case, _), (now_case, pick)) = (split(before), split(now));
            assert_eq!(case, now_case);
            match ACCEPTED_PICK_CHANGES
                .iter()
                .find(|(accepted, _)| *accepted == case)
            {
                Some((_, accepted_pick)) => {
                    assert_ne!(
                        before, now,
                        "{case} no longer changes; drop it from the list"
                    );
                    assert_eq!(pick, *accepted_pick, "{case}");
                }
                None => assert_eq!(before, now, "automatic pick changed"),
            }
        }
    }

    type PinnedMachine = (
        Platform,
        &'static str,
        u32,
        u64,
        u64,
        Option<(&'static str, u32)>,
        bool,
    );

    /// Machines existing stacks may run, with the vCPU, memory, disk, GPU and nested
    /// virtualization they are planned with. Each must stay in the catalog with those values.
    #[rustfmt::skip]
    const PINNED_MACHINES: &[PinnedMachine] = &[
        (Platform::Aws, "t4g.micro", 2, GI, 20 * GI, None, false),
        (Platform::Aws, "t4g.small", 2, 2 * GI, 20 * GI, None, false),
        (Platform::Aws, "t4g.medium", 2, 4 * GI, 20 * GI, None, false),
        (Platform::Aws, "t4g.large", 2, 8 * GI, 20 * GI, None, false),
        (Platform::Aws, "t3.xlarge", 4, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "t4g.xlarge", 4, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "m7g.medium", 1, 4 * GI, 20 * GI, None, false),
        (Platform::Aws, "m7i.large", 2, 8 * GI, 20 * GI, None, false),
        (Platform::Aws, "m7g.large", 2, 8 * GI, 20 * GI, None, false),
        (Platform::Aws, "m8i.large", 2, 8 * GI, 20 * GI, None, true),
        (Platform::Aws, "m7i.xlarge", 4, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "m7g.xlarge", 4, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "m8i.xlarge", 4, 16 * GI, 20 * GI, None, true),
        (Platform::Aws, "m7i.2xlarge", 8, 32 * GI, 20 * GI, None, false),
        (Platform::Aws, "m7g.2xlarge", 8, 32 * GI, 20 * GI, None, false),
        (Platform::Aws, "m8i.2xlarge", 8, 32 * GI, 20 * GI, None, true),
        (Platform::Aws, "m7i.4xlarge", 16, 64 * GI, 20 * GI, None, false),
        (Platform::Aws, "m7g.4xlarge", 16, 64 * GI, 20 * GI, None, false),
        (Platform::Aws, "m8i.4xlarge", 16, 64 * GI, 20 * GI, None, true),
        (Platform::Aws, "c7g.medium", 1, 2 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8g.medium", 1, 2 * GI, 20 * GI, None, false),
        (Platform::Aws, "c7g.large", 2, 4 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8g.large", 2, 4 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8i.large", 2, 4 * GI, 20 * GI, None, true),
        (Platform::Aws, "c7g.xlarge", 4, 8 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8g.xlarge", 4, 8 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8i.xlarge", 4, 8 * GI, 20 * GI, None, true),
        (Platform::Aws, "c7g.2xlarge", 8, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8g.2xlarge", 8, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8i.2xlarge", 8, 16 * GI, 20 * GI, None, true),
        (Platform::Aws, "c7g.4xlarge", 16, 32 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8g.4xlarge", 16, 32 * GI, 20 * GI, None, false),
        (Platform::Aws, "c8i.4xlarge", 16, 32 * GI, 20 * GI, None, true),
        (Platform::Aws, "r7g.medium", 1, 8 * GI, 20 * GI, None, false),
        (Platform::Aws, "r7g.large", 2, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "r6a.large", 2, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "r7i.large", 2, 16 * GI, 20 * GI, None, false),
        (Platform::Aws, "r7g.xlarge", 4, 32 * GI, 20 * GI, None, false),
        (Platform::Aws, "r6a.xlarge", 4, 32 * GI, 20 * GI, None, false),
        (Platform::Aws, "r7i.xlarge", 4, 32 * GI, 20 * GI, None, false),
        (Platform::Aws, "r7g.2xlarge", 8, 64 * GI, 20 * GI, None, false),
        (Platform::Aws, "r6a.2xlarge", 8, 64 * GI, 20 * GI, None, false),
        (Platform::Aws, "r7i.2xlarge", 8, 64 * GI, 20 * GI, None, false),
        (Platform::Aws, "r7g.4xlarge", 16, 128 * GI, 20 * GI, None, false),
        (Platform::Aws, "r6a.4xlarge", 16, 128 * GI, 20 * GI, None, false),
        (Platform::Aws, "r7i.4xlarge", 16, 128 * GI, 20 * GI, None, false),
        (Platform::Aws, "i4i.xlarge", 4, 32 * GI, 937 * GI, None, false),
        (Platform::Aws, "i4i.2xlarge", 8, 64 * GI, 1875 * GI, None, false),
        (Platform::Aws, "i4i.4xlarge", 16, 128 * GI, 3750 * GI, None, false),
        (Platform::Aws, "i4i.8xlarge", 32, 256 * GI, 7500 * GI, None, false),
        (Platform::Aws, "g5.xlarge", 4, 16 * GI, 250 * GI, Some(("nvidia-t4", 1)), false),
        (Platform::Aws, "g5.2xlarge", 8, 32 * GI, 450 * GI, Some(("nvidia-t4", 1)), false),
        (Platform::Aws, "p4d.24xlarge", 96, 1152 * GI, 8000 * GI, Some(("nvidia-a100", 8)), false),
        (Platform::Aws, "p5.48xlarge", 192, 2048 * GI, 8000 * GI, Some(("nvidia-h100", 8)), false),
        (Platform::Gcp, "e2-micro", 2, GI, 20 * GI, None, false),
        (Platform::Gcp, "e2-small", 2, 2 * GI, 20 * GI, None, false),
        (Platform::Gcp, "e2-medium", 2, 4 * GI, 20 * GI, None, false),
        (Platform::Gcp, "n2-standard-2", 2, 8 * GI, 20 * GI, None, true),
        (Platform::Gcp, "n2-standard-4", 4, 16 * GI, 20 * GI, None, true),
        (Platform::Gcp, "n2-standard-8", 8, 32 * GI, 20 * GI, None, true),
        (Platform::Gcp, "n2-standard-16", 16, 64 * GI, 20 * GI, None, true),
        (Platform::Gcp, "c3-standard-4", 4, 8 * GI, 20 * GI, None, false),
        (Platform::Gcp, "c3-standard-8", 8, 16 * GI, 20 * GI, None, false),
        (Platform::Gcp, "n2-highmem-2", 2, 16 * GI, 20 * GI, None, false),
        (Platform::Gcp, "n2-highmem-4", 4, 32 * GI, 20 * GI, None, false),
        (Platform::Gcp, "n2-highmem-8", 8, 64 * GI, 20 * GI, None, false),
        (Platform::Gcp, "n2-highmem-16", 16, 128 * GI, 20 * GI, None, false),
        (Platform::Gcp, "n2-highmem-32", 32, 256 * GI, 20 * GI, None, false),
        (Platform::Gcp, "c3d-standard-8", 8, 32 * GI, 480 * GI, None, false),
        (Platform::Gcp, "c3d-standard-16", 16, 64 * GI, 960 * GI, None, false),
        (Platform::Gcp, "c3d-standard-30", 30, 120 * GI, 1920 * GI, None, false),
        (Platform::Gcp, "n1-standard-4-t4", 4, 15 * GI, 100 * GI, Some(("nvidia-t4", 1)), false),
        (Platform::Gcp, "a2-highgpu-1g", 12, 85 * GI, 100 * GI, Some(("nvidia-a100", 1)), false),
        (Platform::Gcp, "a2-highgpu-8g", 96, 1360 * GI, 100 * GI, Some(("nvidia-a100", 8)), false),
        (Platform::Gcp, "a3-highgpu-8g", 208, 1872 * GI, 100 * GI, Some(("nvidia-h100", 8)), false),
        (Platform::Azure, "Standard_B1s", 1, GI, 20 * GI, None, false),
        (Platform::Azure, "Standard_B2s", 2, 4 * GI, 20 * GI, None, false),
        (Platform::Azure, "Standard_B2ms", 2, 8 * GI, 20 * GI, None, false),
        (Platform::Azure, "Standard_B4ms", 4, 16 * GI, 20 * GI, None, false),
        (Platform::Azure, "Standard_D2s_v4", 2, 8 * GI, 20 * GI, None, false),
        (Platform::Azure, "Standard_D2s_v5", 2, 8 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_D4s_v5", 4, 16 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_D8s_v5", 8, 32 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_D16s_v5", 16, 64 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_D2s_v6", 2, 8 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_D4s_v6", 4, 16 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_D8s_v6", 8, 32 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_D16s_v6", 16, 64 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_F2s_v2", 2, 4 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_F4s_v2", 4, 8 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_F8s_v2", 8, 16 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_F16s_v2", 16, 32 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_E2s_v5", 2, 16 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_E4s_v5", 4, 32 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_E8s_v5", 8, 64 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_E16s_v5", 16, 128 * GI, 20 * GI, None, true),
        (Platform::Azure, "Standard_L8s_v3", 8, 64 * GI, 1788 * GI, None, false),
        (Platform::Azure, "Standard_L16s_v3", 16, 128 * GI, 3576 * GI, None, false),
        (Platform::Azure, "Standard_L32s_v3", 32, 256 * GI, 7154 * GI, None, false),
        (Platform::Azure, "Standard_NC4as_T4_v3", 4, 28 * GI, 176 * GI, Some(("nvidia-t4", 1)), false),
        (Platform::Azure, "Standard_NC24ads_A100_v4", 24, 220 * GI, 958 * GI, Some(("nvidia-a100", 1)), false),
        (Platform::Azure, "Standard_NC96ads_A100_v4", 96, 880 * GI, 3916 * GI, Some(("nvidia-a100", 4)), false),
        (Platform::Azure, "Standard_ND96isr_H100_v5", 96, 1900 * GI, 1000 * GI, Some(("nvidia-h100", 8)), false),
    ];

    /// Pinned machines whose GPU type the AWS API reports differently, with that type.
    const GPU_TYPE_CORRECTIONS: &[(&str, &str)] =
        &[("g5.xlarge", "nvidia-a10g"), ("g5.2xlarge", "nvidia-a10g")];

    #[test]
    fn every_pinned_machine_is_still_in_the_catalog_with_its_values() {
        for &(platform, name, vcpu, memory_bytes, storage_bytes, gpu, nested_virt) in
            PINNED_MACHINES
        {
            let spec = find_instance_type(platform, name)
                .unwrap_or_else(|| panic!("{platform} machine {name} left the catalog"));
            let gpu = gpu.map(|(gpu_type, count)| {
                let corrected = GPU_TYPE_CORRECTIONS
                    .iter()
                    .find(|(corrected, _)| *corrected == name);
                (corrected.map_or(gpu_type, |(_, gpu_type)| *gpu_type), count)
            });
            assert_eq!(
                (
                    spec.vcpu,
                    spec.memory_bytes,
                    spec.ephemeral_storage_bytes,
                    spec.gpu.map(|gpu| (gpu.gpu_type, gpu.count)),
                    spec.nested_virt,
                ),
                (vcpu, memory_bytes, storage_bytes, gpu, nested_virt),
                "{platform} {name}"
            );
        }
    }

    #[test]
    fn automatic_selection_considers_exactly_the_pinned_machines() {
        let pinned: std::collections::HashSet<_> = PINNED_MACHINES
            .iter()
            .map(|&(platform, name, ..)| (platform, name))
            .collect();
        let auto_select: std::collections::HashSet<_> = CATALOG
            .iter()
            .filter(|spec| spec.auto_select)
            .map(|spec| (spec.platform, spec.name))
            .collect();
        assert_eq!(auto_select, pinned);
    }

    fn general_requirements() -> WorkloadRequirements {
        WorkloadRequirements {
            total_cpu_at_desired: 3.0,
            total_memory_bytes_at_desired: 12 * GI,
            total_cpu_at_max: 3.0,
            total_memory_bytes_at_max: 12 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
            architecture: None,
            nested_virt: true,
        }
    }

    #[test]
    fn azure_auto_selection_prefers_dsv5_then_dsv6_in_catalog_order() {
        let series = auto_select_series(Platform::Azure);
        let position = |name| series.iter().position(|series| *series == name);
        assert!(
            position("Dsv5").unwrap() < position("Dsv6").unwrap(),
            "{series:?}"
        );
    }

    #[test]
    fn with_room_everywhere_the_pick_is_the_plain_pick() {
        let requirements = general_requirements();
        let plain = select_instance_type(Platform::Azure, &requirements).unwrap();
        let with_room =
            select_instance_type_with_room(Platform::Azure, &requirements, 2, &|_, _| true)
                .unwrap();
        assert_eq!(plain.instance_type, with_room.instance_type);
        assert!(
            with_room.instance_type.ends_with("_v5"),
            "{}",
            with_room.instance_type
        );
    }

    #[test]
    fn a_series_without_room_is_skipped_for_the_next_one() {
        let requirements = general_requirements();
        let selection =
            select_instance_type_with_room(Platform::Azure, &requirements, 2, &|spec, _| {
                spec.series != "Dsv5"
            })
            .unwrap();
        assert_eq!(selection.instance_type, "Standard_D4s_v6");
    }

    #[test]
    fn room_is_asked_for_the_requested_machines() {
        let requirements = general_requirements();
        let asked = std::cell::RefCell::new(Vec::new());
        select_instance_type_with_room(Platform::Azure, &requirements, 3, &|spec, machines| {
            asked.borrow_mut().push((spec.series, machines));
            true
        })
        .unwrap();
        assert!(asked.borrow().iter().all(|(_, machines)| *machines == 3));
    }

    #[test]
    fn with_no_room_anywhere_the_plain_pick_is_kept_for_the_quota_refusal() {
        let requirements = general_requirements();
        let plain = select_instance_type(Platform::Azure, &requirements).unwrap();
        let none = select_instance_type_with_room(Platform::Azure, &requirements, 2, &|_, _| false)
            .unwrap();
        assert_eq!(plain.instance_type, none.instance_type);
    }

    #[test]
    fn catalog_lists_no_bare_metal_machine() {
        for spec in CATALOG {
            assert!(!spec.name.contains("metal"), "{}", spec.name);
        }
    }

    /// The generator writes each series as one run of rows, smallest first.
    #[test]
    fn test_catalog_instance_types_sorted_by_vcpu_within_series() {
        let mut finished = std::collections::HashSet::new();
        for window in CATALOG.windows(2) {
            let (previous, next) = (&window[0], &window[1]);
            if (previous.platform, previous.series) == (next.platform, next.series) {
                assert!(
                    previous.vcpu <= next.vcpu,
                    "{} ({}) before {} ({})",
                    previous.name,
                    previous.vcpu,
                    next.name,
                    next.vcpu
                );
            } else {
                finished.insert((previous.platform, previous.series));
                assert!(
                    !finished.contains(&(next.platform, next.series)),
                    "series {} is split",
                    next.series
                );
            }
        }
    }
}
