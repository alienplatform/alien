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
    EnvironmentVariablesSnapshot, ExternalBindings, KubernetesClientConfig, ManagementPermissions,
    OtlpConfig, PermissionProfile, PermissionsConfig, Platform, Resource, Stack, StackSettings,
    StackState,
};
use indexmap::IndexMap;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::core::v1::{EnvVar, PodTemplateSpec};
use std::sync::Arc;

/// Answer pod listings with no pods: controllers that wait for a workload also
/// look at its pods to explain a stuck start.
pub(crate) fn mock_empty_pod_list(server: &httpmock::MockServer) {
    server.mock(|when, then| {
        when.method(httpmock::Method::GET)
            .path_matches(httpmock::Regex::new(r"^/api/v1/namespaces/[^/]+/pods$").unwrap());
        then.status(200).json_body(serde_json::json!({
            "apiVersion": "v1", "kind": "PodList", "metadata": {}, "items": []
        }));
    });
}

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
    pod_template_env(&deployment.spec.as_ref().expect("deployment spec").template)
}

/// Returns the first container's env from a DaemonSet pod template.
pub(crate) fn daemonset_env(daemonset: &DaemonSet) -> Vec<EnvVar> {
    pod_template_env(&daemonset.spec.as_ref().expect("daemonset spec").template)
}

/// Returns the first container's env from any workload pod template.
pub(crate) fn pod_template_env(template: &PodTemplateSpec) -> Vec<EnvVar> {
    template.spec.as_ref().expect("pod spec").containers[0]
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
    /// The cloud the cluster runs in, for controllers that also call that cloud's APIs.
    cloud: Option<ClientConfig>,
    /// The API server to talk to; an address that does not resolve when unset.
    server_url: Option<String>,
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            built_architectures: Default::default(),
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
            cloud: None,
            server_url: None,
        }
    }

    pub(crate) fn with_server_url(mut self, url: String) -> Self {
        self.server_url = Some(url);
        self
    }

    /// Pulls every workload image from the site registry `registry`.
    pub(crate) fn with_image_registry(mut self, registry: &str) -> Self {
        self.deployment_config.image_registry = Some(registry.to_string());
        self
    }

    /// Adds a resource the workload links, in the state its controller holds.
    pub(crate) fn with_linked_resource<R: alien_core::ResourceDefinition>(
        mut self,
        resource: R,
        controller: impl crate::core::ResourceController + 'static,
    ) -> Self {
        let resource = Resource::new(resource);
        let state = alien_core::StackResourceState::builder()
            .resource_type(resource.resource_type().to_string())
            .status(controller.get_status())
            .config(resource.clone())
            .internal_state(
                crate::core::serialize_controller(&controller).expect("controller serialization"),
            )
            .lifecycle(alien_core::ResourceLifecycle::Live)
            .dependencies(vec![])
            .build();
        self.state
            .resources
            .insert(resource.id().to_string(), state);
        self
    }

    /// Adds a linked resource the deployment binds to pre-existing infrastructure: Running, with
    /// no controller state, and `binding` in the deployment's external bindings.
    pub(crate) fn with_external_linked_resource<R: alien_core::ResourceDefinition>(
        mut self,
        resource: R,
        binding: alien_core::ExternalBinding,
    ) -> Self {
        let resource = Resource::new(resource);
        let state = alien_core::StackResourceState::builder()
            .resource_type(resource.resource_type().to_string())
            .status(alien_core::ResourceStatus::Running)
            .config(resource.clone())
            .lifecycle(alien_core::ResourceLifecycle::Live)
            .dependencies(vec![])
            .build();
        self.state
            .resources
            .insert(resource.id().to_string(), state);
        self.deployment_config
            .external_bindings
            .insert(resource.id().to_string(), binding);
        self
    }

    pub(crate) fn with_cloud(mut self, cloud: ClientConfig) -> Self {
        self.cloud = Some(cloud);
        self
    }

    pub(crate) fn deployment_config_mut(&mut self) -> &mut DeploymentConfig {
        &mut self.deployment_config
    }

    pub(crate) fn with_monitoring(mut self, monitoring: OtlpConfig) -> Self {
        self.deployment_config.monitoring = Some(monitoring);
        self
    }

    pub(crate) fn with_service_provider(
        mut self,
        service_provider: Arc<dyn PlatformServiceProvider>,
    ) -> Self {
        self.service_provider = service_provider;
        self
    }

    pub(crate) fn ctx(&self) -> ResourceControllerContext<'_> {
        let kubernetes = Box::new(KubernetesClientConfig::Manual {
            server_url: self
                .server_url
                .clone()
                .unwrap_or_else(|| "https://kubernetes.test".to_string()),
            certificate_authority_data: None,
            insecure_skip_tls_verify: Some(true),
            client_certificate_data: None,
            client_key_data: None,
            token: None,
            username: None,
            password: None,
            namespace: Some("test-ns".to_string()),
            additional_headers: Default::default(),
        });
        let client_config = match &self.cloud {
            Some(cloud) => ClientConfig::KubernetesCloud {
                kubernetes,
                cloud: Box::new(cloud.clone()),
            },
            None => ClientConfig::Kubernetes(kubernetes),
        };
        ResourceControllerContext {
            desired_config: &self.resource,
            platform: Platform::Kubernetes,
            client_config,
            state: &self.state,
            resource_prefix: "test",
            registry: &self.registry,
            desired_stack: &self.stack,
            service_provider: &self.service_provider,
            deployment_config: &self.deployment_config,
            initial_setup_authority: alien_core::InitialSetupAuthority::DirectSetup,
            heartbeat_collector: HeartbeatCollector::default(),
        }
    }
}

