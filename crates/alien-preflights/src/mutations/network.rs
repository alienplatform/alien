//! Network mutation that creates a Network resource for the stack.
//!
//! The Network resource provides cloud-agnostic networking infrastructure (VPC, VNet, subnets, etc.)
//! that can be shared across multiple resource types.
//!
//! The network is created when:
//! - The user explicitly configures `StackSettings.network` (UseDefault, Create, BYO-VPC), OR
//! - The stack has resources that require VPC networking on a cloud platform
//!   (auto-created as an isolated VPC with sensible defaults)

use crate::compile_time::stack_requires_network;
use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    DeploymentConfig, Network, NetworkSettings, Platform, ResourceEntry, ResourceLifecycle, Stack,
    StackState,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Default network settings used when auto-creating a VPC (isolated, production-safe).
fn default_network_settings() -> NetworkSettings {
    NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    }
}

/// Mutation that adds a Network resource to the stack.
///
/// Runs in two modes:
/// 1. **Explicit**: User configured `StackSettings.network` -> use those settings
/// 2. **Auto-create**: No settings, but resources require VPC -> create isolated VPC with defaults
///
/// The result is a frozen `default-network` resource that other resources can depend on.
pub struct NetworkMutation;

#[async_trait]
impl StackMutation for NetworkMutation {
    fn description(&self) -> &'static str {
        "Add Network resource for VPC networking"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        // Only cloud platforms need VPC networking
        if !matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) {
            return false;
        }

        // Don't create a duplicate if network already exists
        if stack
            .resources
            .iter()
            .any(|(id, _)| id == "default-network")
        {
            return false;
        }

        match &config.stack_settings.network {
            Some(network_settings) => {
                // Explicit settings: verify BYO-VPC matches target platform
                matches!(
                    (network_settings, stack_state.platform),
                    (NetworkSettings::UseDefault, _)
                        | (NetworkSettings::Create { .. }, _)
                        | (NetworkSettings::ByoVpcAws { .. }, Platform::Aws)
                        | (NetworkSettings::ByoVpcGcp { .. }, Platform::Gcp)
                        | (NetworkSettings::ByoVnetAzure { .. }, Platform::Azure)
                )
            }
            None => {
                // Auto-create when the stack has resources that require VPC networking
                stack_requires_network(stack)
            }
        }
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        let network_settings = config.stack_settings.network.clone().unwrap_or_else(|| {
            info!(
                platform = ?stack_state.platform,
                "Auto-creating isolated VPC for resources that require networking"
            );
            default_network_settings()
        });

        info!(
            platform = ?stack_state.platform,
            "Adding Network resource to stack"
        );

        let network_id = "default-network";

        let network = Network::new(network_id.to_string())
            .settings(network_settings)
            .build();

        let network_entry = ResourceEntry {
            config: alien_core::Resource::new(network),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        };

        stack
            .resources
            .insert(network_id.to_string(), network_entry);

        debug!("Added Network resource '{}'", network_id);

        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Container, ContainerCode, EnvironmentVariablesSnapshot, ExternalBindings, ResourceEntry,
        ResourceLifecycle, ResourceSpec, StackSettings,
    };
    use indexmap::IndexMap;
    use std::collections::HashMap;

    fn create_test_stack() -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: alien_core::permissions::PermissionsConfig::default(),
        }
    }

    fn create_stack_with_container() -> Stack {
        let mut stack = create_test_stack();
        let container = Container::new("api".to_string())
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
            .build();
        stack.resources.insert(
            "api".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        stack
    }

    fn create_stack_state(platform: Platform) -> StackState {
        StackState {
            platform,
            resource_prefix: "test".to_string(),
            resources: HashMap::new(),
        }
    }

    fn create_deployment_config(network: Option<NetworkSettings>) -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings {
                network,
                ..Default::default()
            })
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    // --- Explicit network settings tests ---

    #[test]
    fn test_should_run_with_create_settings_aws() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Aws);
        let config = create_deployment_config(Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }));

        let mutation = NetworkMutation;
        assert!(mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn test_should_run_with_byo_vpc_aws() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Aws);
        let config = create_deployment_config(Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-123".to_string(),
            public_subnet_ids: vec!["subnet-1".to_string()],
            private_subnet_ids: vec!["subnet-2".to_string()],
            security_group_ids: vec![],
        }));

        let mutation = NetworkMutation;
        assert!(mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn test_should_not_run_on_local_platform() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Local);
        let config = create_deployment_config(Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }));

        let mutation = NetworkMutation;
        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn test_should_not_run_on_kubernetes_platform() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Kubernetes);
        let config = create_deployment_config(Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }));

        let mutation = NetworkMutation;
        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn test_should_not_run_with_mismatched_byo_settings() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Gcp);
        let config = create_deployment_config(Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-123".to_string(),
            public_subnet_ids: vec![],
            private_subnet_ids: vec![],
            security_group_ids: vec![],
        }));

        let mutation = NetworkMutation;
        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn test_should_not_run_if_network_already_exists() {
        let mut stack = create_test_stack();
        let network = Network::new("default-network".to_string())
            .settings(NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            })
            .build();
        stack.resources.insert(
            "default-network".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(network),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );

        let stack_state = create_stack_state(Platform::Aws);
        let config = create_deployment_config(Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }));

        let mutation = NetworkMutation;
        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[tokio::test]
    async fn test_mutate_uses_explicit_settings() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Aws);
        let config = create_deployment_config(Some(NetworkSettings::Create {
            cidr: Some("10.0.0.0/16".to_string()),
            availability_zones: 3,
        }));

        let mutation = NetworkMutation;
        let mutated_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        assert!(mutated_stack.resources.contains_key("default-network"));

        let network_entry = mutated_stack.resources.get("default-network").unwrap();
        assert_eq!(network_entry.lifecycle, ResourceLifecycle::Frozen);

        let network = network_entry.config.downcast_ref::<Network>().unwrap();
        assert_eq!(network.id(), "default-network");

        match &network.settings {
            NetworkSettings::Create {
                cidr,
                availability_zones,
            } => {
                assert_eq!(cidr.as_deref(), Some("10.0.0.0/16"));
                assert_eq!(*availability_zones, 3);
            }
            _ => panic!("Expected Create settings"),
        }
    }

    // --- Auto-create tests ---

    #[test]
    fn test_should_auto_create_when_containers_exist_on_cloud() {
        let stack = create_stack_with_container();
        let stack_state = create_stack_state(Platform::Gcp);
        let config = create_deployment_config(None);

        let mutation = NetworkMutation;
        assert!(mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn test_should_not_auto_create_without_containers() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Aws);
        let config = create_deployment_config(None);

        let mutation = NetworkMutation;
        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn test_should_not_auto_create_on_local_platform() {
        let stack = create_stack_with_container();
        let stack_state = create_stack_state(Platform::Local);
        let config = create_deployment_config(None);

        let mutation = NetworkMutation;
        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[test]
    fn test_should_not_auto_create_on_kubernetes_platform() {
        let stack = create_stack_with_container();
        let stack_state = create_stack_state(Platform::Kubernetes);
        let config = create_deployment_config(None);

        let mutation = NetworkMutation;
        assert!(!mutation.should_run(&stack, &stack_state, &config));
    }

    #[tokio::test]
    async fn test_mutate_auto_creates_with_defaults() {
        let stack = create_stack_with_container();
        let stack_state = create_stack_state(Platform::Gcp);
        let config = create_deployment_config(None);

        let mutation = NetworkMutation;
        let mutated_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        assert!(mutated_stack.resources.contains_key("default-network"));

        let network_entry = mutated_stack.resources.get("default-network").unwrap();
        assert_eq!(network_entry.lifecycle, ResourceLifecycle::Frozen);

        let network = network_entry.config.downcast_ref::<Network>().unwrap();
        match &network.settings {
            NetworkSettings::Create {
                cidr,
                availability_zones,
            } => {
                assert!(
                    cidr.is_none(),
                    "Auto-created network should have no explicit CIDR"
                );
                assert_eq!(*availability_zones, 2);
            }
            _ => panic!("Expected auto-created network to use Create settings"),
        }
    }

    // --- UseDefault tests ---

    #[test]
    fn test_should_run_with_use_default_settings() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Gcp);
        let config = create_deployment_config(Some(NetworkSettings::UseDefault));

        let mutation = NetworkMutation;
        assert!(mutation.should_run(&stack, &stack_state, &config));
    }

    #[tokio::test]
    async fn test_mutate_with_use_default_passes_through() {
        let stack = create_test_stack();
        let stack_state = create_stack_state(Platform::Aws);
        let config = create_deployment_config(Some(NetworkSettings::UseDefault));

        let mutation = NetworkMutation;
        let mutated_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        let network_entry = mutated_stack.resources.get("default-network").unwrap();
        let network = network_entry.config.downcast_ref::<Network>().unwrap();
        assert_eq!(network.settings, NetworkSettings::UseDefault);
    }
}
