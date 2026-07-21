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
    let spec = find_instance_type(Platform::Aws, "m7g.2xlarge").expect("should find m7g.2xlarge");
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
    // Even with very large containers, instance size is capped at 8 vCPUs
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

    let error =
        select_instance_type(Platform::Gcp, &req).expect_err("GCP catalog has no ARM64 machine");

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

#[test]
fn test_catalog_instance_types_sorted_by_vcpu_within_family() {
    // Verify that within each (platform, family) group, vcpu is non-decreasing.
    // This ensures our "min_by_key(vcpu)" logic works correctly.
    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let entries = catalog_for_platform(platform);
        let mut by_family: std::collections::HashMap<_, Vec<_>> = std::collections::HashMap::new();
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
