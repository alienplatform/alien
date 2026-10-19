//! Runtime-owned client and replica-discovery Services for Containers.

use super::kubernetes::{kubernetes_port_name, KubernetesContainerController};
use crate::core::kubernetes_errors::is_remote_resource_conflict;
use crate::core::{
    kubernetes_branded_resource_labels, kubernetes_cleanup_resource_labels,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::Container;
use alien_error::{AlienError, Context, ContextError};
use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use sha2::{Digest, Sha256};
use tracing::debug;

pub(crate) fn peer_service_name(workload: &str) -> String {
    if workload.len() <= 57 {
        return format!("{workload}-peers");
    }
    // Preserve the purpose suffix and distinguish long names with a shared prefix.
    let digest = format!("{:x}", Sha256::digest(workload.as_bytes()));
    format!("{}-{}-peers", &workload[..40], &digest[..16])
}

/// Headless discovery for replicas of the same Container.
fn peer_service(
    service: &k8s_openapi::api::core::v1::Service,
    service_name: &str,
    labels: std::collections::BTreeMap<String, String>,
) -> k8s_openapi::api::core::v1::Service {
    let mut peer = service.clone();
    peer.metadata.name = Some(peer_service_name(service_name));
    let spec = peer.spec.as_mut().expect("generated Service has a spec");
    spec.cluster_ip = Some("None".into());
    // Starting replicas enter discovery as soon as they have an address, so every policy
    // write also reaches a replica that is still catching up. Replica-to-replica writes
    // and snapshots do not wait for readiness; only the client Service waits for it.
    spec.publish_not_ready_addresses = Some(true);
    spec.selector.get_or_insert_default().extend(labels);
    peer
}

impl KubernetesContainerController {
    pub(super) async fn reconcile_internal_service(
        &mut self,
        config: &Container,
        service_name: &str,
        namespace: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        if let Some(service) = self.build_internal_service(config, service_name, namespace, ctx) {
            let peer = config.egress.is_some().then(|| {
                peer_service(
                    &service,
                    service_name,
                    kubernetes_branded_resource_labels(ctx, &config.id),
                )
            });
            self.upsert_service(config, service, ctx, true).await?;
            if let Some(peer) = peer {
                // Save identity before the API write so retries and cleanup cover a
                // successful create whose response was lost.
                self.peer_service_name = peer.metadata.name.clone();
                return self.upsert_service(config, peer, ctx, false).await;
            }
        } else {
            self.delete_internal_service(namespace, service_name, &config.id, ctx, true)
                .await?;
        }
        if let Some(peer) = self.peer_service_name.clone() {
            self.delete_internal_service(namespace, &peer, &config.id, ctx, false)
                .await?;
            self.peer_service_name = None;
        }
        Ok(())
    }

    async fn upsert_service(
        &self,
        config: &Container,
        mut service: Service,
        ctx: &ResourceControllerContext<'_>,
        allow_legacy_scope: bool,
    ) -> Result<()> {
        let service_name = service
            .metadata
            .name
            .clone()
            .expect("generated Service has a name");
        let namespace = service
            .metadata
            .namespace
            .clone()
            .expect("generated Service has a namespace");
        let service_client = ctx
            .service_provider
            .get_kubernetes_service_client(ctx.get_kubernetes_config()?)
            .await?;
        match service_client.create_service(&namespace, &service).await {
            Ok(_) => Ok(()),
            Err(e) if is_remote_resource_conflict(&e) => {
                let existing = service_client
                    .get_service(&namespace, &service_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get internal Service '{}' before update",
                            service_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;
                let desired_labels = service
                    .metadata
                    .labels
                    .as_ref()
                    .cloned()
                    .unwrap_or_default();
                if !crate::core::kubernetes_labels_match_identity(
                    existing.metadata.labels.as_ref(),
                    &desired_labels,
                    &["managed-by", "component", "app"],
                ) || !(crate::core::kubernetes_labels_match_current_scope(
                    existing.metadata.labels.as_ref(),
                    &desired_labels,
                ) || (allow_legacy_scope
                    && crate::core::kubernetes_labels_have_compatible_scope(
                        existing.metadata.labels.as_ref(),
                        &desired_labels,
                    )))
                {
                    return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "Refusing to mutate Kubernetes Service '{service_name}' because it is not owned by Container '{}' in this deployment",
                            config.id
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                service.metadata.resource_version = existing.metadata.resource_version;
                service_client
                    .update_service(&namespace, &service_name, &service)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to update internal Service '{}'", service_name),
                        resource_id: Some(config.id.clone()),
                    })?;
                Ok(())
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to create internal Service '{}'", service_name),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    pub(super) async fn delete_internal_service(
        &self,
        namespace: &str,
        service_name: &str,
        resource_id: &str,
        ctx: &ResourceControllerContext<'_>,
        legacy_owner_proven: bool,
    ) -> Result<()> {
        let kubernetes_config = ctx.get_kubernetes_config()?;
        let service_client = ctx
            .service_provider
            .get_kubernetes_service_client(kubernetes_config)
            .await?;

        let existing = match service_client.get_service(namespace, service_name).await {
            Ok(existing) => existing,
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(());
            }
            Err(e) => {
                return Err(e.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get internal Service '{service_name}' before deletion"
                    ),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
        };
        let mut desired_labels =
            self.build_labels(self.workload_name.as_deref().unwrap_or(service_name));
        desired_labels.extend(kubernetes_cleanup_resource_labels(ctx, resource_id));
        if !crate::core::kubernetes_labels_match_identity(
            existing.metadata.labels.as_ref(),
            &desired_labels,
            &["managed-by", "component", "app"],
        ) || !(crate::core::kubernetes_labels_match_current_scope(
            existing.metadata.labels.as_ref(),
            &desired_labels,
        ) || (legacy_owner_proven
            && crate::core::kubernetes_labels_have_compatible_scope(
                existing.metadata.labels.as_ref(),
                &desired_labels,
            )))
        {
            debug!(
                service_name,
                resource_id,
                "Leaving same-name Kubernetes Service untouched because it is not owned by this Container deployment"
            );
            return Ok(());
        }

        match service_client.delete_service(namespace, service_name).await {
            Ok(()) => Ok(()),
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete internal Service '{}'", service_name),
                resource_id: Some(service_name.to_string()),
            })),
        }
    }

    pub(super) fn build_internal_service(
        &self,
        config: &Container,
        service_name: &str,
        namespace: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Option<Service> {
        if config.ports.is_empty() {
            return None;
        }

        let selector_labels = self.build_labels(service_name);
        let mut labels = selector_labels.clone();
        labels.extend(kubernetes_cleanup_resource_labels(ctx, &config.id));
        Some(Service {
            metadata: ObjectMeta {
                name: Some(service_name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels.clone()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                type_: Some("ClusterIP".to_string()),
                selector: Some(selector_labels),
                ports: Some(
                    config
                        .ports
                        .iter()
                        .map(|port| ServicePort {
                            name: Some(kubernetes_port_name(port)),
                            port: port.port as i32,
                            protocol: Some("TCP".to_string()),
                            target_port: Some(IntOrString::Int(port.port as i32)),
                            ..Default::default()
                        })
                        .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn starting_replicas_are_discoverable_before_they_take_traffic() {
        let service = k8s_openapi::api::core::v1::Service {
            spec: Some(k8s_openapi::api::core::v1::ServiceSpec::default()),
            ..Default::default()
        };
        let peer = super::peer_service(&service, "sandbox-egress-gateway", Default::default());
        let spec = peer.spec.unwrap();
        assert_eq!(spec.cluster_ip.as_deref(), Some("None"));
        // Writers and replica fan-out must reach a replica that is still catching up.
        assert_eq!(spec.publish_not_ready_addresses, Some(true));
        assert_eq!(
            peer.metadata.name.as_deref(),
            Some("sandbox-egress-gateway-peers")
        );
    }
}
