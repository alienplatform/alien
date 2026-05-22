//! Deployment prerequisite checks.
//!
//! These checks validate that the concrete deployment target/config provides
//! capabilities required by the final stack. They intentionally do not run
//! during `alien build`, because build should validate stack shape without
//! requiring deployment-environment state.

use crate::error::Result;
use crate::{CheckResult, DeploymentPrerequisiteCheck};
use alien_core::{
    ComputeBackend, ComputeCluster, Container, DeploymentConfig, ExposeProtocol, Platform, Stack,
    StackState,
};

fn is_cloud_platform(platform: Platform) -> bool {
    matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
}

fn stack_requires_managed_container_backend(stack: &Stack) -> bool {
    stack.resources().any(|(_, entry)| {
        entry.config.downcast_ref::<Container>().is_some()
            || entry.config.downcast_ref::<ComputeCluster>().is_some()
    })
}

fn resources_requiring_domain_metadata(stack: &Stack) -> Vec<String> {
    let mut resources = Vec::new();

    for (resource_id, entry) in stack.resources() {
        if let Some(container) = entry.config.downcast_ref::<Container>() {
            if container
                .ports
                .iter()
                .any(|port| port.expose == Some(ExposeProtocol::Http))
            {
                resources.push(format!("container '{}' (exposed HTTP port)", resource_id));
            }
        }
    }

    resources
}

/// Validates that cloud container deployments have a managed container backend.
pub struct ManagedContainerBackendRequiredCheck;

