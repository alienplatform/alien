//! KubernetesCluster mutation that auto-generates the Kubernetes runtime substrate.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesHeartbeatMode, Platform, ResourceEntry, ResourceLifecycle, ResourceRef, Stack,
    StackState,
};
use async_trait::async_trait;
use tracing::info;

const DEFAULT_CLUSTER_ID: &str = "kubernetes";

pub struct KubernetesClusterMutation;

#[async_trait]
impl StackMutation for KubernetesClusterMutation {
    fn description(&self) -> &'static str {
        "Auto-generate KubernetesCluster substrate resources for Kubernetes workloads"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &alien_core::DeploymentConfig,
    ) -> bool {
        if stack_state.platform != Platform::Kubernetes {
            return false;
        }

        has_kubernetes_workload(stack) && !has_kubernetes_cluster(stack)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &alien_core::DeploymentConfig,
    ) -> Result<Stack> {
        if stack_state.platform != Platform::Kubernetes {
            return Ok(stack);
        }

        if !has_kubernetes_cluster(&stack) && has_kubernetes_workload(&stack) {
            let provider = provider_from_base_platform(config.base_platform);
            let settings = config
                .stack_settings
                .kubernetes
                .as_ref()
                .and_then(|settings| settings.cluster.as_ref());
            let cluster = KubernetesCluster::new(DEFAULT_CLUSTER_ID.to_string())
                .provider(provider)
                .ownership(cluster_ownership(provider, settings))
                .namespace(kubernetes_namespace(config, settings))
                .maybe_cloud(settings.and_then(|settings| settings.cloud.clone()))
                .heartbeat_mode(heartbeat_mode(provider))
                .build();

            let dependencies = if stack.resources.contains_key("default-network") {
                vec![ResourceRef::new(
                    alien_core::Network::RESOURCE_TYPE,
                    "default-network",
                )]
            } else {
                Vec::new()
            };

            stack.resources.insert(
                DEFAULT_CLUSTER_ID.to_string(),
                ResourceEntry {
                    config: alien_core::Resource::new(cluster),
                    lifecycle: ResourceLifecycle::Frozen,
                    dependencies,
                    remote_access: false,
                },
            );

            info!(
                cluster_id = DEFAULT_CLUSTER_ID,
                provider = ?provider,
                "Generated KubernetesCluster substrate"
            );
        }

        add_cluster_dependencies(&mut stack);

        Ok(stack)
    }
}

fn has_kubernetes_workload(stack: &Stack) -> bool {
    stack.resources.values().any(|entry| {
        matches!(
            entry.config.resource_type().as_ref(),
            "container" | "worker" | "daemon"
        )
    })
}

fn has_kubernetes_cluster(stack: &Stack) -> bool {
    stack
        .resources
        .values()
        .any(|entry| entry.config.resource_type().as_ref() == "kubernetes-cluster")
}

fn provider_from_base_platform(base_platform: Option<Platform>) -> KubernetesClusterProvider {
    match base_platform {
        Some(Platform::Aws) => KubernetesClusterProvider::Eks,
        Some(Platform::Gcp) => KubernetesClusterProvider::Gke,
        Some(Platform::Azure) => KubernetesClusterProvider::Aks,
        _ => KubernetesClusterProvider::Generic,
    }
}

fn heartbeat_mode(provider: KubernetesClusterProvider) -> KubernetesHeartbeatMode {
    match provider {
        KubernetesClusterProvider::Eks
        | KubernetesClusterProvider::Gke
        | KubernetesClusterProvider::Aks => KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata,
        KubernetesClusterProvider::Generic => KubernetesHeartbeatMode::KubernetesApi,
    }
}

fn cluster_ownership(
    provider: KubernetesClusterProvider,
    settings: Option<&alien_core::KubernetesClusterSettings>,
) -> KubernetesClusterOwnership {
    if let Some(settings) = settings {
        return settings.ownership;
    }

    match provider {
        KubernetesClusterProvider::Eks
        | KubernetesClusterProvider::Gke
        | KubernetesClusterProvider::Aks => KubernetesClusterOwnership::Managed,
        KubernetesClusterProvider::Generic => KubernetesClusterOwnership::External,
    }
}

