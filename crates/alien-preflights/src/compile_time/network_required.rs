//! Network configuration validation checks.
//!
//! Provides:
//! - `stack_requires_network()` — shared helper to detect if a stack needs VPC networking
//!   (used by NetworkMutation to auto-create a VPC and by compile-time checks)
//! - `NetworkSettingsPlatformCheck` — ensures BYO-VPC settings match the target platform
//! - `PublicSubnetsRequiredCheck` — ensures public subnets are configured for public ingress

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{NetworkSettings, Platform, Stack};

/// Resource types that require VPC networking on cloud platforms.
///
/// This is the single source of truth. When adding a new resource type that
/// needs VPC (e.g., databases, caches), add it here.
///
/// Resources that *optionally* use a network (e.g., functions) are NOT listed
/// here — they use the network if it exists but don't require one.
const RESOURCE_TYPES_REQUIRING_NETWORK: &[&str] = &[
    "container",         // Triggers ContainerClusterMutation -> needs VPC
    "container-cluster", // ASGs/MIGs/VMSSs need VPC subnets
];

/// Check if any resource in the stack requires VPC networking.
pub fn stack_requires_network(stack: &Stack) -> bool {
    stack.resources().any(|(_, entry)| {
        RESOURCE_TYPES_REQUIRING_NETWORK.contains(&entry.config.resource_type().as_ref())
    })
}

/// Ensures public subnets are configured when resources need public ingress.
///
/// This check validates that BYO-VPC configurations include public subnet IDs
/// when resources have public ingress requirements.
pub struct PublicSubnetsRequiredCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for PublicSubnetsRequiredCheck {
    fn description(&self) -> &'static str {
        "Public subnets must be configured when resources require public ingress"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        // Only relevant for cloud platforms and when using BYO-VPC
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && stack
                .resources()
                .any(|(_, entry)| entry.config.resource_type().0.as_ref() == "network")
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        // Check if any resource requires public ingress
        let public_ingress_required = self.stack_requires_public_ingress(stack);

        if !public_ingress_required {
            return Ok(CheckResult::success());
        }

