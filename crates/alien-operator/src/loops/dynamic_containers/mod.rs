//! Release-independent Kubernetes workloads requested by the deployment owner.

mod secrets;
mod service;
mod workload;

use self::secrets::{put_registry_secret, put_secret, read_secret, remove_unused_registry_secret};
use self::service::{put_service, read_service};
use self::workload::{put_deployment, read_deployment};
use crate::error::{ErrorData, Result};
use crate::OperatorState;
use alien_core::sync::{DynamicContainerReport, DynamicContainerStatus, TargetDynamicContainer};
use alien_core::KubernetesClientConfig;
use alien_error::{AlienError, Context};
use alien_k8s_clients::{
    Error as KubernetesError, ErrorData as KubernetesErrorData, KubernetesClient,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const OWNER_LABEL: &str = "alien.dev/dynamic-deployment";
const NAME_LABEL: &str = "alien.dev/dynamic-container";
const MANAGED_BY_LABEL: &str = "managed-by";

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

/// The runtime scope label (`ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY` and `..._VALUE`, the
/// same pair every other object of the release carries), when the Operator runs under
/// a chart that sets one. The chart's pre-delete cleanup selects on it.
fn runtime_scope() -> Option<(String, String)> {
    crate::runtime_deployment_scope_from_env().ok().flatten()
}

/// Labels for the metadata of everything a dynamic container creates: its own
/// labels, plus the release's runtime scope label so `helm uninstall` removes it. Pod
/// selectors keep using [`labels`]: a Deployment's selector can't change.
fn object_labels(deployment_id: &str, name: &str) -> BTreeMap<String, String> {
    let mut object_labels = labels(deployment_id, name);
    if let Some((key, value)) = runtime_scope() {
        object_labels.insert(key, value);
        object_labels.insert(MANAGED_BY_LABEL.to_string(), "runtime".to_string());
    }
    object_labels
}

/// Whether `meta` carries the runtime scope label and `managed-by=runtime` (the pair the
/// chart's cleanup selects on), or there is no scope to carry.
fn has_runtime_scope(meta: &ObjectMeta) -> bool {
    runtime_scope().is_none_or(|(key, value)| {
        let labels = meta.labels.as_ref();
        labels
            .and_then(|labels| labels.get(&key))
            .is_some_and(|stored| *stored == value)
            && labels
                .and_then(|labels| labels.get(MANAGED_BY_LABEL))
                .is_some_and(|stored| stored == "runtime")
    })
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
fn installed_compute_ready(
    deployment: &alien_core::DeploymentState,
    stack: &alien_core::Stack,
) -> bool {
    deployment.current_release.is_some()
        && stack
            .resources()
            .filter(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<alien_core::ComputeCluster>()
                    .is_some()
            })
            .all(|(id, _)| {
                deployment
                    .stack_state
                    .as_ref()
                    .and_then(|state| state.resources.get(id))
                    .is_some_and(|resource| resource.status == alien_core::ResourceStatus::Running)
            })
}

pub async fn reconcile(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    targets: &[TargetDynamicContainer],
    registry_auth: Option<(&str, &str)>,
    installation_pull_secrets: &[String],
    stack: &alien_core::Stack,
) -> Result<Vec<DynamicContainerReport>> {
    reconcile_with_admission(
        client,
        namespace,
        deployment_id,
        targets,
        registry_auth,
        installation_pull_secrets,
        stack,
        None,
    )
    .await
}

async fn reconcile_with_admission(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    targets: &[TargetDynamicContainer],
    registry_auth: Option<(&str, &str)>,
    installation_pull_secrets: &[String],
    stack: &alien_core::Stack,
    blocked: Option<(DynamicContainerStatus, String)>,
) -> Result<Vec<DynamicContainerReport>> {
    // Admission applies only to active targets. Deletions must still converge
    // when an installed boundary is missing, invalid, or temporarily unavailable.
    let placement = if targets.iter().all(|target| target.deleted) || blocked.is_some() {
        Ok(None)
    } else {
        let errors = alien_core::validate_kubernetes_compute(stack);
        if errors.is_empty() {
            alien_core::kubernetes_dynamic_pool(stack)
                .and_then(|pool| alien_core::kubernetes_compute_node_selector(stack, pool))
        } else {
            Err(errors.join("; "))
        }
    };
    let blocked = blocked.or_else(|| {
        placement
            .as_ref()
            .err()
            .map(|message| (DynamicContainerStatus::Failing, message.clone()))
    });
    let node_selector = placement.unwrap_or(None);
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
        } else if let Some((status, message)) = &blocked {
            Ok((*status, Some(message.clone())))
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
                let pull_secrets = pod_pull_secrets(
                    &desired,
                    deployment_id,
                    pull_secret,
                    installation_pull_secrets,
                );
                let deployment = put_deployment(
                    client,
                    namespace,
                    deployment_id,
                    &desired,
                    &pull_secrets,
                    node_selector.clone(),
                )
                .await?;
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
                        && status.updated_replicas.unwrap_or(0) >= desired.replicas as i32
                        && status.replicas.unwrap_or(0) == desired.replicas as i32
                        && status.observed_generation.unwrap_or(0)
                            >= deployment.metadata.generation.unwrap_or(0)
                });
                if ready && !pull_secret {
                    remove_unused_registry_secret(client, namespace, deployment_id, &target.name)
                        .await?;
                }
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

