use std::collections::BTreeMap;

use alien_core::{Worker, WorkerCode};
use alien_error::AlienError;
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, LocalObjectReference, PodSpec, PodTemplateSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

use crate::core::{
    kubernetes_branded_resource_labels, kubernetes_runtime_pod_labels, pod_template_annotations,
    projected_env_vars, EnvironmentVariableBuilder, KubernetesEnvSecretPlan, ResourceController,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};

use super::kubernetes::KubernetesWorkerController;

/// Name of the worker's container in its pod template.
pub(super) const WORKER_CONTAINER_NAME: &str = "worker";

pub(super) async fn build_worker_deployment(
    controller: &KubernetesWorkerController,
    config: &Worker,
    function_name: &str,
    namespace: &str,
    service_account_name: &str,
    image_pull_secret_name: Option<&str>,
    env_secret_plan: Option<&KubernetesEnvSecretPlan>,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Deployment> {
    let selector_labels = worker_labels(function_name);
    let labels = workload_labels(ctx, &config.id, selector_labels.clone());
    let pod_labels = kubernetes_runtime_pod_labels(ctx, labels.clone());

    // Determine the container image
    let image = match &config.code {
        WorkerCode::Image { image } => image.clone(),
        WorkerCode::Source { .. } => {
            // For source code, we would need to get the built image from Build resource
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message: "Source-based workers not yet supported in Kubernetes platform"
                    .to_string(),
            }));
        }
    };

    // Build environment variables
    // IMPORTANT: Start with config.environment which includes injected vars from DeploymentConfig
    let env_builder = EnvironmentVariableBuilder::try_new(&config.environment)?
        .add_worker_runtime_env_vars(ctx, &config.id, config.timeout_seconds)?
        .add_linked_resources(&config.runtime_binding_links(), ctx, &config.id)
        .await?
        .add_self_worker_binding(&config.id, controller.get_binding_params()?.as_ref())?;

    let (env_map, bindings) = env_builder.build_with_bindings();

    // Kubernetes Workers project Secret-kind env vars as secretKeyRefs.
    // The legacy ALIEN_SECRETS vault-load pointer is never emitted into the
    // pod manifest.
    let env_vars = projected_env_vars(env_secret_plan, bindings, env_map)?;

    let container = Container {
        name: WORKER_CONTAINER_NAME.to_string(),
        image: Some(image),
        ports: Some(vec![ContainerPort {
            container_port: 8080,
            name: Some("http".to_string()),
            protocol: Some("TCP".to_string()),
            ..Default::default()
        }]),
        env: Some(env_vars),
        resources: Some(k8s_openapi::api::core::v1::ResourceRequirements {
            requests: Some({
                let mut requests = BTreeMap::new();
                requests.insert(
                    "memory".to_string(),
                    k8s_openapi::apimachinery::pkg::api::resource::Quantity(format!(
                        "{}Mi",
                        config.memory_mb
                    )),
                );
                requests.insert(
                    "cpu".to_string(),
                    k8s_openapi::apimachinery::pkg::api::resource::Quantity("100m".to_string()),
                );
                requests
            }),
            limits: Some({
                let mut limits = BTreeMap::new();
                limits.insert(
                    "memory".to_string(),
                    k8s_openapi::apimachinery::pkg::api::resource::Quantity(format!(
                        "{}Mi",
                        config.memory_mb
                    )),
                );
                limits.insert(
                    "cpu".to_string(),
                    k8s_openapi::apimachinery::pkg::api::resource::Quantity("1".to_string()),
                );
                limits
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let image_pull_secrets = image_pull_secret_name.map(|name| {
        vec![LocalObjectReference {
            name: name.to_string(),
        }]
    });
    let pod_annotations = pod_template_annotations(env_secret_plan, &config.environment)?;

    // Source images share the stack build target. Workers do not select a
    // compute pool, but must still run on nodes compatible with that image.
    let node_selector = alien_core::kubernetes_compute_node_selector(ctx.desired_stack, None)
        .map_err(|message| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: config.id.clone(),
                message,
            })
        })?;
    let pod_spec = PodSpec {
        node_selector,
        service_account_name: Some(service_account_name.to_string()),
        containers: vec![container],
        restart_policy: Some("Always".to_string()),
        image_pull_secrets,
        ..Default::default()
    };

    Ok(Deployment {
        metadata: ObjectMeta {
            name: Some(function_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(selector_labels),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(pod_labels),
                    annotations: pod_annotations,
                    ..Default::default()
                }),
                spec: Some(pod_spec),
            },
            ..Default::default()
        }),
        ..Default::default()
    })
}