        // Find the network resource and check its settings
        for (_, resource_entry) in stack.resources() {
            if resource_entry.config.resource_type().0.as_ref() != "network" {
                continue;
            }

            if let Some(network) = resource_entry.config.downcast_ref::<alien_core::Network>() {
                match (&network.settings, platform) {
                    // UseDefault / Create: public subnets are auto-handled
                    (NetworkSettings::UseDefault, _) | (NetworkSettings::Create { .. }, _) => {}

                    // BYO-VPC AWS: check public_subnet_ids
                    (
                        NetworkSettings::ByoVpcAws {
                            public_subnet_ids, ..
                        },
                        Platform::Aws,
                    ) => {
                        if public_subnet_ids.is_empty() {
                            errors.push(
                                "Stack has resources with public ingress, but BYO-VPC \
                                 configuration has no public subnets. Add public_subnet_ids \
                                 to your network configuration."
                                    .to_string(),
                            );
                        }
                    }

                    // BYO-VPC GCP: GCP uses different networking model
                    (NetworkSettings::ByoVpcGcp { .. }, Platform::Gcp) => {
                        // GCP doesn't have public/private subnets in the same way
                        // Public access is controlled via Cloud NAT and firewall rules
                    }

                    // BYO-VNet Azure: check public_subnet_name
                    (
                        NetworkSettings::ByoVnetAzure {
                            public_subnet_name, ..
                        },
                        Platform::Azure,
                    ) => {
                        if public_subnet_name.is_empty() {
                            errors.push(
                                "Stack has resources with public ingress, but BYO-VNet \
                                 configuration has no public subnet. Add public_subnet_name \
                                 to your network configuration."
                                    .to_string(),
                            );
                        }
                    }

                    // Mismatched platform
                    _ => {
                        // Platform mismatch will be caught by NetworkSettingsPlatformCheck
                    }
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

impl PublicSubnetsRequiredCheck {
    /// Check if any resource requires public ingress
    fn stack_requires_public_ingress(&self, stack: &Stack) -> bool {
        for (_, resource_entry) in stack.resources() {
            let resource_type = resource_entry.config.resource_type();
            let resource_type_str = resource_type.0.as_ref();

            match resource_type_str {
                "function" => {
                    if let Some(function) =
                        resource_entry.config.downcast_ref::<alien_core::Function>()
                    {
                        if matches!(function.ingress, alien_core::Ingress::Public) {
                            return true;
                        }
                    }
                }
                // TODO: Check Container resources when implemented
                _ => {}
            }
        }

        false
    }
}

/// Ensures BYO-VPC settings match the target platform.
///
/// For example, ByoVpcAws settings should only be used on AWS platform.
pub struct NetworkSettingsPlatformCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for NetworkSettingsPlatformCheck {
    fn description(&self) -> &'static str {
        "BYO-VPC settings must match target platform"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        // Only relevant for cloud platforms with BYO settings
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && stack
                .resources()
                .any(|(_, entry)| entry.config.resource_type().0.as_ref() == "network")
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for (resource_id, resource_entry) in stack.resources() {
            if resource_entry.config.resource_type().0.as_ref() != "network" {
                continue;
            }

            if let Some(network) = resource_entry.config.downcast_ref::<alien_core::Network>() {
                let settings_platform = match &network.settings {
                    NetworkSettings::UseDefault | NetworkSettings::Create { .. } => None,
                    NetworkSettings::ByoVpcAws { .. } => Some(Platform::Aws),
                    NetworkSettings::ByoVpcGcp { .. } => Some(Platform::Gcp),
                    NetworkSettings::ByoVnetAzure { .. } => Some(Platform::Azure),
                };

                if let Some(settings_platform) = settings_platform {
                    if settings_platform != platform {
                        errors.push(format!(
                            "Network '{}' has {:?} BYO settings but target platform is {:?}. \
                             Use network settings that match your target platform.",
                            resource_id, settings_platform, platform
                        ));
                    }
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
        permissions::PermissionsConfig, CapacityGroup, Container, ContainerCluster, ContainerCode,
        Function, FunctionCode, Ingress, Network, NetworkSettings, ResourceEntry,
        ResourceLifecycle, ResourceSpec,
    };
    use indexmap::IndexMap;

    fn create_stack(resources: IndexMap<String, ResourceEntry>) -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig::default(),
        }
    }

    fn create_network_entry(settings: NetworkSettings) -> ResourceEntry {
        ResourceEntry {
            config: alien_core::Resource::new(
                Network::new("default-network".to_string())
                    .settings(settings)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn create_function_entry(id: &str, ingress: Ingress) -> ResourceEntry {
        ResourceEntry {
            config: alien_core::Resource::new(
                Function::new(id.to_string())
                    .code(FunctionCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("default".to_string())
                    .ingress(ingress)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn create_container_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: alien_core::Resource::new(
                Container::new(id.to_string())
                    .code(ContainerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .cpu(ResourceSpec {
                        min: "0.5".to_string(),
                        desired: "1".to_string(),
                    })
                    .memory(ResourceSpec {
                        min: "512Mi".to_string(),
                        desired: "1Gi".to_string(),
                    })
                    .port(8080)
                    .permissions("default".to_string())
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    // stack_requires_network tests

    #[test]
    fn test_stack_requires_network_with_container() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        let stack = create_stack(resources);
        assert!(stack_requires_network(&stack));
    }

    #[test]
    fn test_stack_requires_network_with_container_cluster() {
        let mut resources = IndexMap::new();
        let cluster = ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 10,
            })
            .build();
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
                lifecycle: ResourceLifecycle::LiveOnSetup,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        let stack = create_stack(resources);
        assert!(stack_requires_network(&stack));
    }

    #[test]
    fn test_stack_does_not_require_network_for_functions_only() {
        let mut resources = IndexMap::new();
        resources.insert(
            "my-function".to_string(),
            create_function_entry("my-function", Ingress::Private),
        );
        let stack = create_stack(resources);
        assert!(!stack_requires_network(&stack));
    }

    #[test]
    fn test_stack_does_not_require_network_when_empty() {
        let stack = create_stack(IndexMap::new());
        assert!(!stack_requires_network(&stack));
    }

    // PublicSubnetsRequiredCheck tests

    #[tokio::test]
    async fn test_public_subnets_required_with_byo_vpc_missing_public() {
        let mut resources = IndexMap::new();
        resources.insert(
            "default-network".to_string(),
            create_network_entry(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec![], // Empty - no public subnets
                private_subnet_ids: vec!["subnet-1".to_string()],
                security_group_ids: vec![],
            }),
        );
        resources.insert(
            "my-function".to_string(),
            create_function_entry("my-function", Ingress::Public),
        );

        let stack = create_stack(resources);
        let check = PublicSubnetsRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("public subnets"));
    }

    #[tokio::test]
    async fn test_public_subnets_success_with_byo_vpc() {
        let mut resources = IndexMap::new();
        resources.insert(
            "default-network".to_string(),
            create_network_entry(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec!["subnet-pub-1".to_string()],
                private_subnet_ids: vec!["subnet-priv-1".to_string()],
                security_group_ids: vec![],
            }),
        );
        resources.insert(
            "my-function".to_string(),
            create_function_entry("my-function", Ingress::Public),
        );

        let stack = create_stack(resources);
        let check = PublicSubnetsRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_public_subnets_not_required_without_public_ingress() {
        let mut resources = IndexMap::new();
        resources.insert(
            "default-network".to_string(),
            create_network_entry(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec![], // Empty, but that's OK
                private_subnet_ids: vec!["subnet-1".to_string()],
                security_group_ids: vec![],
            }),
        );
        resources.insert(
            "my-function".to_string(),
            create_function_entry("my-function", Ingress::Private),
        );

        let stack = create_stack(resources);
        let check = PublicSubnetsRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    // NetworkSettingsPlatformCheck tests

    #[tokio::test]
    async fn test_platform_mismatch_aws_settings_on_gcp() {
        let mut resources = IndexMap::new();
        resources.insert(
            "default-network".to_string(),
            create_network_entry(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec![],
                private_subnet_ids: vec!["subnet-1".to_string()],
                security_group_ids: vec![],
            }),
        );

        let stack = create_stack(resources);
        let check = NetworkSettingsPlatformCheck;
        let result = check.check(&stack, Platform::Gcp).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("target platform"));
    }

    #[tokio::test]
    async fn test_platform_match_success() {
        let mut resources = IndexMap::new();
        resources.insert(
            "default-network".to_string(),
            create_network_entry(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec![],
                private_subnet_ids: vec!["subnet-1".to_string()],
                security_group_ids: vec![],
            }),
        );

        let stack = create_stack(resources);
        let check = NetworkSettingsPlatformCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_create_settings_work_on_any_platform() {
        let mut resources = IndexMap::new();
        resources.insert(
            "default-network".to_string(),
            create_network_entry(NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            }),
        );

        let stack = create_stack(resources);
        let check = NetworkSettingsPlatformCheck;

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let result = check.check(&stack, platform).await.unwrap();
            assert!(result.success);
        }
    }

    #[tokio::test]
    async fn test_use_default_settings_work_on_any_platform() {
        let mut resources = IndexMap::new();
        resources.insert(
            "default-network".to_string(),
            create_network_entry(NetworkSettings::UseDefault),
        );

        let stack = create_stack(resources);
        let check = NetworkSettingsPlatformCheck;

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let result = check.check(&stack, platform).await.unwrap();
            assert!(result.success);
        }
    }
}
