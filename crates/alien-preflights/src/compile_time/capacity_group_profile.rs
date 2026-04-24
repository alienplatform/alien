//! Validates that ContainerCluster capacity groups have instance_type and profile set
//! for cloud platforms.
//!
//! This is a defense-in-depth check. For auto-generated clusters, the
//! ContainerClusterMutation always populates these fields. This check catches the
//! case where a user manually defines a ContainerCluster without profiles, which
//! would cause a provisioning failure when the infra controller tries to create
//! the Horizon cluster or launch instances.
//!
//! NOTE: This check runs on the mutated stack (after mutations), so it validates
//! both user-defined and auto-generated clusters. It is registered as a runtime
//! check would be, but since it doesn't need cloud access, it runs at compile time
//! on the post-mutation stack. In practice, it is registered as a compile-time check
//! that runs on the mutated stack during deployment preflights.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{ContainerCluster, Platform, Stack};

/// Validates that all ContainerCluster capacity groups have instance_type and profile
/// set for cloud platforms (AWS, GCP, Azure).
pub struct CapacityGroupProfileCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for CapacityGroupProfileCheck {
    fn description(&self) -> &'static str {
        "Capacity groups must have instance_type and profile for cloud platforms"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && stack
                .resources
                .values()
                .any(|entry| entry.config.downcast_ref::<ContainerCluster>().is_some())
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, entry) in &stack.resources {
            let Some(cluster) = entry.config.downcast_ref::<ContainerCluster>() else {
                continue;
            };

            for group in &cluster.capacity_groups {
                if group.instance_type.is_none() {
                    errors.push(format!(
                        "ContainerCluster '{}' capacity group '{}': instance_type is not set. \
                        This is required for cloud platforms. If using an auto-generated cluster, \
                        this should be resolved by ContainerClusterMutation. If defining a cluster \
                        manually, set instance_type explicitly.",
                        resource_id, group.group_id
                    ));
                }

                if group.profile.is_none() {
                    errors.push(format!(
                        "ContainerCluster '{}' capacity group '{}': profile is not set. \
                        This is required for cloud platforms (Horizon needs the machine profile \
                        for capacity planning). If using an auto-generated cluster, this should \
                        be resolved by ContainerClusterMutation.",
                        resource_id, group.group_id
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        CapacityGroup, ContainerCluster, MachineProfile, Resource, ResourceEntry, ResourceLifecycle,
    };
    use indexmap::IndexMap;

    fn make_stack(cluster: ContainerCluster) -> Stack {
        let mut resources = IndexMap::new();
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: Resource::new(cluster),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: alien_core::permissions::PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    #[tokio::test]
    async fn test_passes_with_complete_profile() {
        let cluster = ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.2xlarge".to_string()),
                profile: Some(MachineProfile {
                    cpu: "8.0".to_string(),
                    memory_bytes: 32 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    gpu: None,
                }),
                min_size: 1,
                max_size: 10,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success, "should pass: {:?}", result.errors);
    }

    #[tokio::test]
    async fn test_fails_without_instance_type() {
        let cluster = ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: Some(MachineProfile {
                    cpu: "8.0".to_string(),
                    memory_bytes: 32 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    gpu: None,
                }),
                min_size: 1,
                max_size: 10,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("instance_type is not set"));
    }

    #[tokio::test]
    async fn test_fails_without_profile() {
        let cluster = ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.2xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 10,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("profile is not set"));
    }

    #[tokio::test]
    async fn test_skips_non_cloud_platforms() {
        let cluster = ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 1,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        assert!(!check.should_run(&stack, Platform::Local));
        assert!(!check.should_run(&stack, Platform::Kubernetes));
    }
}