#[async_trait::async_trait]
impl DeploymentPrerequisiteCheck for ManagedContainerBackendRequiredCheck {
    fn description(&self) -> &'static str {
        "Cloud container deployments require a managed container backend"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        is_cloud_platform(stack_state.platform) && stack_requires_managed_container_backend(stack)
    }

    async fn check(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        if matches!(config.compute_backend, Some(ComputeBackend::Horizon(_))) {
            return Ok(CheckResult::success());
        }

        let resources = stack
            .resources()
            .filter_map(|(resource_id, entry)| {
                if entry.config.downcast_ref::<Container>().is_some()
                    || entry.config.downcast_ref::<ComputeCluster>().is_some()
                {
                    Some(resource_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        Ok(CheckResult::failed(vec![format!(
            "Cloud container deployments require a managed container backend. \
             Found container resources: {resources}. \
             The deployment config must include computeBackend for cloud platforms."
        )]))
    }
}

/// Validates that cloud public HTTP container deployments have domain metadata.
pub struct DomainMetadataRequiredCheck;

#[async_trait::async_trait]
impl DeploymentPrerequisiteCheck for DomainMetadataRequiredCheck {
    fn description(&self) -> &'static str {
        "Cloud public HTTP container deployments require domain metadata"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        is_cloud_platform(stack_state.platform)
            && !resources_requiring_domain_metadata(stack).is_empty()
    }

    async fn check(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        if config.domain_metadata.is_some() {
            return Ok(CheckResult::success());
        }

        let resource_list = resources_requiring_domain_metadata(stack).join(", ");
        Ok(CheckResult::failed(vec![format!(
            "Cloud public HTTP container deployments require domain metadata. \
             Found containers with exposed HTTP ports: {resource_list}. \
             The deployment config must include domainMetadata for cloud platforms."
        )]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::PreflightRunner;
    use alien_core::permissions::PermissionProfile;
    use alien_core::{
        permissions::PermissionsConfig, CertificateStatus, ContainerCode, DnsRecordStatus,
        DomainMetadata, EnvironmentVariablesSnapshot, Ingress, Resource, ResourceDomainInfo,
        ResourceEntry, ResourceLifecycle, ResourceSpec, Worker, WorkerCode,
    };
    use indexmap::IndexMap;
    use std::collections::HashMap;

    fn stack_state(platform: Platform) -> StackState {
        StackState::new(platform)
    }

    fn deployment_config() -> DeploymentConfig {
        DeploymentConfig {
            stack_settings: Default::default(),
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "2026-05-13T00:00:00Z".to_string(),
            },
            allow_frozen_changes: false,
            compute_backend: None,
            external_bindings: Default::default(),
            base_platform: None,
            public_urls: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
        }
    }

    fn domain_metadata(resource_id: &str) -> DomainMetadata {
        DomainMetadata {
            base_domain: "example.com".to_string(),
            public_subdomain: "test".to_string(),
            hosted_zone_id: "zone".to_string(),
            resources: HashMap::from([(
                resource_id.to_string(),
                ResourceDomainInfo {
                    fqdn: format!("{resource_id}.test.example.com"),
                    certificate_id: "cert".to_string(),
                    certificate_status: CertificateStatus::Issued,
                    dns_status: DnsRecordStatus::Active,
                    dns_error: None,
                    certificate_chain: None,
                    private_key: None,
                    issued_at: None,
                },
            )]),
        }
    }

    fn horizon_backend() -> ComputeBackend {
        ComputeBackend::Horizon(alien_core::HorizonConfig {
            url: "https://containers.example.com".to_string(),
            horizon_host_image: None,
            clusters: HashMap::new(),
        })
    }

    fn create_stack(resources: IndexMap<String, ResourceEntry>) -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig::default().with_profile(
                "default",
                PermissionProfile::new().global(["storage/data-read"]),
            ),
            supported_platforms: None,
        }
    }

    fn create_container_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Container::new(id.to_string())
                    .code(ContainerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .cpu(ResourceSpec {
                        min: "0.25".to_string(),
                        desired: "0.5".to_string(),
                    })
                    .memory(ResourceSpec {
                        min: "256Mi".to_string(),
                        desired: "512Mi".to_string(),
                    })
                    .replicas(1)
                    .permissions("default".to_string())
                    .port(8080)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn create_public_container_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Container::new(id.to_string())
                    .code(ContainerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .cpu(ResourceSpec {
                        min: "0.25".to_string(),
                        desired: "0.5".to_string(),
                    })
                    .memory(ResourceSpec {
                        min: "256Mi".to_string(),
                        desired: "512Mi".to_string(),
                    })
                    .replicas(1)
                    .permissions("default".to_string())
                    .expose_port(8080, ExposeProtocol::Http)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn create_public_function_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Worker::new(id.to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("default".to_string())
                    .ingress(Ingress::Public)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    #[tokio::test]
    async fn managed_container_backend_fails_without_compute_backend_on_cloud() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        let stack = create_stack(resources);
        let config = deployment_config();
        let check = ManagedContainerBackendRequiredCheck;

        assert!(check.should_run(&stack, &stack_state(Platform::Aws), &config));
        let result = check
            .check(&stack, &stack_state(Platform::Aws), &config)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("managed container backend"));
        assert!(result.errors[0].contains("api"));
    }

    #[tokio::test]
    async fn managed_container_backend_passes_with_compute_backend_on_cloud() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        let stack = create_stack(resources);
        let mut config = deployment_config();
        config.compute_backend = Some(horizon_backend());
        let check = ManagedContainerBackendRequiredCheck;

        let result = check
            .check(&stack, &stack_state(Platform::Aws), &config)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn managed_container_backend_skips_local_and_kubernetes() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        let stack = create_stack(resources);
        let config = deployment_config();
        let check = ManagedContainerBackendRequiredCheck;

        assert!(!check.should_run(&stack, &stack_state(Platform::Local), &config));
        assert!(!check.should_run(&stack, &stack_state(Platform::Kubernetes), &config));
    }

    #[tokio::test]
    async fn build_time_preflights_allow_cloud_containers_without_deployment_config() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        let stack = create_stack(resources);
        let runner = PreflightRunner::new();

        let result = runner
            .run_build_time_preflights(&stack, Platform::Aws)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn domain_metadata_fails_for_public_ingress_on_cloud() {
        let mut resources = IndexMap::new();
        resources.insert("web".to_string(), create_public_container_entry("web"));
        let stack = create_stack(resources);
        let config = deployment_config();
        let check = DomainMetadataRequiredCheck;

        assert!(check.should_run(&stack, &stack_state(Platform::Gcp), &config));
        let result = check
            .check(&stack, &stack_state(Platform::Gcp), &config)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("domain metadata"));
        assert!(result.errors[0].contains("web"));
    }

    #[tokio::test]
    async fn domain_metadata_not_required_for_public_functions_on_cloud() {
        let mut resources = IndexMap::new();
        resources.insert("web".to_string(), create_public_function_entry("web"));
        let stack = create_stack(resources);
        let config = deployment_config();
        let check = DomainMetadataRequiredCheck;

        assert!(!check.should_run(&stack, &stack_state(Platform::Aws), &config));
    }

    #[tokio::test]
    async fn domain_metadata_passes_when_present() {
        let mut resources = IndexMap::new();
        resources.insert("web".to_string(), create_public_container_entry("web"));
        let stack = create_stack(resources);
        let mut config = deployment_config();
        config.domain_metadata = Some(domain_metadata("web"));
        let check = DomainMetadataRequiredCheck;

        let result = check
            .check(&stack, &stack_state(Platform::Gcp), &config)
            .await
            .unwrap();

        assert!(result.success);
    }
}
