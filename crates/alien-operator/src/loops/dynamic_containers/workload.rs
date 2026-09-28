use super::{backend_name, failed, labels, missing, owned};
use crate::error::{ErrorData, Result};
use alien_core::sync::TargetDynamicContainer;
use alien_error::{Context, ContextError};
use alien_k8s_clients::KubernetesClient;
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Capabilities, Container, ContainerPort, EmptyDirVolumeSource, EnvVar, EnvVarSource,
    HTTPGetAction, LocalObjectReference, PodSecurityContext, PodSpec, PodTemplateSpec, Probe,
    ResourceRequirements, SeccompProfile, SecretKeySelector, SecurityContext, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use std::collections::BTreeMap;

pub(super) async fn read_deployment(
    client: &KubernetesClient,
    namespace: &str,
    name: &str,
) -> Result<Option<Deployment>> {
    match client.get_deployment(namespace, name).await {
        Ok(value) => Ok(Some(value)),
        Err(error) if missing(&error) => Ok(None),
        Err(error) => Err(error.context(ErrorData::DeploymentFailed {
            message: format!("Failed to read Kubernetes Deployment {name}"),
        })),
    }
}

fn desired_deployment(
    namespace: &str,
    deployment_id: &str,
    target: &TargetDynamicContainer,
    manager_pull_secret: bool,
) -> Deployment {
    let name = backend_name(deployment_id, &target.name);
    let scope = labels(deployment_id, &target.name);
    let mut env: Vec<EnvVar> = target
        .env
        .iter()
        .map(|(key, value)| EnvVar {
            name: key.clone(),
            value: Some(value.clone()),
            ..Default::default()
        })
        .collect();
    env.push(EnvVar {
        name: "ALIEN_DYNAMIC_GENERATION".to_string(),
        value: Some(target.generation.to_string()),
        ..Default::default()
    });
    for key in target.secret_env.keys() {
        env.push(EnvVar {
            name: key.clone(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: key.clone(),
                    name: format!("{name}-env"),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
    }
    let probe = target.health_check.as_ref().map(|check| Probe {
        http_get: Some(HTTPGetAction {
            path: Some(check.path.clone()),
            port: IntOrString::Int(i32::from(check.port)),
            scheme: Some("HTTP".to_string()),
            ..Default::default()
        }),
        failure_threshold: Some(3),
        period_seconds: Some(10),
        success_threshold: Some(1),
        timeout_seconds: Some(3),
        ..Default::default()
    });
    // Kubernetes stores decimal CPU values in millicores (for example, 0.1
    // becomes 100m). Use that form in the desired spec so every sync does not
    // trigger an otherwise identical rollout.
    let cpu = target
        .cpu
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| Quantity(format!("{}m", (value * 1000.0).round() as u64)))
        .unwrap_or_else(|| Quantity(target.cpu.clone()));
    let container = Container {
        name: "app".to_string(),
        image: Some(target.image.clone()),
        image_pull_policy: Some("IfNotPresent".to_string()),
        env: Some(env),
        ports: Some(
            target
                .ports
                .iter()
                .map(|port| ContainerPort {
                    container_port: i32::from(*port),
                    protocol: Some("TCP".to_string()),
                    ..Default::default()
                })
                .collect(),
        ),
        readiness_probe: probe.clone(),
        liveness_probe: probe.clone(),
        // Images may prepare an app before opening its port. Kubernetes must
        // wait for that first healthy response before liveness can restart it.
        startup_probe: probe.map(|mut startup| {
            startup.failure_threshold = Some(90);
            startup
        }),
        resources: Some(ResourceRequirements {
            requests: Some(BTreeMap::from([
                ("cpu".to_string(), cpu.clone()),
                ("memory".to_string(), Quantity(target.memory.clone())),
            ])),
            limits: Some(BTreeMap::from([
                ("cpu".to_string(), cpu),
                ("memory".to_string(), Quantity(target.memory.clone())),
            ])),
            ..Default::default()
        }),
        security_context: Some(SecurityContext {
            allow_privilege_escalation: Some(false),
            privileged: Some(false),
            read_only_root_filesystem: Some(true),
            run_as_non_root: Some(true),
            run_as_user: Some(65532),
            run_as_group: Some(65532),
            capabilities: Some(Capabilities {
                drop: Some(vec!["ALL".to_string()]),
                ..Default::default()
            }),
            seccomp_profile: Some(SeccompProfile {
                type_: "RuntimeDefault".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        volume_mounts: Some(vec![
            VolumeMount {
                name: "app".to_string(),
                mount_path: "/app".to_string(),
                ..Default::default()
            },
            VolumeMount {
                name: "tmp".to_string(),
                mount_path: "/tmp".to_string(),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };
    Deployment {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(scope.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(target.replicas as i32),
            selector: LabelSelector {
                match_labels: Some(scope.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(scope),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![container],
                    automount_service_account_token: Some(false),
                    image_pull_secrets: manager_pull_secret.then(|| {
                        vec![LocalObjectReference {
                            name: format!("{name}-registry"),
                        }]
                    }),
                    security_context: Some(PodSecurityContext {
                        fs_group: Some(65532),
                        ..Default::default()
                    }),
                    host_network: Some(false),
                    host_pid: Some(false),
                    host_ipc: Some(false),
                    volumes: Some(vec![
                        Volume {
                            name: "app".to_string(),
                            empty_dir: Some(EmptyDirVolumeSource::default()),
                            ..Default::default()
                        },
                        Volume {
                            name: "tmp".to_string(),
                            empty_dir: Some(EmptyDirVolumeSource::default()),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(super) async fn put_deployment(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    target: &TargetDynamicContainer,
    manager_pull_secret: bool,
) -> Result<Deployment> {
    let name = backend_name(deployment_id, &target.name);
    let current = read_deployment(client, namespace, &name).await?;
    if let Some(existing) = &current {
        if !owned(&existing.metadata, deployment_id, &target.name) {
            return Err(failed(format!(
                "Refusing to replace foreign Kubernetes Deployment {name}"
            )));
        }
    }
    let mut desired = desired_deployment(namespace, deployment_id, target, manager_pull_secret);
    if let Some(existing) = &current {
        if deployment_matches(existing, &desired) {
            return Ok(existing.clone());
        }
        desired.metadata.resource_version = existing.metadata.resource_version.clone();
    }
    let result = if current.is_some() {
        client.update_deployment(namespace, &name, &desired).await
    } else {
        client.create_deployment(namespace, &desired).await
    };
    result.context(ErrorData::DeploymentFailed {
        message: format!("Failed to apply Kubernetes Deployment {name}"),
    })
}

fn deployment_matches(current: &Deployment, desired: &Deployment) -> bool {
    let (Some(current_spec), Some(desired_spec)) = (&current.spec, &desired.spec) else {
        return false;
    };
    let (Some(current_pod), Some(desired_pod)) =
        (&current_spec.template.spec, &desired_spec.template.spec)
    else {
        return false;
    };
    let (Some(current_app), Some(desired_app)) = (
        current_pod.containers.first(),
        desired_pod.containers.first(),
    ) else {
        return false;
    };
    current_pod.containers.len() == 1
        && current_app.name == "app"
        && current_pod
            .init_containers
            .as_ref()
            .is_none_or(Vec::is_empty)
        && current_spec.replicas == desired_spec.replicas
        && current_app.image == desired_app.image
        && current_app.env == desired_app.env
        && current_app.ports == desired_app.ports
        && current_app.resources == desired_app.resources
        && current_app.security_context == desired_app.security_context
        && current_app.readiness_probe == desired_app.readiness_probe
        && current_app.liveness_probe == desired_app.liveness_probe
        && current_app.startup_probe == desired_app.startup_probe
        && current_app.volume_mounts == desired_app.volume_mounts
        && current_pod.volumes == desired_pod.volumes
        && current_pod.automount_service_account_token == Some(false)
        && current_pod.host_network.unwrap_or(false) == desired_pod.host_network.unwrap_or(false)
        && current_pod.host_pid.unwrap_or(false) == desired_pod.host_pid.unwrap_or(false)
        && current_pod.host_ipc.unwrap_or(false) == desired_pod.host_ipc.unwrap_or(false)
        && current_pod.security_context == desired_pod.security_context
        && current_pod.image_pull_secrets == desired_pod.image_pull_secrets
}
