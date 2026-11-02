use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{ComputeCluster, Platform, Stack};

const GIB: u64 = 1024 * 1024 * 1024;

/// Validates pools that require an additional empty disk on each machine.
///
/// On AWS, GCP and Azure the runtime controller attaches the disk, so a size
/// the provider cannot create fails here, before any cloud resource exists.
/// Machines checks the disk when a host joins. Kubernetes cannot provide it,
/// so declaring one there is an error instead of a requirement that silently
/// goes unmet.
pub struct ComputePoolDataDiskCheck;

/// Largest data disk each provider can create, in GiB: gp3 EBS volumes and
/// pd-balanced persistent disks hold up to 64 TiB, Azure Premium SSD managed
/// data disks up to 32,767 GiB.
fn max_data_disk_gib(platform: Platform) -> Option<u64> {
    match platform {
        Platform::Aws | Platform::Gcp => Some(64 * 1024),
        Platform::Azure => Some(32_767),
        Platform::Machines | Platform::Kubernetes | Platform::Local | Platform::Test => None,
    }
}

#[async_trait::async_trait]
impl CompileTimeCheck for ComputePoolDataDiskCheck {
    fn code(&self) -> Option<&'static str> {
        Some("COMPUTE_POOL_DATA_DISK_INVALID")
    }

    fn description(&self) -> &'static str {
        "Compute pool data disks must have a size the platform can provide"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        data_disk_pools(stack).next().is_some()
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        for (cluster_id, group_id, size_bytes) in data_disk_pools(stack) {
            let pool = format!("ComputeCluster '{cluster_id}' pool '{group_id}'");
            if size_bytes == 0 {
                errors.push(format!(
                    "{pool}: requirements.dataDisk.size must be greater than zero."
                ));
            }
            match platform {
                Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Machines => {}
                Platform::Local | Platform::Test => warnings.push(format!(
                    "{pool} requires an additional empty disk on each machine. Platform '{platform}' does not provide or check one."
                )),
                Platform::Kubernetes => errors.push(format!(
                    "{pool} requires an additional empty disk on each machine, which Alien cannot attach to nodes on platform 'kubernetes'."
                )),
            }
            if let Some(max_gib) = max_data_disk_gib(platform) {
                let size_gib = size_bytes.div_ceil(GIB);
                if size_gib > max_gib {
                    errors.push(format!(
                        "{pool}: requirements.dataDisk.size is {size_gib} GiB, but the largest data disk on platform '{platform}' is {max_gib} GiB."
                    ));
                }
            }
        }

        Ok(match (errors.is_empty(), warnings.is_empty()) {
            (true, true) => CheckResult::success(),
            (true, false) => CheckResult::with_warnings(warnings),
            (false, _) => CheckResult::failed_with_warnings(errors, warnings),
        })
    }
}

fn data_disk_pools(stack: &Stack) -> impl Iterator<Item = (&str, &str, u64)> {
    stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<ComputeCluster>())
        .flat_map(|cluster| {
            cluster.capacity_groups.iter().filter_map(move |group| {
                let data_disk = group.profile.as_ref()?.data_disk.as_ref()?;
                Some((
                    cluster.id.as_str(),
                    group.group_id.as_str(),
                    data_disk.size_bytes,
                ))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::PreflightRunner;
    use alien_core::{CapacityGroup, DataDiskRequirement, MachineProfile, ResourceLifecycle};

    fn stack_with_data_disk(size_bytes: Option<u64>) -> Stack {
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: Some(MachineProfile {
                    cpu: "4".to_string(),
                    memory_bytes: 8 * GIB,
                    ephemeral_storage_bytes: 20 * GIB,
                    architecture: None,
                    gpu: None,
                    data_disk: size_bytes.map(|size_bytes| DataDiskRequirement { size_bytes }),
                }),
                min_size: 1,
                max_size: 3,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build();
        Stack::new("test".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .build()
    }

    #[tokio::test]
    async fn machines_accepts_a_sized_data_disk() {
        let stack = stack_with_data_disk(Some(50 * GIB));
        let result = ComputePoolDataDiskCheck
            .check(&stack, Platform::Machines)
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[tokio::test]
    async fn zero_size_is_rejected() {
        let stack = stack_with_data_disk(Some(0));
        let result = ComputePoolDataDiskCheck
            .check(&stack, Platform::Machines)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(
            result.errors,
            vec!["ComputeCluster 'compute' pool 'general': requirements.dataDisk.size must be greater than zero."]
        );
    }

    #[tokio::test]
    async fn clouds_accept_sizes_up_to_the_provider_limit() {
        for (platform, max_gib) in [
            (Platform::Aws, 65_536),
            (Platform::Gcp, 65_536),
            (Platform::Azure, 32_767),
        ] {
            let at_limit = ComputePoolDataDiskCheck
                .check(&stack_with_data_disk(Some(max_gib * GIB)), platform)
                .await
                .unwrap();
            assert!(at_limit.success, "{platform}: {:?}", at_limit.errors);

            // One byte over rounds up to a GiB the provider cannot create.
            let over = ComputePoolDataDiskCheck
                .check(&stack_with_data_disk(Some(max_gib * GIB + 1)), platform)
                .await
                .unwrap();
            assert!(!over.success, "{platform} should reject an over-limit disk");
            assert_eq!(
                over.errors,
                vec![format!(
                    "ComputeCluster 'compute' pool 'general': requirements.dataDisk.size is {} GiB, but the largest data disk on platform '{platform}' is {max_gib} GiB.",
                    max_gib + 1
                )]
            );
        }
    }

    #[tokio::test]
    async fn kubernetes_rejects_a_data_disk() {
        let result = ComputePoolDataDiskCheck
            .check(&stack_with_data_disk(Some(50 * GIB)), Platform::Kubernetes)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(
            result.errors,
            vec!["ComputeCluster 'compute' pool 'general' requires an additional empty disk on each machine, which Alien cannot attach to nodes on platform 'kubernetes'."]
        );
    }

    #[tokio::test]
    async fn local_warns_instead_of_failing() {
        let stack = stack_with_data_disk(Some(50 * GIB));
        let result = ComputePoolDataDiskCheck
            .check(&stack, Platform::Local)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("Platform 'local' does not provide or check one"));
    }

    #[tokio::test]
    async fn registered_build_checks_run_for_a_data_disk() {
        let without_disk = stack_with_data_disk(None);
        assert!(!ComputePoolDataDiskCheck.should_run(&without_disk, Platform::Aws));

        let with_disk = stack_with_data_disk(Some(50 * GIB));
        for (platform, success) in [(Platform::Aws, true), (Platform::Kubernetes, false)] {
            let summary = PreflightRunner::new()
                .run_compile_time_checks(&with_disk, platform)
                .await
                .unwrap();
            let result = summary
                .results
                .iter()
                .find(|result| result.code.as_deref() == Some("COMPUTE_POOL_DATA_DISK_INVALID"))
                .expect("the data disk check should run for a stack that declares one");
            assert_eq!(result.success, success, "{platform}: {:?}", result.errors);
        }
    }
}
