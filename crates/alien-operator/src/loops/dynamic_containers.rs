//! Release-independent Kubernetes workloads requested by the deployment owner.

use std::collections::{BTreeMap, BTreeSet};

use alien_core::sync::{DynamicContainerReport, DynamicContainerStatus, TargetDynamicContainer};
use alien_core::KubernetesClientConfig;
use alien_error::{AlienError, Context, ContextError};
use alien_k8s_clients::{
    Error as KubernetesError, ErrorData as KubernetesErrorData, KubernetesClient,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Capabilities, Container, ContainerPort, EmptyDirVolumeSource, EnvVar, EnvVarSource,
    HTTPGetAction, LocalObjectReference, PodSecurityContext, PodSpec, PodTemplateSpec, Probe,
    ResourceRequirements, SeccompProfile, Secret, SecretKeySelector, SecurityContext, Service,
    ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use crate::OperatorState;

const OWNER_LABEL: &str = "alien.dev/dynamic-deployment";
const NAME_LABEL: &str = "alien.dev/dynamic-container";

fn backend_name(deployment_id: &str, name: &str) -> String {
    let digest = Sha256::digest(format!("{deployment_id}:{name}").as_bytes());
    let hex = format!("{digest:x}");
    format!("dc-{}", &hex[..24])
}

fn labels(deployment_id: &str, name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (OWNER_LABEL.to_string(), deployment_id.to_string()),
        (NAME_LABEL.to_string(), name.to_string()),
    ])
}

fn owned(meta: &ObjectMeta, deployment_id: &str, name: &str) -> bool {
    meta.labels.as_ref().is_some_and(|labels| {
        labels
            .get(OWNER_LABEL)
            .is_some_and(|value| value == deployment_id)
            && labels.get(NAME_LABEL).is_some_and(|value| value == name)
    })
}

fn missing(error: &KubernetesError) -> bool {
    matches!(
        error.error.as_ref(),
        Some(KubernetesErrorData::RemoteResourceNotFound { .. })
    )
}

fn failed(message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::DeploymentFailed {
        message: message.into(),
    })
}

async fn read_deployment(
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

async fn read_service(
    client: &KubernetesClient,
    namespace: &str,
    name: &str,
) -> Result<Option<Service>> {
    match client.get_service(namespace, name).await {
        Ok(value) => Ok(Some(value)),
        Err(error) if missing(&error) => Ok(None),
        Err(error) => Err(error.context(ErrorData::DeploymentFailed {
            message: format!("Failed to read Kubernetes Service {name}"),
        })),
    }
}

async fn read_secret(
    client: &KubernetesClient,
    namespace: &str,
    name: &str,
) -> Result<Option<Secret>> {
    match client.get_secret(namespace, name).await {
        Ok(value) => Ok(Some(value)),
        Err(error) if missing(&error) => Ok(None),
        Err(_) => Err(failed(format!("Failed to read Kubernetes Secret {name}"))),
    }
}

async fn put_secret(
    client: &KubernetesClient,
    namespace: &str,
    target: &TargetDynamicContainer,
    deployment_id: &str,
) -> Result<()> {
    let name = format!("{}-env", backend_name(deployment_id, &target.name));
    let current = read_secret(client, namespace, &name).await?;
    if let Some(existing) = &current {
        if !owned(&existing.metadata, deployment_id, &target.name) {
            return Err(failed(format!(
                "Refusing to replace foreign Kubernetes Secret {name}"
            )));
        }
    }
    if target.secret_env.is_empty() {
        return Ok(());
    }
    if current.as_ref().is_some_and(|secret| {
        target.secret_env.iter().all(|(key, value)| {
            secret
                .data
                .as_ref()
                .and_then(|data| data.get(key))
                .is_some_and(|stored| stored.0 == value.as_bytes())
        }) && secret
            .data
            .as_ref()
            .is_some_and(|data| data.len() == target.secret_env.len())
    }) {
        return Ok(());
    }
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(deployment_id, &target.name)),
            resource_version: current
                .as_ref()
                .and_then(|value| value.metadata.resource_version.clone()),
            ..Default::default()
        },
        string_data: Some(target.secret_env.clone()),
        type_: Some("Opaque".to_string()),
        ..Default::default()
    };
    let result = if current.is_some() {
        client.update_secret(namespace, &name, &secret).await
    } else {
        client.create_secret(namespace, &secret).await
    };
    result
        .map(|_| ())
        .map_err(|_| failed("Failed to apply dynamic container Secret"))
}

