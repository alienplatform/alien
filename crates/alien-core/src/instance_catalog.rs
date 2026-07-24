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

mod catalog;
use catalog::CATALOG;

const KI: u64 = 1024;
const MI: u64 = KI * 1024;
const GI: u64 = MI * 1024;

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

    // Binary suffixes (powers of 1024)
    if let Some(num) = s.strip_suffix("Ti") {
        let v: f64 = num
            .parse()
            .map_err(|_| format!("invalid memory value: '{s}'"))?;
        return Ok((v * 1024.0 * 1024.0 * 1024.0 * 1024.0) as u64);
    }
    if let Some(num) = s.strip_suffix("Gi") {
        let v: f64 = num
            .parse()
            .map_err(|_| format!("invalid memory value: '{s}'"))?;
        return Ok((v * 1024.0 * 1024.0 * 1024.0) as u64);
    }
    if let Some(num) = s.strip_suffix("Mi") {
        let v: f64 = num
            .parse()
            .map_err(|_| format!("invalid memory value: '{s}'"))?;
        return Ok((v * 1024.0 * 1024.0) as u64);
    }
    if let Some(num) = s.strip_suffix("Ki") {
        let v: f64 = num
            .parse()
            .map_err(|_| format!("invalid memory value: '{s}'"))?;
        return Ok((v * 1024.0) as u64);
    }

    // Decimal suffixes (powers of 1000)
    if let Some(num) = s.strip_suffix('T') {
        let v: f64 = num
            .parse()
            .map_err(|_| format!("invalid memory value: '{s}'"))?;
        return Ok((v * 1_000_000_000_000.0) as u64);
    }
    if let Some(num) = s.strip_suffix('G') {
        let v: f64 = num
            .parse()
            .map_err(|_| format!("invalid memory value: '{s}'"))?;
        return Ok((v * 1_000_000_000.0) as u64);
    }
    if let Some(num) = s.strip_suffix('M') {
        let v: f64 = num
            .parse()
            .map_err(|_| format!("invalid memory value: '{s}'"))?;
        return Ok((v * 1_000_000.0) as u64);
    }
    if let Some(num) = s.strip_suffix('k') {
        let v: f64 = num
            .parse()
            .map_err(|_| format!("invalid memory value: '{s}'"))?;
        return Ok((v * 1000.0) as u64);
    }

    // Plain bytes
    s.parse()
        .map_err(|_| format!("invalid memory value: '{s}'"))
}

// ---------------------------------------------------------------------------
// Instance type catalog
// ---------------------------------------------------------------------------

/// Instance family classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub family: InstanceFamily,
    pub architecture: Architecture,
    /// vCPU count (hardware total)
    pub vcpu: u32,
    /// Memory in bytes (hardware total)
    pub memory_bytes: u64,
    /// Ephemeral storage in bytes (hardware total, NVMe for storage-optimized)
    pub ephemeral_storage_bytes: u64,
    /// GPU specification (for GPU instances)
    pub gpu: Option<CatalogGpu>,
}

impl InstanceTypeSpec {
    /// Whether this instance type supports
    /// `CpuOptions.NestedVirtualization=enabled` on AWS launch.
    ///
    /// Per AWS docs (`aws ec2 create-launch-template help`), nested
    /// virtualization is only supported on 8th-generation Intel instance
    /// types: c8i, m8i, r8i, and their `-flex` variants. We classify by
    /// family-name prefix rather than a per-row bool so the existing 70+
    /// catalog rows don't need an extra field.
    pub fn is_nested_virt_capable(&self) -> bool {
        if self.platform != Platform::Aws {
            // GCP/Azure equivalents would need their own family lists.
            // Today nested virt is wired through only for AWS.
            return false;
        }
        let name = self.name;
        name.starts_with("m8i.")
            || name.starts_with("c8i.")
            || name.starts_with("r8i.")
            || name.starts_with("m8i-flex.")
            || name.starts_with("c8i-flex.")
            || name.starts_with("r8i-flex.")
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
        }
    }
}

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

/// Hard cap on vCPUs for non-GPU/non-storage workloads. Equivalent to AWS 2xlarge.
/// Beyond this, horizontal scaling is always preferred over bigger machines.
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
/// 3. All other workloads: Size the machine to fit a small HA-friendly baseline,
///    capped at 8 vCPUs. Use GeneralPurpose family for broad availability and
///    reasonable cost. Scale horizontally for more capacity.
///
/// Returns an error if no suitable instance type is found.
pub fn select_instance_type(
    platform: Platform,
    requirements: &WorkloadRequirements,
) -> Result<InstanceSelection, String> {
    // Determine which family to use. Nested virt isn't available on
    // burstable hardware on any cloud, so a workload that classifies as
    // Burstable but needs nested virt must be upgraded to GeneralPurpose
    // (the family that actually has nested-virt-capable entries).
    let raw_family = select_family(requirements);
    let family = if requirements.nested_virt && raw_family == InstanceFamily::Burstable {
        InstanceFamily::GeneralPurpose
    } else {
        raw_family
    };

    let candidates: Vec<&InstanceTypeSpec> = CATALOG
        .iter()
        .filter(|spec| spec.platform == platform && spec.family == family)
        .filter(|spec| {
            if requirements.nested_virt {
                spec.is_nested_virt_capable()
            } else {
                !spec.is_nested_virt_capable()
            }
        })
        .collect();

    if candidates.is_empty() {
        return Err(if requirements.nested_virt {
            format!(
                "no nested-virt-capable {family:?} instance types in catalog for platform {platform}; \
                 only 8th-gen Intel families (m8i/c8i/r8i) support nested virtualization on AWS"
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
            return Err(format!(
                "no storage-optimized instance with >= {} bytes ephemeral storage on platform {platform}",
                requirements.max_ephemeral_storage_bytes
            ));
        }
        filtered
    } else {
        candidates
    };

    let architecture = requirements
        .architecture
        .or_else(|| default_architecture(platform))
        .ok_or_else(|| format!("platform {platform} has no default compute architecture"))?;
    let candidates: Vec<&InstanceTypeSpec> = candidates
        .into_iter()
        .filter(|spec| spec.architecture == architecture)
        .collect();
    if candidates.is_empty() {
        return Err(format!(
            "architecture {architecture:?} is unavailable for this workload on platform {platform}"
        ));
    }

    // Cap at MAX_STANDARD_VCPU for non-GPU/non-storage workloads
    let vcpu_cap =
        if family == InstanceFamily::GpuCompute || family == InstanceFamily::StorageOptimized {
            u32::MAX
        } else {
            MAX_STANDARD_VCPU
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
            candidates
                .iter()
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
        profile: selected.to_machine_profile(),
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

fn allocatable_cpu(instance: &InstanceTypeSpec) -> f64 {
    (instance.vcpu as f64 - SYSTEM_RESERVE_CPU).max(0.25)
}

fn allocatable_memory_bytes(instance: &InstanceTypeSpec) -> u64 {
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
mod tests;
