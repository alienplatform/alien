//! Validates that ComputeCluster capacity groups have instance_type and profile set
//! for cloud platforms.
//!
//! This is a defense-in-depth check. For auto-generated clusters, the
//! ComputeClusterMutation always populates these fields. This check catches the
//! case where a user manually defines a ComputeCluster without profiles, which
//! would cause a provisioning failure when the infra controller tries to create
//! the managed container cluster or launch instances.
//!
//! NOTE: This check runs on the mutated stack (after mutations), so it validates
//! both user-defined and auto-generated clusters. It is registered as a runtime
//! check would be, but since it doesn't need cloud access, it runs at compile time
//! on the post-mutation stack. In practice, it is registered as a compile-time check
//! that runs on the mutated stack during deployment preflights.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{ComputeCluster, Platform, Stack};

/// Validates that all ComputeCluster capacity groups have instance_type and profile
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
                .any(|entry| entry.config.downcast_ref::<ComputeCluster>().is_some())
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, entry) in &stack.resources {
            let Some(cluster) = entry.config.downcast_ref::<ComputeCluster>() else {
                continue;
            };

            for group in &cluster.capacity_groups {
                let Some(instance_type) = group.instance_type.as_deref() else {
                    errors.push(format!(
                        "ComputeCluster '{}' capacity group '{}': instance_type is not set. \
                        This is required for cloud platforms. If using an auto-generated cluster, \
                        this should be resolved by ComputeClusterMutation. If defining a cluster \
                        manually, set instance_type explicitly.",
                        resource_id, group.group_id
                    ));
                    continue;
                };

                // Profile may be None on customer-declared groups; the
                // backfill step in ComputeClusterMutation resolves it from
                // the instance catalog at deployment time. Here we only
                // require that the resolution will succeed — i.e. the
                // instance_type is in the catalog. If it isn't, neither
                // backfill nor downstream provisioning will work, so we
                // surface that early.
                if group.profile.is_none()
                    && alien_core::instance_catalog::find_instance_type(platform, instance_type)
                        .is_none()
                {
                    errors.push(format!(
                        "ComputeCluster '{}' capacity group '{}': profile is not set and \
                        instance_type '{}' is not in the {:?} instance catalog \
                        (cannot derive profile). Set `profile` explicitly or pick a \
                        catalog-known instance_type.",
                        resource_id, group.group_id, instance_type, platform
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
        CapacityGroup, ComputeCluster, MachineProfile, Resource, ResourceEntry, ResourceLifecycle,
    };
    use indexmap::IndexMap;

    fn make_stack(cluster: ComputeCluster) -> Stack {
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
            inputs: vec![],
        }
    }

    #[tokio::test]
    async fn test_passes_with_complete_profile() {
        let cluster = ComputeCluster::new("compute".to_string())
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
                nested_virtualization: None,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success, "should pass: {:?}", result.errors);
    }

    #[tokio::test]
    async fn test_fails_without_instance_type() {
        let cluster = ComputeCluster::new("compute".to_string())
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
                nested_virtualization: None,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("instance_type is not set"));
    }

    #[tokio::test]
    async fn test_passes_without_profile_when_instance_type_is_catalog_known() {
        // No `profile` set, but `instance_type` is in the catalog → the
        // mutation phase will derive `profile` from the catalog entry, so
        // the check passes.
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.2xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 10,
                nested_virtualization: None,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success, "should pass: {:?}", result.errors);
    }

    #[tokio::test]
    async fn test_fails_without_profile_when_instance_type_unknown() {
        // No `profile` and instance_type is NOT in the catalog → backfill
        // can't help, so surface the error.
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("fictional.42xlarge".to_string()),
                profile: None,
                min_size: 1,
                max_size: 10,
                nested_virtualization: None,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("not in the"));
        assert!(result.errors[0].contains("instance catalog"));
    }

    #[tokio::test]
    async fn test_skips_non_cloud_platforms() {
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 1,
                nested_virtualization: None,
            })
            .build();

        let stack = make_stack(cluster);
        let check = CapacityGroupProfileCheck;
        assert!(!check.should_run(&stack, Platform::Local));
        assert!(!check.should_run(&stack, Platform::Kubernetes));
    }
}
