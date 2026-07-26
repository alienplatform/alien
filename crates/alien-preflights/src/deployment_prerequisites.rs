//! Deployment prerequisite checks.
//!
//! These checks validate that the concrete deployment target/config provides
//! capabilities required by the final stack. They intentionally do not run
//! during `alien build`, because build should validate stack shape without
//! requiring deployment-environment state.

use crate::error::Result;
use crate::{CheckResult, DeploymentPrerequisiteCheck};
use alien_core::{
    validate_binding_type, ComputeBackend, ComputeCluster, Container, Daemon, DeploymentConfig,
    EnvironmentVariable, ExposeProtocol, KubernetesCluster, PermissionSet, Platform, ResourceEntry,
    ResourceLifecycle, Stack, StackState, Worker,
};
use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, BindingTarget, PermissionContext,
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
                .public_endpoints
                .iter()
                .any(|endpoint| endpoint.protocol == ExposeProtocol::Http)
            {
                resources.push(format!("container '{}' (public endpoint)", resource_id));
            }
        }
    }

    resources
}

fn external_binding_required_types(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Kubernetes => &["storage", "queue", "kv", "artifact-registry"],
        Platform::Machines => &["storage", "queue", "kv", "vault", "postgres"],
        _ => &[],
    }
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

/// Validates that AWS Live resource-scoped management permissions are emitted by setup.
pub struct AwsLiveManagementPermissionsSetupCheck;