/// The targets with every image moved to the site registry on a mirrored install, as for the
/// release's own workloads.
fn mirror_targets(
    targets: &[TargetDynamicContainer],
    image_registry: Option<&str>,
) -> Vec<TargetDynamicContainer> {
    targets
        .iter()
        .map(|target| {
            let mut target = target.clone();
            if let Some(registry) = image_registry {
                target.image =
                    alien_core::image_rewrite::rewrite_to_site_registry(&target.image, registry);
            }
            target
        })
        .collect()
}

async fn in_cluster_client(state: &OperatorState) -> Result<(KubernetesClient, &str)> {
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
    Ok((client, namespace))
}

/// Protected mode runs no dynamic container: no signed release names one. So
/// the deployment's existing ones, started before its owner pinned release
/// keys, are removed like the stale objects of an emptied target.
pub async fn remove_all(state: &OperatorState, deployment_id: &str) -> Result<()> {
    let (client, namespace) = in_cluster_client(state).await?;
    remove_all_in(&client, namespace, deployment_id).await
}

async fn remove_all_in(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
) -> Result<()> {
    let empty = alien_core::Stack::new("cleanup".to_string()).build();
    reconcile_with_admission(
        client,
        namespace,
        deployment_id,
        &[],
        None,
        &[],
        &empty,
        None,
    )
    .await
    .map(|_| ())
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
    let (client, namespace) = in_cluster_client(state).await?;
    let pull_token = if state.config.sync.is_some() {
        Some(
            crate::daily_identity::request_token(state, serde_json::json!({"type":"registryPull"}))
                .await?,
        )
    } else {
        None
    };
    let registry_auth = state.config.sync.as_ref().and_then(|sync| {
        let host = sync.url.host_str()?;
        let host = match sync.url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        Some((host, pull_token.as_deref()?))
    });
    let deployment = state.db.get_deployment_state().await?;
    let installed_stack = deployment
        .as_ref()
        .and_then(|deployment| deployment.runtime_metadata.as_ref())
        .and_then(|metadata| metadata.prepared_stack.as_ref());
    let installed = deployment
        .as_ref()
        .zip(installed_stack)
        .is_some_and(|(deployment, stack)| installed_compute_ready(deployment, stack));
    let blocked = (!installed).then(|| {
        (
            DynamicContainerStatus::Pending,
            "Dynamic containers are waiting for the installed release and compute pools to be ready"
                .to_string(),
        )
    });
    let cleanup_stack = alien_core::Stack::new("cleanup".to_string()).build();
    let stack = installed_stack.unwrap_or(&cleanup_stack);
    let image_registry = state.config.image_registry.as_deref();
    let target = mirror_targets(&target, image_registry);
    // A mirrored install pulls from the site registry, which the manager's token cannot open.
    let registry_auth = registry_auth
        .as_ref()
        .filter(|_| image_registry.is_none())
        .map(|(host, token)| (host.as_str(), *token));
    reconcile_with_admission(
        &client,
        namespace,
        deployment_id,
        &target,
        registry_auth,
        &state.config.image_pull_secrets,
        stack,
        blocked,
    )
    .await
    .map(Some)
}

/// Pull Secrets for a dynamic container's pod: the manager credential written for this
/// container (when it pulls from the manager), then the installation's own pull Secrets, which
/// carry the site registry's credentials on a mirrored install.
fn pod_pull_secrets(
    target: &TargetDynamicContainer,
    deployment_id: &str,
    manager_pull_secret: bool,
    installation_pull_secrets: &[String],
) -> Vec<String> {
    let mut names = Vec::new();
    if manager_pull_secret {
        names.push(format!(
            "{}-registry",
            backend_name(deployment_id, &target.name)
        ));
    }
    names.extend(installation_pull_secrets.iter().cloned());
    names
}

#[cfg(test)]
mod tests {
    use super::{installed_compute_ready, mirror_targets, pod_pull_secrets};
    use alien_core::{
        ComputeCluster, DeploymentState, DeploymentStatus, Platform, ReleaseInfo, Resource,
        ResourceLifecycle, ResourceStatus, Stack, StackResourceState, StackState,
    };

