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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    Arm64,
    X86_64,
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
    /// Convert this catalog entry into a `MachineProfile` for use in `CapacityGroup`.
    pub fn to_machine_profile(&self) -> MachineProfile {
        MachineProfile {
            cpu: format!("{}.0", self.vcpu),
            memory_bytes: self.memory_bytes,
            ephemeral_storage_bytes: self.ephemeral_storage_bytes,
            gpu: self.gpu.map(|g| GpuSpec {
                gpu_type: g.gpu_type.to_string(),
                count: g.count,
            }),
        }
    }
}

// Helpers for readable byte constants
const KI: u64 = 1024;
const MI: u64 = KI * 1024;
const GI: u64 = MI * 1024;

/// The complete instance type catalog.
///
/// This is the single source of truth for instance type specifications.
/// Update this array when adding support for new instance types.
///
/// NOTE: Ephemeral storage values for non-NVMe instances are conservative defaults
/// (EBS-backed root volumes). Storage-optimized instances list their NVMe capacity.
static CATALOG: &[InstanceTypeSpec] = &[
    // =========================================================================
    // AWS — ARM (Graviton) preferred for cost efficiency
    // =========================================================================

    // Burstable (t4g — ARM Graviton2)
    InstanceTypeSpec {
        name: "t4g.micro",
        platform: Platform::Aws,
        family: InstanceFamily::Burstable,
        architecture: Architecture::Arm64,
        vcpu: 2,
        memory_bytes: 1 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "t4g.small",
        platform: Platform::Aws,
        family: InstanceFamily::Burstable,
        architecture: Architecture::Arm64,
        vcpu: 2,
        memory_bytes: 2 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "t4g.medium",
        platform: Platform::Aws,
        family: InstanceFamily::Burstable,
        architecture: Architecture::Arm64,
        vcpu: 2,
        memory_bytes: 4 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "t4g.large",
        platform: Platform::Aws,
        family: InstanceFamily::Burstable,
        architecture: Architecture::Arm64,
        vcpu: 2,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "t4g.xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::Burstable,
        architecture: Architecture::Arm64,
        vcpu: 4,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // General Purpose (m7g — ARM Graviton3, up to 2xlarge / 8 vCPU)
    InstanceTypeSpec {
        name: "m7g.medium",
        platform: Platform::Aws,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::Arm64,
        vcpu: 1,
        memory_bytes: 4 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "m7g.large",
        platform: Platform::Aws,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::Arm64,
        vcpu: 2,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "m7g.xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::Arm64,
        vcpu: 4,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "m7g.2xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::Arm64,
        vcpu: 8,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "m7g.4xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::Arm64,
        vcpu: 16,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Compute Optimized (c7g — ARM Graviton3, up to 2xlarge / 8 vCPU)
    InstanceTypeSpec {
        name: "c7g.medium",
        platform: Platform::Aws,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::Arm64,
        vcpu: 1,
        memory_bytes: 2 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "c7g.large",
        platform: Platform::Aws,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::Arm64,
        vcpu: 2,
        memory_bytes: 4 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "c7g.xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::Arm64,
        vcpu: 4,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "c7g.2xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::Arm64,
        vcpu: 8,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "c7g.4xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::Arm64,
        vcpu: 16,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Memory Optimized (r7g — ARM Graviton3, up to 2xlarge / 8 vCPU)
    InstanceTypeSpec {
        name: "r7g.medium",
        platform: Platform::Aws,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::Arm64,
        vcpu: 1,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "r7g.large",
        platform: Platform::Aws,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::Arm64,
        vcpu: 2,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "r7g.xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::Arm64,
        vcpu: 4,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "r7g.2xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::Arm64,
        vcpu: 8,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "r7g.4xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::Arm64,
        vcpu: 16,
        memory_bytes: 128 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Storage Optimized (i4i — x86_64, NVMe)
    InstanceTypeSpec {
        name: "i4i.xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 937 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "i4i.2xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 1875 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "i4i.4xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 16,
        memory_bytes: 128 * GI,
        ephemeral_storage_bytes: 3750 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "i4i.8xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 32,
        memory_bytes: 256 * GI,
        ephemeral_storage_bytes: 7500 * GI,
        gpu: None,
    },
    // GPU — NVIDIA T4 (g5 — x86_64)
    InstanceTypeSpec {
        name: "g5.xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 250 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-t4",
            count: 1,
        }),
    },
    InstanceTypeSpec {
        name: "g5.2xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 450 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-t4",
            count: 1,
        }),
    },
    // GPU — NVIDIA A100 (p4d — x86_64)
    InstanceTypeSpec {
        name: "p4d.24xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 96,
        memory_bytes: 1152 * GI,
        ephemeral_storage_bytes: 8000 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-a100",
            count: 8,
        }),
    },
    // GPU — NVIDIA H100 (p5 — x86_64)
    InstanceTypeSpec {
        name: "p5.48xlarge",
        platform: Platform::Aws,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 192,
        memory_bytes: 2048 * GI,
        ephemeral_storage_bytes: 8000 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-h100",
            count: 8,
        }),
    },
    // =========================================================================
    // GCP
    // =========================================================================

    // Burstable (e2)
    InstanceTypeSpec {
        name: "e2-micro",
        platform: Platform::Gcp,
        family: InstanceFamily::Burstable,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 1 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "e2-small",
        platform: Platform::Gcp,
        family: InstanceFamily::Burstable,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 2 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "e2-medium",
        platform: Platform::Gcp,
        family: InstanceFamily::Burstable,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 4 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // General Purpose (n2-standard, up to 16 vCPU)
    InstanceTypeSpec {
        name: "n2-standard-2",
        platform: Platform::Gcp,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "n2-standard-4",
        platform: Platform::Gcp,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "n2-standard-8",
        platform: Platform::Gcp,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "n2-standard-16",
        platform: Platform::Gcp,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::X86_64,
        vcpu: 16,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Compute Optimized (c3-standard, up to 8 vCPU)
    InstanceTypeSpec {
        name: "c3-standard-4",
        platform: Platform::Gcp,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "c3-standard-8",
        platform: Platform::Gcp,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Memory Optimized (n2-highmem, up to 8 vCPU)
    InstanceTypeSpec {
        name: "n2-highmem-2",
        platform: Platform::Gcp,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "n2-highmem-4",
        platform: Platform::Gcp,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "n2-highmem-8",
        platform: Platform::Gcp,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "n2-highmem-16",
        platform: Platform::Gcp,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 16,
        memory_bytes: 128 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "n2-highmem-32",
        platform: Platform::Gcp,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 32,
        memory_bytes: 256 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Storage Optimized (c3d-standard with local SSD)
    InstanceTypeSpec {
        name: "c3d-standard-8",
        platform: Platform::Gcp,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 480 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "c3d-standard-16",
        platform: Platform::Gcp,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 16,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 960 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "c3d-standard-30",
        platform: Platform::Gcp,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 30,
        memory_bytes: 120 * GI,
        ephemeral_storage_bytes: 1920 * GI,
        gpu: None,
    },
    // GPU — NVIDIA T4 (n1-standard + T4)
    InstanceTypeSpec {
        name: "n1-standard-4-t4",
        platform: Platform::Gcp,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 15 * GI,
        ephemeral_storage_bytes: 100 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-t4",
            count: 1,
        }),
    },
    // GPU — NVIDIA A100 (a2-highgpu)
    InstanceTypeSpec {
        name: "a2-highgpu-1g",
        platform: Platform::Gcp,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 12,
        memory_bytes: 85 * GI,
        ephemeral_storage_bytes: 100 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-a100",
            count: 1,
        }),
    },
    InstanceTypeSpec {
        name: "a2-highgpu-8g",
        platform: Platform::Gcp,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 96,
        memory_bytes: 1360 * GI,
        ephemeral_storage_bytes: 100 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-a100",
            count: 8,
        }),
    },
    // GPU — NVIDIA H100 (a3-highgpu)
    InstanceTypeSpec {
        name: "a3-highgpu-8g",
        platform: Platform::Gcp,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 208,
        memory_bytes: 1872 * GI,
        ephemeral_storage_bytes: 100 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-h100",
            count: 8,
        }),
    },
    // =========================================================================
    // Azure
    // =========================================================================

    // Burstable (B-series v2)
    InstanceTypeSpec {
        name: "Standard_B1s",
        platform: Platform::Azure,
        family: InstanceFamily::Burstable,
        architecture: Architecture::X86_64,
        vcpu: 1,
        memory_bytes: 1 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_B2s",
        platform: Platform::Azure,
        family: InstanceFamily::Burstable,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 4 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_B2ms",
        platform: Platform::Azure,
        family: InstanceFamily::Burstable,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_B4ms",
        platform: Platform::Azure,
        family: InstanceFamily::Burstable,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // General Purpose (Dv5-series, up to 16 vCPU)
    InstanceTypeSpec {
        name: "Standard_D2s_v5",
        platform: Platform::Azure,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_D4s_v5",
        platform: Platform::Azure,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_D8s_v5",
        platform: Platform::Azure,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_D16s_v5",
        platform: Platform::Azure,
        family: InstanceFamily::GeneralPurpose,
        architecture: Architecture::X86_64,
        vcpu: 16,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Compute Optimized (Fv2-series, up to 16 vCPU)
    InstanceTypeSpec {
        name: "Standard_F2s_v2",
        platform: Platform::Azure,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 4 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_F4s_v2",
        platform: Platform::Azure,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 8 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_F8s_v2",
        platform: Platform::Azure,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_F16s_v2",
        platform: Platform::Azure,
        family: InstanceFamily::ComputeOptimized,
        architecture: Architecture::X86_64,
        vcpu: 16,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Memory Optimized (Ev5-series, up to 16 vCPU)
    InstanceTypeSpec {
        name: "Standard_E2s_v5",
        platform: Platform::Azure,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 2,
        memory_bytes: 16 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_E4s_v5",
        platform: Platform::Azure,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 32 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_E8s_v5",
        platform: Platform::Azure,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_E16s_v5",
        platform: Platform::Azure,
        family: InstanceFamily::MemoryOptimized,
        architecture: Architecture::X86_64,
        vcpu: 16,
        memory_bytes: 128 * GI,
        ephemeral_storage_bytes: 20 * GI,
        gpu: None,
    },
    // Storage Optimized (Lsv3-series with NVMe)
    InstanceTypeSpec {
        name: "Standard_L8s_v3",
        platform: Platform::Azure,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 8,
        memory_bytes: 64 * GI,
        ephemeral_storage_bytes: 1788 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_L16s_v3",
        platform: Platform::Azure,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 16,
        memory_bytes: 128 * GI,
        ephemeral_storage_bytes: 3576 * GI,
        gpu: None,
    },
    InstanceTypeSpec {
        name: "Standard_L32s_v3",
        platform: Platform::Azure,
        family: InstanceFamily::StorageOptimized,
        architecture: Architecture::X86_64,
        vcpu: 32,
        memory_bytes: 256 * GI,
        ephemeral_storage_bytes: 7154 * GI,
        gpu: None,
    },
    // GPU — NVIDIA T4 (NCasT4_v3-series)
    InstanceTypeSpec {
        name: "Standard_NC4as_T4_v3",
        platform: Platform::Azure,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 4,
        memory_bytes: 28 * GI,
        ephemeral_storage_bytes: 176 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-t4",
            count: 1,
        }),
    },
    // GPU — NVIDIA A100 (NC A100 v4-series)
    InstanceTypeSpec {
        name: "Standard_NC24ads_A100_v4",
        platform: Platform::Azure,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 24,
        memory_bytes: 220 * GI,
        ephemeral_storage_bytes: 958 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-a100",
            count: 1,
        }),
    },
    InstanceTypeSpec {
        name: "Standard_NC96ads_A100_v4",
        platform: Platform::Azure,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 96,
        memory_bytes: 880 * GI,
        ephemeral_storage_bytes: 3916 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-a100",
            count: 4,
        }),
    },
    // GPU — NVIDIA H100 (ND H100 v5-series)
    InstanceTypeSpec {
        name: "Standard_ND96isr_H100_v5",
        platform: Platform::Azure,
        family: InstanceFamily::GpuCompute,
        architecture: Architecture::X86_64,
        vcpu: 96,
        memory_bytes: 1900 * GI,
        ephemeral_storage_bytes: 1000 * GI,
        gpu: Some(CatalogGpu {
            gpu_type: "nvidia-h100",
            count: 8,
        }),
    },
];

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