fn kubernetes_namespace(
    config: &alien_core::DeploymentConfig,
    settings: Option<&alien_core::KubernetesClusterSettings>,
) -> String {
    if let Some(namespace) = settings
        .and_then(|settings| settings.namespace.as_ref())
        .filter(|namespace| !namespace.is_empty())
    {
        return namespace.clone();
    }

    let _ = config;
    "default".to_string()
}

fn add_cluster_dependencies(stack: &mut Stack) {
    let dependency = ResourceRef::new(
        KubernetesCluster::RESOURCE_TYPE,
        DEFAULT_CLUSTER_ID.to_string(),
    );

    for entry in stack.resources.values_mut() {
        if !matches!(
            entry.config.resource_type().as_ref(),
            "container" | "worker" | "daemon"
        ) {
            continue;
        }

        if !entry.dependencies.iter().any(|existing| {
            existing.resource_type == dependency.resource_type && existing.id == dependency.id
        }) {
            entry.dependencies.push(dependency.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Container, ContainerCode, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
        KubernetesClusterSettings, KubernetesSettings, ResourceSpec, StackSettings,
    };

    fn config_with_settings(
        base_platform: Option<Platform>,
        stack_settings: StackSettings,
    ) -> DeploymentConfig {
        DeploymentConfig {
            deployment_name: None,
            stack_settings,
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
            allow_frozen_changes: false,
            compute_backend: None,
            external_bindings: ExternalBindings::default(),
            base_platform,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
        }
    }

    fn config(base_platform: Option<Platform>) -> DeploymentConfig {
        config_with_settings(base_platform, StackSettings::default())
    }

    fn stack_with_container() -> Stack {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.25".to_string(),
            })
            .memory(ResourceSpec {
                min: "128Mi".to_string(),
                desired: "128Mi".to_string(),
            })
            .permissions("app".to_string())
            .build();

        Stack::new("test".to_string())
            .add(container, ResourceLifecycle::Live)
            .build()
    }

    #[tokio::test]
    async fn inserts_kubernetes_cluster_for_kubernetes_workloads() {
        let mutation = KubernetesClusterMutation;
        let state = StackState::new(Platform::Kubernetes);
        let stack = mutation
            .mutate(stack_with_container(), &state, &config(Some(Platform::Aws)))
            .await
            .unwrap();

        let cluster = stack.resources["kubernetes"]
            .config
            .downcast_ref::<KubernetesCluster>()
            .unwrap();

        assert_eq!(cluster.provider, KubernetesClusterProvider::Eks);
        assert_eq!(cluster.ownership, KubernetesClusterOwnership::Managed);
        assert!(stack.resources["api"]
            .dependencies
            .iter()
            .any(|dependency| {
                dependency.resource_type == KubernetesCluster::RESOURCE_TYPE
                    && dependency.id == "kubernetes"
            }));
    }

    #[tokio::test]
    async fn does_not_insert_kubernetes_cluster_for_cloud_platforms() {
        let mutation = KubernetesClusterMutation;
        let state = StackState::new(Platform::Aws);
        let stack = mutation
            .mutate(stack_with_container(), &state, &config(None))
            .await
            .unwrap();

        assert!(!stack.resources.contains_key("kubernetes"));
    }

    #[tokio::test]
    async fn explicit_existing_cluster_settings_override_default_managed_cluster() {
        let mutation = KubernetesClusterMutation;
        let state = StackState::new(Platform::Kubernetes);
        let config = config_with_settings(
            Some(Platform::Aws),
            StackSettings {
                kubernetes: Some(KubernetesSettings {
                    cluster: Some(KubernetesClusterSettings {
                        ownership: KubernetesClusterOwnership::Existing,
                        namespace: Some("alien-runtime".to_string()),
                        cloud: None,
                    }),
                    exposure: None,
                }),
                ..StackSettings::default()
            },
        );
        let stack = mutation
            .mutate(stack_with_container(), &state, &config)
            .await
            .unwrap();

        let cluster = stack.resources["kubernetes"]
            .config
            .downcast_ref::<KubernetesCluster>()
            .unwrap();

        assert_eq!(cluster.provider, KubernetesClusterProvider::Eks);
        assert_eq!(cluster.ownership, KubernetesClusterOwnership::Existing);
        assert_eq!(cluster.namespace, "alien-runtime");
    }
}