/// Generated Kubernetes exposure (Ingress, managed TLS secret) with domain metadata for
/// `resource_id`, at the given certificate status. The metadata carries certificate material
/// only when `material` is set, as the manager does for an issued certificate.
pub(crate) fn managed_certificate_config(
    config: &mut DeploymentConfig,
    resource_id: &str,
    status: alien_core::CertificateStatus,
    material: bool,
) {
    use alien_core::{
        DnsRecordStatus, DomainMetadata, KubernetesCertificateMode, KubernetesExposureSettings,
        KubernetesIngressRouteProfile, KubernetesRouteProfile, KubernetesSettings,
        ResourceDomainInfo,
    };
    config.stack_settings.kubernetes = Some(KubernetesSettings {
        cluster: None,
        exposure: Some(KubernetesExposureSettings::Generated {
            route: KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
                ingress_class_name: "traefik".into(),
                ..Default::default()
            }),
            certificate: KubernetesCertificateMode::ManagedTlsSecret {
                secret_name_template: "{resource}-tls".into(),
            },
        }),
    });
    config.domain_metadata = Some(DomainMetadata {
        base_domain: "example.com".into(),
        public_subdomain: "app".into(),
        hosted_zone_id: "Z1".into(),
        resources: std::collections::HashMap::from([(
            resource_id.to_string(),
            ResourceDomainInfo {
                fqdn: format!("{resource_id}.example.com"),
                certificate_id: "cert-1".into(),
                certificate_status: status,
                dns_status: DnsRecordStatus::Pending,
                dns_error: None,
                certificate_chain: material.then(|| "chain".to_string()),
                private_key: material.then(|| "key".to_string()),
                issued_at: material.then(|| "2026-01-01T00:00:00Z".to_string()),
                endpoints: Default::default(),
                aliases: Vec::new(),
            },
        )]),
    });
}

/// Drives a controller that is in its public endpoint state (`controller`) through a managed
/// certificate that is still being issued, as every Kubernetes workload controller must:
/// it waits past the 60 polls other endpoint waits get, still waits well inside the saved
/// 30-minute bound, fails with a timeout once the bound passes, and keeps waiting on a status the issuer
/// reports as failed (it retries). `backdate` moves the saved start of the certificate wait to a unix time.
pub(crate) async fn check_certificate_wait<C>(
    harness: &mut KubernetesManifestTestHarness,
    controller: C,
    resource_id: &str,
    status: alien_core::ResourceStatus,
    backdate: impl Fn(&mut C, i64),
) where
    C: crate::core::ResourceController + Clone,
{
    use alien_core::CertificateStatus;
    managed_certificate_config(
        harness.deployment_config_mut(),
        resource_id,
        CertificateStatus::Pending,
        false,
    );
    let mut controller = controller;
    for poll in 0..80 {
        controller
            .step(&harness.ctx())
            .await
            .unwrap_or_else(|error| panic!("poll {poll} of a pending certificate: {error}"));
        assert_eq!(controller.get_status(), status);
    }

    let now = chrono::Utc::now().timestamp();
    backdate(&mut controller, now - 5 * 60);
    controller
        .step(&harness.ctx())
        .await
        .expect("still inside the certificate bound");
    assert_eq!(controller.get_status(), status);

    backdate(&mut controller, now - 11 * 60);
    let error = controller
        .step(&harness.ctx())
        .await
        .expect_err("the saved bound has passed");
    // The error the 60-poll limit gave before: the deployment retries automatically after it.
    assert_eq!(error.code, "RESOURCE_POLLING_TIMEOUT", "{error}");
    assert!(!error.retryable, "{error}");

    // A status the issuer reports as failed is retried by the issuer: it still waits.
    managed_certificate_config(
        harness.deployment_config_mut(),
        resource_id,
        CertificateStatus::Failed,
        false,
    );
    controller
        .step(&harness.ctx())
        .await
        .expect("a failed status waits like pending");
    assert_eq!(controller.get_status(), status);
}