/// A deployment token is used only for the manager's own registry host. Never
/// place it in credentials for a caller-selected external registry.
async fn put_registry_secret(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    target: &TargetDynamicContainer,
    registry_auth: Option<(&str, &str)>,
) -> Result<bool> {
    let Some((manager_host, token)) = registry_auth else {
        return Ok(false);
    };
    if target.image.split('/').next() != Some(manager_host) {
        return Ok(false);
    }
    let name = format!("{}-registry", backend_name(deployment_id, &target.name));
    let current = read_secret(client, namespace, &name).await?;
    if let Some(existing) = &current {
        if !owned(&existing.metadata, deployment_id, &target.name) {
            return Err(failed(format!(
                "Refusing to replace foreign Kubernetes Secret {name}"
            )));
        }
    }
    let auths = BTreeMap::from([(
        manager_host.to_string(),
        serde_json::json!({
            "username": "deployment",
            "password": token,
            "auth": BASE64.encode(format!("deployment:{token}")),
        }),
    )]);
    let docker_config = serde_json::to_vec(&serde_json::json!({ "auths": auths }))
        .map_err(|_| failed("Failed to encode registry credentials"))?;
    if current.as_ref().is_some_and(|secret| {
        secret
            .data
            .as_ref()
            .and_then(|data| data.get(".dockerconfigjson"))
            .is_some_and(|stored| stored.0 == docker_config)
    }) {
        return Ok(true);
    }
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels(deployment_id, &target.name)),
            resource_version: current
                .as_ref()
                .and_then(|value| value.metadata.resource_version.clone()),
            ..Default::default()
        },
        type_: Some("kubernetes.io/dockerconfigjson".to_string()),
        data: Some(BTreeMap::from([(
            ".dockerconfigjson".to_string(),
            k8s_openapi::ByteString(docker_config),
        )])),
        ..Default::default()
    };
    let result = if current.is_some() {
        client.update_secret(namespace, &name, &secret).await
    } else {
        client.create_secret(namespace, &secret).await
    };
    result.map_err(|_| failed("Failed to apply dynamic container registry Secret"))?;
    Ok(true)
}