/// Maximum number of machines per cluster (Horizon limit).
const MAX_MACHINES_PER_CLUSTER: u32 = 10;

/// Hard cap on vCPUs for non-GPU/non-storage workloads. Equivalent to AWS 2xlarge.
/// Beyond this, horizontal scaling is always preferred over bigger machines.
const MAX_STANDARD_VCPU: u32 = 8;

/// How many of the largest container we want to fit per machine (for bin-packing).
const CONTAINERS_PER_MACHINE: f64 = 4.0;

/// Overhead factor for system processes and bin-packing inefficiency.
const OVERHEAD_FACTOR: f64 = 1.25;

/// Select the best instance type for a workload on a given platform.
///
/// The algorithm:
/// 1. GPU workloads: Match by GPU type, find smallest instance with enough GPUs.
/// 2. Storage-heavy workloads (>200Gi ephemeral): Use storage-optimized instances.
/// 3. All other workloads: Size the machine to fit ~4 of the largest container
///    with overhead, capped at 8 vCPUs. Use GeneralPurpose family for broad
///    availability and reasonable cost. Scale horizontally for more capacity.
///
/// Returns an error if no suitable instance type is found.
pub fn select_instance_type(
    platform: Platform,
    requirements: &WorkloadRequirements,
) -> Result<InstanceSelection, String> {
    // Determine which family to use
    let family = select_family(requirements);

    // Filter catalog to matching platform + family
    let candidates: Vec<&InstanceTypeSpec> = CATALOG
        .iter()
        .filter(|spec| spec.platform == platform && spec.family == family)
        .collect();

    if candidates.is_empty() {
        return Err(format!(
            "no {family:?} instance types in catalog for platform {platform}"
        ));
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

    // Size the instance based on the largest single container, not total workload.
    // Target: fit ~4 of the largest container per machine with overhead.
    let target_cpu =
        (requirements.max_cpu_per_container * CONTAINERS_PER_MACHINE * OVERHEAD_FACTOR).max(0.25);
    let target_memory =
        (requirements.max_memory_per_container as f64 * CONTAINERS_PER_MACHINE * OVERHEAD_FACTOR)
            .max(256.0 * MI as f64);

    // Cap at MAX_STANDARD_VCPU for non-GPU/non-storage workloads
    let vcpu_cap =
        if family == InstanceFamily::GpuCompute || family == InstanceFamily::StorageOptimized {
            u32::MAX
        } else {
            MAX_STANDARD_VCPU
        };

    // Find the smallest instance that meets per-container targets within the cap.
    let selected = candidates
        .iter()
        .filter(|spec| {
            spec.vcpu <= vcpu_cap
                && spec.vcpu as f64 >= target_cpu
                && spec.memory_bytes as f64 >= target_memory
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
    let min_machines = compute_min_machines(max_machines);

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
    // How many machines to fit total CPU at max scale (with 25% headroom)
    let cpu_with_headroom = requirements.total_cpu_at_max * 1.25;
    let cpu_machines = (cpu_with_headroom / instance.vcpu as f64).ceil() as u32;

    // How many machines to fit total memory at max scale (with 25% headroom)
    let mem_with_headroom = requirements.total_memory_bytes_at_max as f64 * 1.25;
    let mem_machines = (mem_with_headroom / instance.memory_bytes as f64).ceil() as u32;

    // Take the larger of CPU-based and memory-based, clamped to cluster limit
    cpu_machines
        .max(mem_machines)
        .max(1)
        .min(MAX_MACHINES_PER_CLUSTER)
}

/// Calculate minimum machines for HA.
fn compute_min_machines(max_machines: u32) -> u32 {
    // At least 1, at most 2 for HA (larger min for larger clusters)
    if max_machines >= 3 {
        2
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_select_burstable_for_small_workload() {
        let req = WorkloadRequirements {
            total_cpu_at_max: 1.0,
            total_memory_bytes_at_max: 2 * GI,
            max_cpu_per_container: 0.5,
            max_memory_per_container: 1 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::Burstable);
    }

    #[test]
    fn test_select_general_purpose_for_standard_workload() {
        // Standard workloads always get GeneralPurpose regardless of CPU:memory ratio
        let req = WorkloadRequirements {
            total_cpu_at_max: 20.0,
            total_memory_bytes_at_max: 80 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
    }

    #[test]
    fn test_select_general_purpose_even_for_cpu_heavy() {
        // CPU-heavy workloads still get GeneralPurpose (no more ComputeOptimized auto-select)
        let req = WorkloadRequirements {
            total_cpu_at_max: 20.0,
            total_memory_bytes_at_max: 20 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 2 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::GeneralPurpose);
    }

    #[test]
    fn test_select_storage_optimized_for_large_ephemeral() {
        let req = WorkloadRequirements {
            total_cpu_at_max: 8.0,
            total_memory_bytes_at_max: 32 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 8 * GI,
            max_ephemeral_storage_bytes: 500 * GI,
            gpu: None,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::StorageOptimized);
    }

    #[test]
    fn test_select_gpu_instance() {
        let req = WorkloadRequirements {
            total_cpu_at_max: 8.0,
            total_memory_bytes_at_max: 32 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 16 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: Some(GpuSpec {
                gpu_type: "nvidia-a100".to_string(),
                count: 1,
            }),
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        let spec = find_instance_type(Platform::Aws, sel.instance_type).unwrap();
        assert_eq!(spec.family, InstanceFamily::GpuCompute);
        assert!(spec.gpu.is_some());
    }

    #[test]
    fn test_select_works_for_all_cloud_platforms() {
        let req = WorkloadRequirements {
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 16 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
        };
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let sel = select_instance_type(platform, &req);
            assert!(sel.is_ok(), "selection failed for {platform}");
        }
    }

    #[test]
    fn test_machine_count_reasonable() {
        // Single container: 1 CPU, 2Gi, maxReplicas=20
        let req = WorkloadRequirements {
            total_cpu_at_max: 20.0,
            total_memory_bytes_at_max: 40 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 2 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        assert!(sel.min_machines >= 1);
        assert!(sel.max_machines <= MAX_MACHINES_PER_CLUSTER);
        assert!(sel.max_machines >= sel.min_machines);
    }

    #[test]
    fn test_instance_size_capped_at_8_vcpu() {
        // Even with very large containers, instance size is capped at 8 vCPUs
        let req = WorkloadRequirements {
            total_cpu_at_max: 70.0,
            total_memory_bytes_at_max: 140 * GI,
            max_cpu_per_container: 2.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
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
    fn test_manager_stack_gets_reasonable_instance() {
        // Simulates the manager stack: 4 containers, each 2 CPU / 4 GiB
        // maxReplicas: 10, 10, 10, 5
        let req = WorkloadRequirements {
            total_cpu_at_max: 70.0,              // 2*10 + 2*10 + 2*10 + 2*5
            total_memory_bytes_at_max: 140 * GI, // 4*10 + 4*10 + 4*10 + 4*5
            max_cpu_per_container: 2.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 20 * GI,
            gpu: None,
        };
        let sel = select_instance_type(Platform::Gcp, &req).unwrap();
        // Should pick n2-standard-8 (8 vCPU, 32 GiB) — NOT c3-standard-44
        assert_eq!(sel.instance_type, "n2-standard-8");
        assert!(sel.max_machines >= 2);
    }

    #[test]
    fn test_profile_has_required_fields() {
        let req = WorkloadRequirements {
            total_cpu_at_max: 4.0,
            total_memory_bytes_at_max: 16 * GI,
            max_cpu_per_container: 1.0,
            max_memory_per_container: 4 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: None,
        };
        let sel = select_instance_type(Platform::Aws, &req).unwrap();
        assert!(!sel.profile.cpu.is_empty());
        assert!(sel.profile.memory_bytes > 0);
        assert!(sel.profile.ephemeral_storage_bytes > 0);
    }

    #[test]
    fn test_error_for_unsupported_gpu_type() {
        let req = WorkloadRequirements {
            total_cpu_at_max: 8.0,
            total_memory_bytes_at_max: 32 * GI,
            max_cpu_per_container: 4.0,
            max_memory_per_container: 16 * GI,
            max_ephemeral_storage_bytes: 10 * GI,
            gpu: Some(GpuSpec {
                gpu_type: "amd-mi300".to_string(),
                count: 1,
            }),
        };
        let result = select_instance_type(Platform::Aws, &req);
        assert!(result.is_err());
    }

    #[test]
    fn test_catalog_instance_types_sorted_by_vcpu_within_family() {
        // Verify that within each (platform, family) group, vcpu is non-decreasing.
        // This ensures our "min_by_key(vcpu)" logic works correctly.
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let entries = catalog_for_platform(platform);
            let mut by_family: std::collections::HashMap<_, Vec<_>> =
                std::collections::HashMap::new();
            for entry in entries {
                by_family
                    .entry(format!("{:?}", entry.family))
                    .or_default()
                    .push(entry);
            }
            for (family, instances) in &by_family {
                for window in instances.windows(2) {
                    assert!(
                        window[0].vcpu <= window[1].vcpu,
                        "catalog not sorted by vcpu for {platform}/{family}: {} ({}) > {} ({})",
                        window[0].name,
                        window[0].vcpu,
                        window[1].name,
                        window[1].vcpu
                    );
                }
            }
        }
    }
}
