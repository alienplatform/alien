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
