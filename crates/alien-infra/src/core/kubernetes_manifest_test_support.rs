//! Shared test support for asserting Kubernetes workload manifests as typed
//! k8s-openapi objects.
//!
//! The controllers build manifests as real `k8s_openapi` structs; these
//! helpers own everything a `ResourceControllerContext` borrows so
//! manifest-builder unit tests can call `build_deployment` /
//! `build_statefulset` / `build_daemonset` directly and assert on the typed
//! objects, without an executor or a live cluster.

use crate::core::{
    DefaultPlatformServiceProvider, HeartbeatCollector, PlatformServiceProvider,
    ResourceControllerContext, ResourceRegistry,
};
use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariable, EnvironmentVariableType,
    EnvironmentVariablesSnapshot, ExternalBindings, ManagementPermissions, PermissionProfile,
    PermissionsConfig, Platform, Resource, Stack, StackSettings, StackState,
};
use indexmap::IndexMap;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::core::v1::{EnvVar, PodTemplateSpec};
use std::sync::Arc;

pub(crate) fn secret_env_var(
    name: &str,
    value: &str,
    target_resources: Option<Vec<&str>>,
) -> EnvironmentVariable {
    EnvironmentVariable {
        name: name.to_string(),
        value: value.to_string(),
        var_type: EnvironmentVariableType::Secret,
        target_resources: target_resources
            .map(|targets| targets.into_iter().map(str::to_string).collect()),
    }
}

/// Returns the first container's env from a Deployment pod template.
pub(crate) fn deployment_env(deployment: &Deployment) -> Vec<EnvVar> {
    pod_template_env(
        &deployment
            .spec
            .as_ref()
            .expect("deployment spec")
            .template,
    )
}

/// Returns the first container's env from a DaemonSet pod template.
pub(crate) fn daemonset_env(daemonset: &DaemonSet) -> Vec<EnvVar> {
    pod_template_env(&daemonset.spec.as_ref().expect("daemonset spec").template)
}

/// Returns the first container's env from any workload pod template.
pub(crate) fn pod_template_env(template: &PodTemplateSpec) -> Vec<EnvVar> {
    template
        .spec
        .as_ref()
        .expect("pod spec")
        .containers[0]
        .env
        .clone()
        .expect("container env")
}

/// Reads the `env-secret-checksum` annotation a controller stamps onto the pod
/// template to roll pods when the environment Secret rotates.
pub(crate) fn pod_template_checksum_annotation(template: &PodTemplateSpec) -> Option<String> {
    template
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.annotations.as_ref())
        .and_then(|annotations| annotations.get("env-secret-checksum"))
        .cloned()
}

/// Asserts an env var is a `secretKeyRef` into `secret_name` keyed by its own
/// name, carrying no inline value.
pub(crate) fn assert_secret_key_ref(env: &[EnvVar], name: &str, secret_name: &str) {
    let var = env
        .iter()
        .find(|var| var.name == name)
        .unwrap_or_else(|| panic!("env var '{name}' missing from manifest"));
    assert_eq!(var.value, None, "'{name}' must not carry an inline value");
    let secret_key_ref = var
        .value_from
        .as_ref()
        .and_then(|source| source.secret_key_ref.as_ref())
        .unwrap_or_else(|| panic!("'{name}' must be a secretKeyRef"));
    assert_eq!(secret_key_ref.name, secret_name);
    assert_eq!(secret_key_ref.key, name);
    assert_eq!(secret_key_ref.optional, Some(false));
}

/// Owns the borrowed parts of a Kubernetes `ResourceControllerContext`.
pub(crate) struct KubernetesManifestTestHarness {
    resource: Resource,
    stack: Stack,
    state: StackState,
    registry: Arc<ResourceRegistry>,
    service_provider: Arc<dyn PlatformServiceProvider>,
    deployment_config: DeploymentConfig,
}

impl KubernetesManifestTestHarness {
    pub(crate) fn new(resource: Resource, variables: Vec<EnvironmentVariable>) -> Self {
        let snapshot = EnvironmentVariablesSnapshot {
            variables,
            hash: "test-hash".to_string(),
            created_at: String::new(),
        };

        let mut profiles = IndexMap::new();
        profiles.insert("default".to_string(), PermissionProfile::new());
        let stack = Stack {
            id: "test-stack".to_string(),
            resources: IndexMap::new(),
            permissions: PermissionsConfig {
                profiles,
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: Vec::new(),
        };

        Self {
            resource,
            stack,
            state: StackState::new(Platform::Kubernetes),
            registry: Arc::new(ResourceRegistry::new()),
            service_provider: Arc::new(DefaultPlatformServiceProvider::default()),
            deployment_config: DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .environment_variables(snapshot)
                .allow_frozen_changes(false)
                .external_bindings(ExternalBindings::default())
                .build(),
        }
    }

    pub(crate) fn ctx(&self) -> ResourceControllerContext<'_> {
        ResourceControllerContext {
            desired_config: &self.resource,
            platform: Platform::Kubernetes,
            client_config: ClientConfig::Test,
            state: &self.state,
            resource_prefix: "test",
            registry: &self.registry,
            desired_stack: &self.stack,
            service_provider: &self.service_provider,
            deployment_config: &self.deployment_config,
            heartbeat_collector: HeartbeatCollector::default(),
        }
    }
}