pub(super) fn worker_labels(function_name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app".to_string(), function_name.to_string()),
        ("managed-by".to_string(), "runtime".to_string()),
        ("component".to_string(), "worker".to_string()),
    ])
}

pub(super) fn kubernetes_namespace(ctx: &ResourceControllerContext<'_>) -> Result<String> {
    let k8s_config = ctx.get_kubernetes_config()?;
    match k8s_config {
        alien_core::KubernetesClientConfig::InCluster { namespace, .. } => {
            namespace.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: "kubernetes".to_string(),
                    message: "Kubernetes namespace not configured in InCluster config".to_string(),
                })
            })
        }
        alien_core::KubernetesClientConfig::Kubeconfig { namespace, .. } => {
            namespace.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: "kubernetes".to_string(),
                    message: "Kubernetes namespace not configured in Kubeconfig".to_string(),
                })
            })
        }
        alien_core::KubernetesClientConfig::Manual { namespace, .. } => {
            namespace.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: "kubernetes".to_string(),
                    message: "Kubernetes namespace not configured in Manual config".to_string(),
                })
            })
        }
    }
}

fn workload_labels(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
    mut labels: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    labels.extend(kubernetes_branded_resource_labels(ctx, resource_id));
    labels
}

#[cfg(test)]
mod tests {
    use alien_core::{
        Resource, Worker, WorkerCode, ENV_ALIEN_SECRETS, ENV_ALIEN_WORKER_TIMEOUT_SECONDS,
    };

    use crate::core::environment_secret_plan;
    use crate::core::kubernetes_manifest_test_support::{
        secret_env_var, KubernetesManifestTestHarness,
    };

    use super::{build_worker_deployment, KubernetesWorkerController};

