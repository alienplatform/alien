use std::collections::BTreeMap;

use alien_core::{Worker, WorkerCode};
use alien_error::AlienError;
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, LocalObjectReference, PodSpec, PodTemplateSpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

use crate::core::{
    kubernetes_branded_resource_labels, kubernetes_runtime_pod_labels, projected_env_vars,
    EnvironmentVariableBuilder, KubernetesEnvSecretPlan, ResourceController,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};

use super::kubernetes::KubernetesWorkerController;

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
        .add_linked_resources(&config.links, ctx, &config.id)
        .await?
        .add_self_worker_binding(&config.id, controller.get_binding_params()?.as_ref())?;

    let (env_map, bindings) = env_builder.build_with_bindings();

    // Kubernetes Workers project Secret-kind env vars as secretKeyRefs.
    // The legacy ALIEN_SECRETS vault-load pointer is never emitted into the
    // pod manifest.
    let env_vars = projected_env_vars(env_secret_plan, bindings, env_map)?;

    let container = Container {
        name: "worker".to_string(),
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
    let pod_annotations = env_secret_plan
        .map(|plan| BTreeMap::from([("env-secret-checksum".to_string(), plan.checksum.clone())]));

    let pod_spec = PodSpec {
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
}