#[async_trait::async_trait]
impl DeploymentPrerequisiteCheck for AwsLiveManagementPermissionsSetupCheck {
    fn description(&self) -> &'static str {
        "AWS Live management permissions should be setup-owned"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        stack_state.platform == Platform::Aws && stack.management().profile().is_some()
    }

    async fn check(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        let mut errors = Vec::new();
        let Some(profile) = stack.management().profile() else {
            return Ok(CheckResult::success());
        };

        for (resource_id, permission_set_refs) in
            profile.0.iter().filter(|(scope, _)| *scope != "*")
        {
            let Some(resource_entry) = stack.resources.get(resource_id) else {
                continue;
            };
            if resource_entry.lifecycle != ResourceLifecycle::Live {
                continue;
            }

            for permission_set_ref in permission_set_refs {
                let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                else {
                    continue;
                };
                if permission_set.platforms.aws.is_none()
                    || permission_set.id.ends_with("/provision")
                {
                    continue;
                }
                if aws_live_management_permission_is_setup_compilable(
                    resource_id,
                    resource_entry,
                    &permission_set,
                ) {
                    continue;
                }

                errors.push(format!(
                    "AWS management permission '{}' is scoped to Live resource '{}', but setup cannot compile a concrete grant for it before provisioning. The permission requires provider resource context that AWS setup does not know yet.",
                    permission_set.id, resource_id
                ));
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

fn aws_live_management_permission_is_setup_compilable(
    resource_id: &str,
    resource_entry: &ResourceEntry,
    permission_set: &PermissionSet,
) -> bool {
    // This validates the provider-neutral AWS permission template with
    // concrete setup-shaped values. Backend emitters still own Terraform or
    // CloudFormation rendering details, so AWS permission JSONC should not use
    // backend-only syntax such as CloudFormation ${!Sub} escapes.
    let context = aws_live_management_setup_context(resource_id, resource_entry);
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(permission_set, BindingTarget::Resource, &context)
        .is_ok()
}

fn aws_live_management_setup_context(
    resource_id: &str,
    resource_entry: &ResourceEntry,
) -> PermissionContext {
    let context = PermissionContext::new()
        .with_stack_prefix("stack")
        .with_aws_region("us-east-1")
        .with_aws_account_id("123456789012")
        .with_managing_account_id("210987654321")
        .with_resource_id(resource_id.to_string());

    if resource_entry.config.downcast_ref::<Worker>().is_some() {
        return context.with_resource_name(format!("stack-{resource_id}"));
    }

    if resource_entry
        .config
        .downcast_ref::<KubernetesCluster>()
        .is_some()
    {
        return context.with_resource_name(resource_id.to_string());
    }

    context
}

/// Validates that targeted environment variables resolve to resources in the final stack.
pub struct TargetResourcesResolveCheck;

#[async_trait::async_trait]
impl DeploymentPrerequisiteCheck for TargetResourcesResolveCheck {
    fn description(&self) -> &'static str {
        "Environment variable target resources should resolve"
    }

    fn should_run(
        &self,
        _stack: &Stack,
        _stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        config
            .environment_variables
            .variables
            .iter()
            .any(|variable| variable.target_resources.is_some())
    }

    async fn check(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        let resource_ids = environment_variable_target_resource_ids(stack);
        let mut errors = Vec::new();

        for variable in &config.environment_variables.variables {
            validate_target_resources(variable, &resource_ids, &mut errors);
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

/// Iterate the stack resources whose types require external bindings on this platform.
fn external_binding_required_resources<'a>(
    stack: &'a Stack,
    stack_state: &StackState,
) -> impl Iterator<Item = (&'a String, &'a ResourceEntry, String)> {
    let required_types = external_binding_required_types(stack_state.platform);
    stack.resources().filter_map(move |(resource_id, entry)| {
        let resource_type = entry.config.resource_type().0.as_ref().to_string();
        if required_types.contains(&resource_type.as_str()) {
            Some((resource_id, entry, resource_type))
        } else {
            None
        }
    })
}

/// Validates that machines infrastructure resources use the Frozen lifecycle.
pub struct MachinesInfrastructureFrozenCheck;

#[async_trait::async_trait]
impl DeploymentPrerequisiteCheck for MachinesInfrastructureFrozenCheck {
    fn code(&self) -> Option<&'static str> {
        Some("MACHINES_INFRASTRUCTURE_MUST_BE_FROZEN")
    }

    fn description(&self) -> &'static str {
        "Machines infrastructure resources must use the Frozen lifecycle"
    }

    fn should_run(
        &self,
        _stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        stack_state.platform == Platform::Machines
    }

    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        let errors = external_binding_required_resources(stack, stack_state)
            .filter(|(_, entry, _)| entry.lifecycle != ResourceLifecycle::Frozen)
            .map(|(resource_id, _, resource_type)| {
                format!(
                    "Resource '{resource_id}' of type '{resource_type}' must use Frozen lifecycle on platform 'machines'. Machines deployments require setup-owned infrastructure with external bindings."
                )
            })
            .collect::<Vec<_>>();

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

/// Validates that external bindings exist for platforms that do not provision infrastructure.
pub struct ExternalBindingsRequiredCheck;

#[async_trait::async_trait]
impl DeploymentPrerequisiteCheck for ExternalBindingsRequiredCheck {
    fn code(&self) -> Option<&'static str> {
        Some("EXTERNAL_BINDING_REQUIRED")
    }

    fn description(&self) -> &'static str {
        "External infrastructure resources require matching bindings"
    }

    fn should_run(
        &self,
        _stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        !external_binding_required_types(stack_state.platform).is_empty()
    }

    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        let errors = external_binding_required_resources(stack, stack_state)
            .filter(|(resource_id, _, _)| !config.external_bindings.has(resource_id))
            .map(|(resource_id, _, resource_type)| {
                format!(
                    "Platform '{}' requires an external binding for infrastructure resource '{resource_id}' of type '{resource_type}'.",
                    stack_state.platform.as_str()
                )
            })
            .collect::<Vec<_>>();

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

/// Validates that provided external bindings match their resource types.
pub struct ExternalBindingsTypeCheck;

#[async_trait::async_trait]
impl DeploymentPrerequisiteCheck for ExternalBindingsTypeCheck {
    fn code(&self) -> Option<&'static str> {
        Some("EXTERNAL_BINDING_TYPE_MISMATCH")
    }

    fn description(&self) -> &'static str {
        "External infrastructure bindings must match their resource types"
    }

    fn should_run(
        &self,
        _stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        !external_binding_required_types(stack_state.platform).is_empty()
    }

    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        let errors = external_binding_required_resources(stack, stack_state)
            .filter_map(|(resource_id, entry, resource_type)| {
                let binding = config.external_bindings.get(resource_id.as_str())?;
                validate_binding_type(&entry.config, binding).err().map(|_| {
                    format!(
                        "External binding for resource '{resource_id}' must match resource type '{resource_type}'."
                    )
                })
            })
            .collect::<Vec<_>>();

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

fn environment_variable_target_resource_ids(stack: &Stack) -> Vec<&str> {
    stack
        .resources()
        .filter_map(|(id, entry)| {
            if entry.config.downcast_ref::<Worker>().is_some()
                || entry.config.downcast_ref::<Container>().is_some()
                || entry.config.downcast_ref::<Daemon>().is_some()
            {
                Some(id.as_str())
            } else {
                None
            }
        })
        .collect()
}

fn validate_target_resources(
    variable: &EnvironmentVariable,
    resource_ids: &[&str],
    errors: &mut Vec<String>,
) {
    let Some(patterns) = variable.target_resources.as_ref() else {
        return;
    };

    if patterns.is_empty() {
        errors.push(format!(
            "Environment variable '{}' has an empty targetResources list; omit targetResources to target every resource.",
            variable.name
        ));
        return;
    }

    for pattern in patterns {
        match target_resource_pattern_matches(pattern, resource_ids) {
            Ok(true) => {}
            Ok(false) => errors.push(format!(
                "Environment variable '{}' targetResources pattern '{}' did not match any resource in the final stack.",
                variable.name, pattern
            )),
            Err(reason) => errors.push(format!(
                "Environment variable '{}' targetResources pattern '{}' is invalid: {reason}.",
                variable.name, pattern
            )),
        }
    }
}

fn target_resource_pattern_matches(
    pattern: &str,
    resource_ids: &[&str],
) -> std::result::Result<bool, &'static str> {
    if pattern.is_empty() {
        return Err("patterns cannot be empty");
    }

    let wildcard_count = pattern.matches('*').count();
    if wildcard_count == 0 {
        return Ok(resource_ids
            .iter()
            .any(|resource_id| *resource_id == pattern));
    }

    if wildcard_count != 1 || !pattern.ends_with('*') {
        return Err("only trailing '*' wildcard patterns are supported");
    }

    let prefix = pattern.strip_suffix('*').unwrap_or(pattern);
    Ok(resource_ids
        .iter()
        .any(|resource_id| resource_id.starts_with(prefix)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::PreflightRunner;
    use alien_core::permissions::{ManagementPermissions, PermissionProfile};
    use alien_core::{
        bindings::{KvBinding, StorageBinding},
        permissions::PermissionsConfig,
        CertificateStatus, ContainerCode, DnsRecordStatus, DomainMetadata, EnvironmentVariable,
        EnvironmentVariableType, EnvironmentVariablesSnapshot, ExternalBinding, HealthCheck,
        PublicEndpoint, Resource, ResourceDomainInfo, ResourceEntry, ResourceLifecycle,
        ResourceSpec, Storage, Worker, WorkerCode, WorkerPublicEndpoint,
    };
    use indexmap::IndexMap;
    use std::collections::HashMap;

    fn stack_state(platform: Platform) -> StackState {
        StackState::new(platform)
    }

    fn deployment_config() -> DeploymentConfig {
        DeploymentConfig {
            deployment_name: Some("test deployment".to_string()),
            stack_settings: Default::default(),
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "2026-05-13T00:00:00Z".to_string(),
            },
            input_values: Default::default(),
            allow_frozen_changes: false,
            compute_backend: None,
            external_bindings: Default::default(),
            base_platform: None,
            label_domain: None,
            observe_label_selector: None,
            observe_all_namespaces: false,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
        }
    }

    fn targeted_env(name: &str, target_resources: Option<Vec<&str>>) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_string(),
            value: "value".to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: target_resources.map(|patterns| {
                patterns
                    .into_iter()
                    .map(std::string::ToString::to_string)
                    .collect()
            }),
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
                    aliases: Vec::new(),
                    endpoints: HashMap::new(),
                },
            )]),
        }
    }

    fn horizon_backend() -> ComputeBackend {
        ComputeBackend::Horizon(alien_core::HorizonConfig {
            url: "https://containers.example.com".to_string(),
            horizon_machine_image: None,
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
            inputs: vec![],
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
            enabled_when: None,
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
                    .public_endpoint(PublicEndpoint {
                        name: "api".to_string(),
                        port: 8080,
                        protocol: ExposeProtocol::Http,
                        host_label: None,
                        wildcard_subdomains: false,
                    })
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    fn create_public_tcp_container_with_http_health_entry(id: &str) -> ResourceEntry {
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
                    .public_endpoint(PublicEndpoint {
                        name: "transactions".to_string(),
                        port: 7000,
                        protocol: ExposeProtocol::Tcp,
                        host_label: None,
                        wildcard_subdomains: false,
                    })
                    .health_check(HealthCheck {
                        path: "/health".to_string(),
                        port: Some(8080),
                        method: "GET".to_string(),
                        timeout_seconds: 1,
                        failure_threshold: 3,
                    })
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
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
                    .public_endpoint(WorkerPublicEndpoint {
                        name: "api".to_string(),
                        host_label: None,
                        wildcard_subdomains: false,
                    })
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    fn create_storage_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(Storage::new(id.to_string()).build()),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    fn create_live_storage_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(Storage::new(id.to_string()).build()),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
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
    async fn domain_metadata_not_required_for_public_tcp_with_private_http_health_check() {
        let mut resources = IndexMap::new();
        resources.insert(
            "transactions".to_string(),
            create_public_tcp_container_with_http_health_entry("transactions"),
        );
        let stack = create_stack(resources);
        let config = deployment_config();
        let check = DomainMetadataRequiredCheck;

        assert!(!check.should_run(&stack, &stack_state(Platform::Aws), &config));
    }

    #[tokio::test]
    async fn aws_live_management_permissions_allow_worker_dispatch_command() {
        let mut resources = IndexMap::new();
        resources.insert("job".to_string(), create_public_function_entry("job"));
        let mut stack = create_stack(resources);
        stack.permissions.management = ManagementPermissions::override_(
            PermissionProfile::new().resource("job", ["worker/dispatch-command"]),
        );
        let config = deployment_config();
        let check = AwsLiveManagementPermissionsSetupCheck;

        assert!(check.should_run(&stack, &stack_state(Platform::Aws), &config));
        let result = check
            .check(&stack, &stack_state(Platform::Aws), &config)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn aws_live_management_permissions_fail_for_unsupported_live_resource_scope() {
        let mut resources = IndexMap::new();
        resources.insert("uploads".to_string(), create_live_storage_entry("uploads"));
        let mut stack = create_stack(resources);
        stack.permissions.management = ManagementPermissions::override_(
            PermissionProfile::new().resource("uploads", ["storage/data-read"]),
        );
        let config = deployment_config();
        let check = AwsLiveManagementPermissionsSetupCheck;

        let result = check
            .check(&stack, &stack_state(Platform::Aws), &config)
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("storage/data-read"));
        assert!(result.errors[0].contains("uploads"));
        assert!(result.errors[0].contains("setup cannot compile"));
    }

    #[tokio::test]
    async fn aws_live_management_permissions_ignore_frozen_resource_scope() {
        let mut resources = IndexMap::new();
        resources.insert("uploads".to_string(), create_storage_entry("uploads"));
        let mut stack = create_stack(resources);
        stack.permissions.management = ManagementPermissions::override_(
            PermissionProfile::new().resource("uploads", ["storage/data-read"]),
        );
        let config = deployment_config();
        let check = AwsLiveManagementPermissionsSetupCheck;

        let result = check
            .check(&stack, &stack_state(Platform::Aws), &config)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn aws_live_management_permissions_do_not_run_for_gcp_or_azure() {
        let mut resources = IndexMap::new();
        resources.insert("uploads".to_string(), create_live_storage_entry("uploads"));
        let mut stack = create_stack(resources);
        stack.permissions.management = ManagementPermissions::override_(
            PermissionProfile::new().resource("uploads", ["storage/data-read"]),
        );
        let config = deployment_config();
        let check = AwsLiveManagementPermissionsSetupCheck;

        assert!(!check.should_run(&stack, &stack_state(Platform::Gcp), &config));
        assert!(!check.should_run(&stack, &stack_state(Platform::Azure), &config));
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

    #[tokio::test]
    async fn machines_infrastructure_requires_external_binding() {
        let mut resources = IndexMap::new();
        resources.insert("uploads".to_string(), create_storage_entry("uploads"));
        let stack = create_stack(resources);
        let config = deployment_config();
        let check = ExternalBindingsRequiredCheck;

        assert_eq!(check.code(), Some("EXTERNAL_BINDING_REQUIRED"));
        assert!(check.should_run(&stack, &stack_state(Platform::Machines), &config));
        let result = check
            .check(&stack, &stack_state(Platform::Machines), &config)
            .await
            .unwrap();

        assert_eq!(
            result.errors,
            vec![
                "Platform 'machines' requires an external binding for infrastructure resource 'uploads' of type 'storage'."
                    .to_string()
            ]
        );
    }

    #[tokio::test]
    async fn machines_infrastructure_must_be_frozen() {
        let mut resources = IndexMap::new();
        resources.insert("uploads".to_string(), create_live_storage_entry("uploads"));
        let stack = create_stack(resources);
        let mut config = deployment_config();
        config.external_bindings.insert(
            "uploads",
            ExternalBinding::Storage(StorageBinding::s3("bucket")),
        );
        let check = MachinesInfrastructureFrozenCheck;

        assert_eq!(check.code(), Some("MACHINES_INFRASTRUCTURE_MUST_BE_FROZEN"));
        assert!(check.should_run(&stack, &stack_state(Platform::Machines), &config));
        assert!(!check.should_run(&stack, &stack_state(Platform::Kubernetes), &config));
        let result = check
            .check(&stack, &stack_state(Platform::Machines), &config)
            .await
            .unwrap();

        assert_eq!(
            result.errors,
            vec![
                "Resource 'uploads' of type 'storage' must use Frozen lifecycle on platform 'machines'. Machines deployments require setup-owned infrastructure with external bindings."
                    .to_string()
            ]
        );
    }

    #[tokio::test]
    async fn machines_infrastructure_rejects_binding_type_mismatch() {
        let mut resources = IndexMap::new();
        resources.insert("uploads".to_string(), create_storage_entry("uploads"));
        let stack = create_stack(resources);
        let mut config = deployment_config();
        config.external_bindings.insert(
            "uploads",
            ExternalBinding::Kv(KvBinding::redis("redis://cache")),
        );
        let check = ExternalBindingsTypeCheck;

        assert_eq!(check.code(), Some("EXTERNAL_BINDING_TYPE_MISMATCH"));
        let result = check
            .check(&stack, &stack_state(Platform::Machines), &config)
            .await
            .unwrap();

        assert_eq!(
            result.errors,
            vec![
                "External binding for resource 'uploads' must match resource type 'storage'."
                    .to_string()
            ]
        );

        // A missing binding is the required-check's finding, not a type mismatch.
        let empty_config = deployment_config();
        let result = ExternalBindingsTypeCheck
            .check(&stack, &stack_state(Platform::Machines), &empty_config)
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn machines_infrastructure_passes_with_frozen_matching_binding() {
        let mut resources = IndexMap::new();
        resources.insert("uploads".to_string(), create_storage_entry("uploads"));
        let stack = create_stack(resources);
        let mut config = deployment_config();
        config.external_bindings.insert(
            "uploads",
            ExternalBinding::Storage(StorageBinding::s3("bucket")),
        );

        for result in [
            MachinesInfrastructureFrozenCheck
                .check(&stack, &stack_state(Platform::Machines), &config)
                .await
                .unwrap(),
            ExternalBindingsRequiredCheck
                .check(&stack, &stack_state(Platform::Machines), &config)
                .await
                .unwrap(),
            ExternalBindingsTypeCheck
                .check(&stack, &stack_state(Platform::Machines), &config)
                .await
                .unwrap(),
        ] {
            assert!(result.success);
            assert!(result.errors.is_empty());
        }
    }

    #[tokio::test]
    async fn target_resources_allows_global_exact_and_suffix_patterns() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        resources.insert(
            "deepstore-agent-write".to_string(),
            create_container_entry("deepstore-agent-write"),
        );
        let stack = create_stack(resources);
        let mut config = deployment_config();
        config.environment_variables.variables = vec![
            targeted_env("GLOBAL", None),
            targeted_env("STAR_ALL", Some(vec!["*"])),
            targeted_env("API_KEY", Some(vec!["api"])),
            targeted_env("DEEPSTORE_MODE", Some(vec!["deepstore-agent-*"])),
        ];
        let check = TargetResourcesResolveCheck;

        assert!(check.should_run(&stack, &stack_state(Platform::Aws), &config));
        let result = check
            .check(&stack, &stack_state(Platform::Aws), &config)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn target_resources_rejects_patterns_that_only_match_non_compute_resources() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        resources.insert("storage".to_string(), create_storage_entry("storage"));
        let stack = create_stack(resources);
        let mut config = deployment_config();
        config.environment_variables.variables =
            vec![targeted_env("STORAGE_ONLY", Some(vec!["storage"]))];
        let check = TargetResourcesResolveCheck;

        let result = check
            .check(&stack, &stack_state(Platform::Aws), &config)
            .await
            .unwrap();

        assert!(!result.success);
        let errors = result.errors.join("\n");
        assert!(errors.contains("STORAGE_ONLY"));
        assert!(errors.contains("storage"));
    }

    #[tokio::test]
    async fn target_resources_rejects_empty_unmatched_and_invalid_patterns() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        let stack = create_stack(resources);
        let mut config = deployment_config();
        config.environment_variables.variables = vec![
            targeted_env("EMPTY", Some(vec![])),
            targeted_env("MISSING", Some(vec!["worker"])),
            targeted_env("BAD_WILDCARD", Some(vec!["api*extra"])),
            targeted_env("MISS_WILDCARD", Some(vec!["worker-*"])),
        ];
        let check = TargetResourcesResolveCheck;

        let result = check
            .check(&stack, &stack_state(Platform::Aws), &config)
            .await
            .unwrap();

        assert!(!result.success);
        let errors = result.errors.join("\n");
        assert!(errors.contains("EMPTY"));
        assert!(errors.contains("empty targetResources list"));
        assert!(errors.contains("MISSING"));
        assert!(errors.contains("worker"));
        assert!(errors.contains("BAD_WILDCARD"));
        assert!(errors.contains("api*extra"));
        assert!(errors.contains("MISS_WILDCARD"));
        assert!(errors.contains("worker-*"));
    }
}
