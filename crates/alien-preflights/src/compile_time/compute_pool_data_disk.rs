use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{ComputeCluster, Platform, Stack};

/// Validates pools that require an additional empty disk on each machine.
///
/// Only Machines checks the disk, when a host joins. Cloud platforms and
/// Kubernetes cannot provide it, so declaring one there is an error instead of
/// a requirement that silently goes unmet.
pub struct ComputePoolDataDiskCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ComputePoolDataDiskCheck {
    fn code(&self) -> Option<&'static str> {
        Some("COMPUTE_POOL_DATA_DISK_INVALID")
    }

    fn description(&self) -> &'static str {
        "Compute pool data disks must have a size and are supported only on machines"
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
                Platform::Machines => {}
                Platform::Local | Platform::Test => warnings.push(format!(
                    "{pool} requires an additional empty disk on each machine. It is checked when a host joins a machines deployment, not on platform '{platform}'."
                )),
                Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Kubernetes => {
                    errors.push(format!(
                        "{pool} requires an additional empty disk on each machine, which is supported only on platform 'machines'. Alien does not attach one on platform '{platform}'."
                    ))
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

    const GIB: u64 = 1024 * 1024 * 1024;

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
    async fn cloud_and_kubernetes_reject_a_data_disk() {
        let stack = stack_with_data_disk(Some(50 * GIB));
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
        ] {
            let result = ComputePoolDataDiskCheck
                .check(&stack, platform)
                .await
                .unwrap();
            assert!(!result.success, "{platform} should reject a data disk");
            assert_eq!(
                result.errors,
                vec![format!(
                    "ComputeCluster 'compute' pool 'general' requires an additional empty disk on each machine, which is supported only on platform 'machines'. Alien does not attach one on platform '{platform}'."
                )]
            );
        }
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
        assert!(result.warnings[0].contains("not on platform 'local'"));
    }

    #[tokio::test]
    async fn registered_build_checks_reject_a_data_disk_on_aws() {
        let without_disk = stack_with_data_disk(None);
        assert!(!ComputePoolDataDiskCheck.should_run(&without_disk, Platform::Aws));

        let with_disk = stack_with_data_disk(Some(50 * GIB));
        let summary = PreflightRunner::new()
            .run_compile_time_checks(&with_disk, Platform::Aws)
            .await
            .unwrap();
        let result = summary
            .results
            .iter()
            .find(|result| result.code.as_deref() == Some("COMPUTE_POOL_DATA_DISK_INVALID"))
            .expect("the data disk check should run for a stack that declares one");
        assert!(!result.success);
    }
}
