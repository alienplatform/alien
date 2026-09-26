use super::{backend_name, failed, labels, missing, owned};
use crate::error::{ErrorData, Result};
use alien_core::sync::TargetDynamicContainer;
use alien_error::{Context, ContextError};
use alien_k8s_clients::KubernetesClient;
use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

pub(super) async fn read_service(
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
                        name: Some(format!("tcp-{port}")),
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

pub(super) async fn put_service(
    client: &KubernetesClient,
    namespace: &str,
    deployment_id: &str,
    target: &TargetDynamicContainer,
) -> Result<()> {
    let name = backend_name(deployment_id, &target.name);
    let current = read_service(client, namespace, &name).await?;
    if target.ports.is_empty() {
        if let Some(existing) = current {
            if !owned(&existing.metadata, deployment_id, &target.name) {
                return Err(failed(format!(
                    "Refusing to delete foreign Kubernetes Service {name}"
                )));
            }
            client
                .delete_service(namespace, &name)
                .await
                .context(ErrorData::DeploymentFailed {
                    message: format!("Failed to delete Kubernetes Service {name}"),
                })?;
        }
        return Ok(());
    }
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
                            let name = format!("tcp-{desired}");
                            existing.port == i32::from(*desired)
                                && existing.name.as_deref() == Some(name.as_str())
                                && existing.protocol.as_deref() == Some("TCP")
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