    #[test]
    fn admission_waits_for_installation_and_keeps_the_installed_boundary_during_updates() {
        let compute = ComputeCluster::new("compute".to_string()).build();
        let stack = Stack::new("test".to_string())
            .add(compute.clone(), ResourceLifecycle::Frozen)
            .build();
        let mut state = DeploymentState {
            platform: Platform::Kubernetes,
            status: DeploymentStatus::Provisioning,
            current_release: None,
            target_release: None,
            stack_state: Some(StackState::new(Platform::Kubernetes)),
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: 1,
        };
        assert!(!installed_compute_ready(&state, &stack));
        state.current_release = Some(ReleaseInfo {
            release_id: Some("release".to_string()),
            version: None,
            description: None,
            stack: stack.clone(),
            signed: None,
            signer_lists: Vec::new(),
        });
        assert!(!installed_compute_ready(&state, &stack));
        let mut resource = StackResourceState::new_pending(
            "compute-cluster".to_string(),
            Resource::new(compute),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        );
        resource.status = ResourceStatus::Running;
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .insert("compute".to_string(), resource);
        state.status = DeploymentStatus::Running;
        assert!(installed_compute_ready(&state, &stack));
        state.status = DeploymentStatus::Updating;
        assert!(installed_compute_ready(&state, &stack));
        state
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .get_mut("compute")
            .unwrap()
            .status = ResourceStatus::RefreshFailed;
        assert!(!installed_compute_ready(&state, &stack));
    }

    #[test]
    fn dynamic_container_images_follow_the_mirror() {
        let target = alien_core::sync::TargetDynamicContainer {
            name: "web".to_string(),
            generation: 1,
            image: "registry.example.com/team/app@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            cpu: "1".to_string(),
            memory: "1Gi".to_string(),
            replicas: 1,
            ports: vec![],
            deleted: false,
            env: Default::default(),
            secret_env: Default::default(),
            value_slots: None,
            health_check: None,
            suspended_reason: None,
        };
        let unmirrored = mirror_targets(std::slice::from_ref(&target), None);
        assert_eq!(unmirrored[0].image, target.image);
        let mirrored = mirror_targets(std::slice::from_ref(&target), Some("mirror.internal/acme"));
        assert!(
            mirrored[0].image.starts_with("mirror.internal/acme/"),
            "{}",
            mirrored[0].image
        );
        assert!(mirrored[0]
            .image
            .ends_with("@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        // A mirrored install keeps the installation's pull Secrets and drops the manager one.
        let site = vec!["site-pull".to_string()];
        assert_eq!(pod_pull_secrets(&target, "dep", false, &site), site);
        let both = pod_pull_secrets(&target, "dep", true, &site);
        assert_eq!(both.len(), 2);
        assert!(both[0].ends_with("-registry"));
        assert_eq!(both[1], "site-pull");
        assert!(pod_pull_secrets(&target, "dep", false, &[]).is_empty());
    }
}

#[cfg(test)]
mod runtime_scope_tests {
    use super::{has_runtime_scope, labels, object_labels};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    // temp_env serializes the tests that touch the process environment.
    #[test]
    fn dynamic_container_objects_carry_the_release_runtime_scope_label() {
        temp_env::with_vars(
            [
                (
                    "ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY",
                    Some("alien.dev/deployment"),
                ),
                ("ALIEN_RUNTIME_DEPLOYMENT_LABEL_VALUE", Some("release-a")),
                ("OPERATOR_RESOURCE_PREFIX", None),
                ("OPERATOR_HELM_RELEASE", None),
            ],
            || {
                let object = object_labels("dep_1", "web");
                assert_eq!(
                    object.get("alien.dev/deployment").map(String::as_str),
                    Some("release-a")
                );
                assert_eq!(
                    object
                        .get("alien.dev/dynamic-deployment")
                        .map(String::as_str),
                    Some("dep_1")
                );
                assert_eq!(
                    object.get("managed-by").map(String::as_str),
                    Some("runtime"),
                    "the chart's cleanup selects on managed-by=runtime and the scope label"
                );
                // Selectors and ownership checks stay what they were: a Deployment's
                // selector can't change, and objects created before this label existed
                // still match.
                assert!(!labels("dep_1", "web").contains_key("alien.dev/deployment"));

                // An object made before the label existed is rewritten to gain it.
                let old = ObjectMeta {
                    labels: Some(labels("dep_1", "web")),
                    ..Default::default()
                };
                assert!(!has_runtime_scope(&old));
                let new = ObjectMeta {
                    labels: Some(object),
                    ..Default::default()
                };
                assert!(has_runtime_scope(&new));
            },
        );

        // No chart scope (a standalone Operator): nothing to add or to require.
        temp_env::with_vars(
            [
                ("ALIEN_RUNTIME_DEPLOYMENT_LABEL_KEY", None::<&str>),
                ("ALIEN_RUNTIME_DEPLOYMENT_LABEL_VALUE", None),
            ],
            || {
                assert_eq!(object_labels("dep_1", "web"), labels("dep_1", "web"));
                assert!(has_runtime_scope(&ObjectMeta::default()));
            },
        );
    }
}