    #[tokio::test]
    async fn worker_manifest_matches_the_stack_source_build_architecture() {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "registry.example.com/api:1".to_string(),
            })
            .permissions("default".to_string())
            .build();
        let compute = alien_core::ComputeCluster::new("compute".to_string())
            .capacity_group(alien_core::CapacityGroup {
                group_id: "apps".to_string(),
                instance_type: None,
                profile: Some(alien_core::MachineProfile {
                    cpu: "2".to_string(),
                    memory_bytes: 4 << 30,
                    ephemeral_storage_bytes: 20 << 30,
                    architecture: Some(alien_core::instance_catalog::Architecture::X86_64),
                    gpu: None,
                }),
                min_size: 1,
                max_size: 3,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build();
        let stack = alien_core::Stack::new("test".to_string())
            .add(compute, alien_core::ResourceLifecycle::Frozen)
            .build();
        let harness = KubernetesManifestTestHarness::new(Resource::new(worker.clone()), Vec::new());
        let mut ctx = harness.ctx();
        ctx.desired_stack = &stack;
        let manifest = build_worker_deployment(
            &KubernetesWorkerController::default(),
            &worker,
            "api",
            "test-ns",
            "api-sa",
            None,
            None,
            &ctx,
        )
        .await
        .unwrap();
        let selector = manifest
            .spec
            .unwrap()
            .template
            .spec
            .unwrap()
            .node_selector
            .unwrap();
        assert_eq!(
            selector.get("kubernetes.io/arch").map(String::as_str),
            Some("amd64")
        );
    }

    #[tokio::test]
    async fn worker_manifest_projects_secrets_without_alien_secrets_pointer() {
        let alien_secrets_pointer = "{\"keys\":[\"APP_SECRET\"],\"hash\":\"test-hash\"}";
        let mut config = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "registry.example.com/api:1".to_string(),
            })
            .permissions("default".to_string())
            .timeout_seconds(3600)
            .expect("literal Worker timeout is within supported range")
            .build();
        config.environment.insert(
            ENV_ALIEN_SECRETS.to_string(),
            alien_secrets_pointer.to_string(),
        );

        let variables = vec![secret_env_var("APP_SECRET", "s3cret", None)];
        let plan = environment_secret_plan("api", "api", &variables).expect("plan");
        let harness = KubernetesManifestTestHarness::new(Resource::new(config.clone()), variables);
        let controller = KubernetesWorkerController {
            deployment_name: Some("api".to_string()),
            namespace: Some("test-ns".to_string()),
            service_name: Some("api".to_string()),
            worker_id: Some("api".to_string()),
            ..Default::default()
        };

        let deployment = build_worker_deployment(
            &controller,
            &config,
            "api",
            "test-ns",
            "api-sa",
            None,
            Some(&plan),
            &harness.ctx(),
        )
        .await
        .expect("worker deployment manifest");

        let env = deployment
            .spec
            .as_ref()
            .expect("deployment spec")
            .template
            .spec
            .as_ref()
            .expect("pod spec")
            .containers[0]
            .env
            .clone()
            .expect("container env");

        assert!(
            env.iter().all(|var| var.name != ENV_ALIEN_SECRETS),
            "Worker pod manifests must not include the legacy vault-load pointer"
        );
        assert_eq!(
            env.iter()
                .find(|var| var.name == ENV_ALIEN_WORKER_TIMEOUT_SECONDS)
                .and_then(|var| var.value.as_deref()),
            Some("3600"),
            "Kubernetes runtime receives the trusted Worker timeout"
        );

        let projected = env
            .iter()
            .find(|var| var.name == "APP_SECRET")
            .expect("worker manifest projects the secret");
        assert_eq!(
            projected
                .value_from
                .as_ref()
                .and_then(|source| source.secret_key_ref.as_ref())
                .map(|secret_key_ref| secret_key_ref.name.as_str()),
            Some("api-env")
        );
    }

    /// The comparison a Ready tick makes projects the linked bindings exactly as the manifest
    /// does, so a pod template rendered with the current bindings, even after a round trip
    /// through the apiserver's JSON, shows no drift: an identical spec never starts a roll. Only
    /// a binding that moved does.
    #[tokio::test]
    async fn a_rendered_pod_template_shows_drift_only_when_a_linked_binding_moves() {
        use crate::core::linked_bindings::{kubernetes_linked_binding_drift, pod_template_env};
        use crate::sandbox::KubernetesSandboxController;
        use alien_core::{Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy};

        let sandbox = Sandbox::new("agent".to_string())
            .code(SandboxCode::Image {
                image: "registry.example.com/session:1".to_string(),
            })
            .egress(SandboxEgress::None)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: Some(600),
                idle_pause_seconds: None,
            })
            .build();
        let serving = |broker: &str| KubernetesSandboxController {
            state: crate::sandbox::KubernetesSandboxState::Ready,
            sandbox_id: Some("agent".to_string()),
            namespace: Some("test-ns".to_string()),
            runtime_class: Some("gvisor".to_string()),
            broker_url: Some(broker.to_string()),
            deployment_label_key: Some("alien.dev/deployment".to_string()),
            deployment_label_value: Some("test".to_string()),
            egress: Some(SandboxEgress::None.into()),
            ..Default::default()
        };
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "registry.example.com/api:1".to_string(),
            })
            .permissions("default".to_string())
            .link(&sandbox)
            .build();
        let links = worker.runtime_binding_links();

        let old = KubernetesManifestTestHarness::new(Resource::new(worker.clone()), Vec::new())
            .with_linked_resource(sandbox.clone(), serving("http://old.test-ns.svc:8080"));
        let manifest = build_worker_deployment(
            &KubernetesWorkerController::default(),
            &worker,
            "api",
            "test-ns",
            "api-sa",
            None,
            None,
            &old.ctx(),
        )
        .await
        .unwrap();
        // As the apiserver hands it back.
        let live: k8s_openapi::api::apps::v1::Deployment =
            serde_json::from_value(serde_json::to_value(&manifest).unwrap()).unwrap();
        let env = pod_template_env(
            live.spec
                .as_ref()
                .and_then(|spec| spec.template.spec.as_ref()),
            super::WORKER_CONTAINER_NAME,
        )
        .expect("the worker container is in the template");
        assert!(
            env.iter().any(|var| var.name == "ALIEN_AGENT_BINDING"),
            "the manifest carries the sandbox binding"
        );

        assert!(
            kubernetes_linked_binding_drift(&old.ctx(), &links, "api", env)
                .await
                .unwrap()
                .is_empty(),
            "a template rendered with the current bindings must not drift"
        );

        let moved = KubernetesManifestTestHarness::new(Resource::new(worker.clone()), Vec::new())
            .with_linked_resource(sandbox.clone(), serving("http://new.test-ns.svc:8080"));
        assert_eq!(
            kubernetes_linked_binding_drift(&moved.ctx(), &links, "api", env)
                .await
                .unwrap(),
            vec!["ALIEN_AGENT_BINDING".to_string()]
        );

        let rolling = KubernetesManifestTestHarness::new(Resource::new(worker.clone()), Vec::new())
            .with_linked_resource(sandbox, {
                let mut controller = serving("http://new.test-ns.svc:8080");
                controller.state = crate::sandbox::KubernetesSandboxState::UpdatingSandbox;
                controller
            });
        assert!(
            kubernetes_linked_binding_drift(&rolling.ctx(), &links, "api", env)
                .await
                .unwrap()
                .is_empty(),
            "a sandbox in its own flow has not reached the value to compare against"
        );
    }

    /// A binding's secret is projected as a `secretKeyRef` beside the binding variable. A
    /// template rendered with the current reference shows no drift, and a reference that moved
    /// to another Secret shows as drift on that variable alone.
    #[tokio::test]
    async fn a_moved_secret_reference_shows_drift_on_its_secret_key_ref() {
        use crate::core::linked_bindings::{kubernetes_linked_binding_drift, pod_template_env};
        use alien_core::bindings::{BindingValue, KvBinding, RedisKvBinding, SecretReference};
        use alien_core::{ExternalBinding, Kv};

        let cache = Kv::new("cache".to_string()).build();
        let redis = |secret: &str| {
            ExternalBinding::Kv(KvBinding::Redis(RedisKvBinding {
                connection_url: BindingValue::SecretRef {
                    secret_ref: SecretReference {
                        name: secret.to_string(),
                        key: "url".to_string(),
                    },
                },
                key_prefix: None,
                database: None,
            }))
        };
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "registry.example.com/api:1".to_string(),
            })
            .permissions("default".to_string())
            .link(&cache)
            .build();
        let links = worker.runtime_binding_links();

        let current = KubernetesManifestTestHarness::new(Resource::new(worker.clone()), Vec::new())
            .with_external_linked_resource(cache.clone(), redis("redis-creds"));
        let manifest = build_worker_deployment(
            &KubernetesWorkerController::default(),
            &worker,
            "api",
            "test-ns",
            "api-sa",
            None,
            None,
            &current.ctx(),
        )
        .await
        .unwrap();
        let live: k8s_openapi::api::apps::v1::Deployment =
            serde_json::from_value(serde_json::to_value(&manifest).unwrap()).unwrap();
        let env = pod_template_env(
            live.spec
                .as_ref()
                .and_then(|spec| spec.template.spec.as_ref()),
            super::WORKER_CONTAINER_NAME,
        )
        .expect("the worker container is in the template");
        let secret_vars: Vec<&str> = env
            .iter()
            .filter(|var| {
                var.value_from
                    .as_ref()
                    .and_then(|source| source.secret_key_ref.as_ref())
                    .is_some_and(|reference| reference.name == "redis-creds")
            })
            .map(|var| var.name.as_str())
            .collect();
        assert_eq!(
            secret_vars.len(),
            1,
            "the manifest projects the binding's secret as one secretKeyRef: {env:?}"
        );

        assert!(
            kubernetes_linked_binding_drift(&current.ctx(), &links, "api", env)
                .await
                .unwrap()
                .is_empty(),
            "a template rendered with the current secret reference must not drift"
        );

        let moved = KubernetesManifestTestHarness::new(Resource::new(worker.clone()), Vec::new())
            .with_external_linked_resource(cache, redis("redis-creds-rotated"));
        assert_eq!(
            kubernetes_linked_binding_drift(&moved.ctx(), &links, "api", env)
                .await
                .unwrap(),
            vec![secret_vars[0].to_string()]
        );
    }
}
