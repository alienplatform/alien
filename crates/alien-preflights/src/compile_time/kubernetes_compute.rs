use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{validate_kubernetes_compute, Platform, Stack};

/// Kubernetes pools constrain placement but do not manage node capacity.
pub struct KubernetesComputeCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for KubernetesComputeCheck {
    fn description(&self) -> &'static str {
        "Kubernetes compute pools must have valid placement and supported execution requirements"
    }
    fn should_run(&self, _stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Kubernetes
    }
    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let errors = validate_kubernetes_compute(stack);
        let warnings = stack.resources().filter_map(|(_, entry)| entry.config.downcast_ref::<alien_core::ComputeCluster>())
            .map(|cluster| format!("ComputeCluster '{}' uses existing Kubernetes nodes. Pool CPU, memory, disk capacity, machine counts, node autoscaling, and failure-domain spread are administrator-managed and are not enforced by Alien. Pool architecture constrains Pod scheduling.", cluster.id)).collect();
        Ok(if errors.is_empty() {
            CheckResult::with_warnings(warnings)
        } else {
            CheckResult::failed_with_warnings(errors, warnings)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{CapacityGroup, ComputeCluster, ResourceLifecycle};

    #[tokio::test]
    async fn capacity_is_disclosed_and_invalid_execution_is_rejected() {
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 3,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build();
        let valid = Stack::new("test".to_string())
            .add(cluster.clone(), ResourceLifecycle::Frozen)
            .build();
        let result = KubernetesComputeCheck
            .check(&valid, Platform::Kubernetes)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("are not enforced"));
        let mut invalid = cluster;
        invalid.capacity_groups[0].nested_virtualization = Some(true);
        let invalid = Stack::new("test".to_string())
            .add(invalid, ResourceLifecycle::Frozen)
            .build();
        let result = KubernetesComputeCheck
            .check(&invalid, Platform::Kubernetes)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.warnings.len(), 1);
        assert!(result
            .errors
            .iter()
            .any(|error| error.contains("nested virtualization")));
        let legacy = KubernetesComputeCheck
            .check(
                &Stack::new("legacy".to_string()).build(),
                Platform::Kubernetes,
            )
            .await
            .unwrap();
        assert!(legacy.success);
        assert!(legacy.warnings.is_empty());
    }
}