fn desired_service(
    namespace: &str,
    deployment_id: &str,
    target: &TargetDynamicContainer,
) -> Service {
    let name = backend_name(deployment_id, &target.name);
    let scope = labels(deployment_id, &target.name);
    Service {
        metadata: ObjectMeta {
            name: Some(name),
            namespace: Some(namespace.to_string()),
            labels: Some(scope.clone()),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            selector: Some(scope),
            ports: Some(
                target
                    .ports
                    .iter()
                    .map(|port| ServicePort {
                        port: i32::from(*port),
                        target_port: Some(IntOrString::Int(i32::from(*port))),
                        protocol: Some("TCP".to_string()),
                        ..Default::default()
                    })
                    .collect(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn put_service(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    target: &TargetDynamicContainer,
) -> Result<()> {
    let name = backend_name(deployment_id, &target.name);
    let current = read_service(client, namespace, &name).await?;
    if let Some(existing) = &current {
        if !owned(&existing.metadata, deployment_id, &target.name) {
            return Err(failed(format!(
                "Refusing to replace foreign Kubernetes Service {name}"
            )));
        }
        if existing.spec.as_ref().is_some_and(|spec| {
            spec.type_.as_deref() == Some("ClusterIP")
                && spec.selector.as_ref() == Some(&labels(deployment_id, &target.name))
                && spec.ports.as_ref().is_some_and(|ports| {
                    ports.len() == target.ports.len()
                        && ports.iter().zip(&target.ports).all(|(existing, desired)| {
                            existing.port == i32::from(*desired)
                                && existing.target_port
                                    == Some(IntOrString::Int(i32::from(*desired)))
                        })
                })
        }) {
            return Ok(());
        }
    }
    let mut desired = desired_service(namespace, deployment_id, target);
    if let Some(existing) = &current {
        desired.metadata.resource_version = existing.metadata.resource_version.clone();
        if let (Some(spec), Some(current_spec)) = (&mut desired.spec, &existing.spec) {
            spec.cluster_ip = current_spec.cluster_ip.clone();
            spec.cluster_ips = current_spec.cluster_ips.clone();
            spec.ip_families = current_spec.ip_families.clone();
            spec.ip_family_policy = current_spec.ip_family_policy.clone();
        }
    }
    let result = if current.is_some() {
        client.update_service(namespace, &name, &desired).await
    } else {
        client.create_service(namespace, &desired).await
    };
    result.map(|_| ()).context(ErrorData::DeploymentFailed {
        message: format!("Failed to apply Kubernetes Service {name}"),
    })
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
            ..Default::default()
        }),
        period_seconds: Some(10),
        timeout_seconds: Some(3),
        ..Default::default()
    });
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
        liveness_probe: probe,
        resources: Some(ResourceRequirements {
            requests: Some(BTreeMap::from([
                ("cpu".to_string(), Quantity(target.cpu.clone())),
                ("memory".to_string(), Quantity(target.memory.clone())),
            ])),
            limits: Some(BTreeMap::from([
                ("cpu".to_string(), Quantity(target.cpu.clone())),
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

async fn put_deployment(
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
    current_spec.replicas == desired_spec.replicas
        && current_app.image == desired_app.image
        && current_app.env == desired_app.env
        && current_app.ports == desired_app.ports
        && current_app.resources == desired_app.resources
        && current_app.security_context == desired_app.security_context
        && current_app.readiness_probe == desired_app.readiness_probe
        && current_app.liveness_probe == desired_app.liveness_probe
        && current_app.volume_mounts == desired_app.volume_mounts
        && current_pod.volumes == desired_pod.volumes
        && current_pod.automount_service_account_token == Some(false)
        && current_pod.host_network == Some(false)
        && current_pod.host_pid == Some(false)
        && current_pod.host_ipc == Some(false)
        && current_pod.security_context == desired_pod.security_context
        && current_pod.image_pull_secrets == desired_pod.image_pull_secrets
}

async fn remove(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    name: &str,
) -> Result<bool> {
    let backend = backend_name(deployment_id, name);
    if let Some(existing) = read_deployment(client, namespace, &backend).await? {
        if !owned(&existing.metadata, deployment_id, name) {
            return Err(failed(format!(
                "Refusing to delete foreign Kubernetes Deployment {backend}"
            )));
        }
        client
            .delete_deployment(namespace, &backend)
            .await
            .context(ErrorData::DeploymentFailed {
                message: format!("Failed to delete Kubernetes Deployment {backend}"),
            })?;
        return Ok(false);
    }
    if let Some(existing) = read_service(client, namespace, &backend).await? {
        if !owned(&existing.metadata, deployment_id, name) {
            return Err(failed(format!(
                "Refusing to delete foreign Kubernetes Service {backend}"
            )));
        }
        client
            .delete_service(namespace, &backend)
            .await
            .context(ErrorData::DeploymentFailed {
                message: format!("Failed to delete Kubernetes Service {backend}"),
            })?;
        return Ok(false);
    }
    for suffix in ["env", "registry"] {
        let secret_name = format!("{backend}-{suffix}");
        if let Some(existing) = read_secret(client, namespace, &secret_name).await? {
            if !owned(&existing.metadata, deployment_id, name) {
                return Err(failed(format!(
                    "Refusing to delete foreign Kubernetes Secret {secret_name}"
                )));
            }
            client
                .delete_secret(namespace, &secret_name)
                .await
                .map_err(|_| failed("Failed to delete dynamic container Secret"))?;
            return Ok(false);
        }
    }
    Ok(true)
}

/// Converge one complete target set and report each requested generation.
pub async fn reconcile(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    targets: &[TargetDynamicContainer],
    registry_auth: Option<(&str, &str)>,
) -> Result<Vec<DynamicContainerReport>> {
    let selector = format!("{OWNER_LABEL}={deployment_id}");
    let existing = client
        .list_deployments(namespace, Some(selector), None)
        .await
        .context(ErrorData::DeploymentFailed {
            message: "Failed to list owned dynamic Deployments".to_string(),
        })?;
    let service_selector = format!("{OWNER_LABEL}={deployment_id}");
    let services = client
        .list_services(namespace, Some(service_selector), None)
        .await
        .context(ErrorData::DeploymentFailed {
            message: "Failed to list owned dynamic Services".to_string(),
        })?;
    let secret_selector = format!("{OWNER_LABEL}={deployment_id}");
    let secrets = client
        .list_secrets(namespace, Some(secret_selector), None)
        .await
        .map_err(|_| failed("Failed to list owned dynamic Secrets"))?;
    let desired_names: BTreeSet<&str> = targets
        .iter()
        .filter(|target| !target.deleted)
        .map(|target| target.name.as_str())
        .collect();
    let mut reports = Vec::with_capacity(targets.len());
    for target in targets {
        let result = if target.deleted {
            remove(client, namespace, deployment_id, &target.name)
                .await
                .map(|removed| {
                    (
                        if removed {
                            DynamicContainerStatus::Stopped
                        } else {
                            DynamicContainerStatus::Pending
                        },
                        None,
                    )
                })
        } else {
            let mut desired = target.clone();
            if desired.suspended_reason.is_some() {
                desired.replicas = 0;
                desired.secret_env.clear();
            }
            async {
                put_secret(client, namespace, &desired, deployment_id).await?;
                let pull_secret =
                    put_registry_secret(client, namespace, deployment_id, &desired, registry_auth)
                        .await?;
                put_service(client, namespace, deployment_id, &desired).await?;
                let deployment =
                    put_deployment(client, namespace, deployment_id, &desired, pull_secret).await?;
                if desired.secret_env.is_empty() {
                    let secret_name = format!("{}-env", backend_name(deployment_id, &target.name));
                    if let Some(secret) = read_secret(client, namespace, &secret_name).await? {
                        if !owned(&secret.metadata, deployment_id, &target.name) {
                            return Err(failed(format!(
                                "Refusing to delete foreign Kubernetes Secret {secret_name}"
                            )));
                        }
                        client
                            .delete_secret(namespace, &secret_name)
                            .await
                            .map_err(|_| {
                                failed("Failed to delete unused dynamic container Secret")
                            })?;
                    }
                }
                let ready = deployment.status.as_ref().is_some_and(|status| {
                    status.available_replicas.unwrap_or(0) >= desired.replicas as i32
                        && status.observed_generation.unwrap_or(0)
                            >= deployment.metadata.generation.unwrap_or(0)
                });
                if !ready && desired.replicas > 0 {
                    let selector =
                        format!("{OWNER_LABEL}={deployment_id},{NAME_LABEL}={}", target.name);
                    let pods = client
                        .list_pods(namespace, Some(selector), None)
                        .await
                        .context(ErrorData::DeploymentFailed {
                            message: "Failed to observe dynamic container Pods".to_string(),
                        })?;
                    for pod in pods.items {
                        for container in pod
                            .status
                            .into_iter()
                            .flat_map(|status| status.container_statuses.unwrap_or_default())
                        {
                            let reason = container
                                .state
                                .and_then(|state| state.waiting)
                                .and_then(|waiting| waiting.reason);
                            if let Some(
                                reason @ ("ErrImagePull"
                                | "ImagePullBackOff"
                                | "CrashLoopBackOff"
                                | "CreateContainerConfigError"),
                            ) = reason.as_deref()
                            {
                                return Ok((
                                    DynamicContainerStatus::Failing,
                                    Some(reason.to_string()),
                                ));
                            }
                        }
                    }
                }
                if let Some(reason) = &target.suspended_reason {
                    return Ok((DynamicContainerStatus::Failing, Some(reason.clone())));
                }
                Ok((
                    if desired.replicas == 0 {
                        DynamicContainerStatus::Stopped
                    } else if ready {
                        DynamicContainerStatus::Running
                    } else {
                        DynamicContainerStatus::Pending
                    },
                    None,
                ))
            }
            .await
        };
        let (status, message) = result.unwrap_or_else(|error: AlienError<ErrorData>| {
            (
                DynamicContainerStatus::Failing,
                Some(error.to_string().chars().take(2000).collect()),
            )
        });
        reports.push(DynamicContainerReport {
            name: target.name.clone(),
            generation: target.generation,
            status,
            message,
        });
    }
    // Delete stale objects from an older target, including Some([]). A foreign
    // object with the same physical name is rejected by `remove`.
    let stale_names: BTreeSet<String> = existing
        .items
        .iter()
        .map(|resource| &resource.metadata)
        .chain(services.items.iter().map(|resource| &resource.metadata))
        .chain(secrets.items.iter().map(|resource| &resource.metadata))
        .filter_map(|meta| {
            meta.labels
                .as_ref()
                .and_then(|labels| labels.get(NAME_LABEL))
                .cloned()
        })
        .collect();
    for name in stale_names {
        if !desired_names.contains(name.as_str()) {
            remove(client, namespace, deployment_id, &name).await?;
        }
    }
    Ok(reports)
}

/// The target is read from encrypted SQLite, so a restart resumes convergence
/// even if it missed the response that last changed the target.
pub async fn reconcile_saved(
    state: &OperatorState,
    deployment_id: &str,
) -> Result<Option<Vec<DynamicContainerReport>>> {
    let Some(target) = state.db.get_target_dynamic_containers().await? else {
        return Ok(None);
    };
    let namespace = state
        .config
        .namespace
        .as_deref()
        .ok_or_else(|| failed("Dynamic containers require a Kubernetes namespace"))?;
    let client = KubernetesClient::new(KubernetesClientConfig::InCluster {
        namespace: Some(namespace.to_string()),
        additional_headers: None,
    })
    .await
    .context(ErrorData::DeploymentFailed {
        message: "Failed to initialize Kubernetes client for dynamic containers".to_string(),
    })?;
    let registry_auth = state.config.sync.as_ref().and_then(|sync| {
        let host = sync.url.host_str()?;
        let host = match sync.url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        Some((host, sync.token.as_str()))
    });
    reconcile(
        &client,
        namespace,
        deployment_id,
        &target,
        registry_auth
            .as_ref()
            .map(|(host, token)| (host.as_str(), *token)),
    )
    .await
    .map(Some)
}
