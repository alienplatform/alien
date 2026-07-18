use std::collections::BTreeMap;

use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::Worker;
use alien_error::{AlienError, Context, ContextError};
use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use tracing::debug;

use crate::core::kubernetes_errors::is_remote_resource_conflict;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

pub(super) async fn reconcile_ready_command_service(
    commands_enabled: &mut bool,
    config: &Worker,
    service_name: &str,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<()> {
    if !*commands_enabled {
        // A legacy pre-push controller must not advertise a capability that was
        // never installed. A real create/update flow may promote it.
        return Ok(());
    }

    reconcile_command_service(config, service_name, namespace, ctx).await?;
    if !config.commands_enabled {
        // Disabling is safe immediately: the owned Service is gone and no
        // output or heartbeat should continue advertising push support.
        *commands_enabled = false;
    }
    Ok(())
}

pub(super) async fn reconcile_command_service(
    config: &Worker,
    service_name: &str,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<()> {
    let kubernetes_config = ctx.get_kubernetes_config()?;
    let service_client = ctx
        .service_provider
        .get_kubernetes_service_client(kubernetes_config)
        .await?;
    let Some(mut service) = build_command_service(config, service_name, namespace) else {
        return delete_command_service(namespace, service_name, &config.id, ctx).await;
    };

    match service_client.create_service(namespace, &service).await {
        Ok(_) => Ok(()),
        Err(error) if is_remote_resource_conflict(&error) => {
            let existing = service_client
                .get_service(namespace, service_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to get internal Worker Service '{}' before update",
                        service_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            if command_service_is_compatible_shared_worker_service(
                &existing,
                service_name,
                &config.id,
            ) {
                debug!(
                    worker_id = %config.id,
                    service_name,
                    "Reusing compatible Helm-managed Worker Service for command push"
                );
                return Ok(());
            }
            ensure_command_service_is_owned(&existing, service_name, &config.id)?;
            if command_service_managed_fields_match(&service, &existing) {
                debug!(
                    worker_id = %config.id,
                    service_name,
                    "Internal Worker Service already matches managed configuration"
                );
                return Ok(());
            }
            preserve_command_service_allocations(&mut service, &existing);
            service_client
                .update_service(namespace, service_name, &service)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to update internal Worker Service '{}'",
                        service_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
            Ok(())
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create internal Worker Service '{}'",
                service_name
            ),
            resource_id: Some(config.id.clone()),
        })),
    }
}

pub(super) async fn delete_command_service(
    namespace: &str,
    service_name: &str,
    resource_id: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<()> {
    let kubernetes_config = ctx.get_kubernetes_config()?;
    let service_client = ctx
        .service_provider
        .get_kubernetes_service_client(kubernetes_config)
        .await?;

    let existing = match service_client.get_service(namespace, service_name).await {
        Ok(service) => service,
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(());
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get internal Worker Service '{}' before deletion",
                    service_name
                ),
                resource_id: Some(resource_id.to_string()),
            }));
        }
    };
    if !command_service_is_owned(&existing, service_name, resource_id) {
        debug!(
            service_name = %service_name,
            resource_id = %resource_id,
            "Leaving same-name Kubernetes Service untouched because it is not owned by this Worker"
        );
        return Ok(());
    }

    match service_client.delete_service(namespace, service_name).await {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to delete internal Worker Service '{}'",
                service_name
            ),
            resource_id: Some(resource_id.to_string()),
        })),
    }
}

fn build_command_service(config: &Worker, service_name: &str, namespace: &str) -> Option<Service> {
    if !config.commands_enabled {
        return None;
    }
    let selector = command_service_selector(service_name);
    let mut labels = selector.clone();
    labels.insert("resource-id".to_string(), config.id.clone());
    Some(Service {
        metadata: ObjectMeta {
            name: Some(service_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                port: 80,
                protocol: Some("TCP".to_string()),
                target_port: Some(IntOrString::Int(8080)),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn command_service_selector(service_name: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("app".to_string(), service_name.to_string()),
        ("managed-by".to_string(), "runtime".to_string()),
        ("component".to_string(), "worker".to_string()),
    ])
}

fn command_service_is_owned(service: &Service, service_name: &str, resource_id: &str) -> bool {
    service.metadata.labels.as_ref().is_some_and(|labels| {
        labels.get("managed-by").map(String::as_str) == Some("runtime")
            && labels.get("component").map(String::as_str) == Some("worker")
            && labels.get("app").map(String::as_str) == Some(service_name)
            && labels.get("resource-id").map(String::as_str) == Some(resource_id)
    })
}

fn command_service_is_compatible_shared_worker_service(
    service: &Service,
    service_name: &str,
    resource_id: &str,
) -> bool {
    let labels_match =
        service.metadata.labels.as_ref().is_some_and(|labels| {
            labels.get("resource-id").map(String::as_str) == Some(resource_id)
        });
    let Some(spec) = service.spec.as_ref() else {
        return false;
    };
    let selector_matches = spec.selector.as_ref().is_some_and(|selector| {
        selector.get("app").map(String::as_str) == Some(service_name)
            && selector.get("managed-by").map(String::as_str) == Some("runtime")
            && selector.get("component").map(String::as_str) == Some("worker")
    });
    let port_matches = spec.ports.as_ref().is_some_and(|ports| {
        ports.iter().any(|port| {
            port.port == 80
                && port.target_port.as_ref() == Some(&IntOrString::Int(8080))
                && port
                    .protocol
                    .as_deref()
                    .is_none_or(|protocol| protocol == "TCP")
        })
    });

    labels_match
        && spec
            .type_
            .as_deref()
            .is_none_or(|service_type| service_type == "ClusterIP")
        && selector_matches
        && port_matches
}

fn ensure_command_service_is_owned(
    service: &Service,
    service_name: &str,
    resource_id: &str,
) -> Result<()> {
    if command_service_is_owned(service, service_name, resource_id) {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::ResourceConfigInvalid {
        message: format!(
            "Refusing to mutate Kubernetes Service '{service_name}' because it is not owned by Worker '{resource_id}'"
        ),
        resource_id: Some(resource_id.to_string()),
    }))
}

fn preserve_command_service_allocations(desired: &mut Service, existing: &Service) {
    desired.metadata.resource_version = existing.metadata.resource_version.clone();

    let (Some(desired_spec), Some(existing_spec)) = (desired.spec.as_mut(), existing.spec.as_ref())
    else {
        return;
    };

    // These values are selected or defaulted by the API server. Re-sending a
    // replacement Service without them can either fail validation (clusterIP
    // is immutable) or accidentally request a different IP-family contract.
    desired_spec.cluster_ip = existing_spec.cluster_ip.clone();
    desired_spec.cluster_ips = existing_spec.cluster_ips.clone();
    desired_spec.ip_families = existing_spec.ip_families.clone();
    desired_spec.ip_family_policy = existing_spec.ip_family_policy.clone();
}

fn command_service_managed_fields_match(desired: &Service, existing: &Service) -> bool {
    let desired_labels = desired.metadata.labels.as_ref();
    let existing_labels = existing.metadata.labels.as_ref();
    let labels_match = desired_labels.is_none_or(|desired_labels| {
        existing_labels.is_some_and(|existing_labels| {
            desired_labels
                .iter()
                .all(|(name, value)| existing_labels.get(name) == Some(value))
        })
    });
    if !labels_match {
        return false;
    }

    let (Some(desired_spec), Some(existing_spec)) = (desired.spec.as_ref(), existing.spec.as_ref())
    else {
        return desired.spec.is_none() && existing.spec.is_none();
    };
    if desired_spec.type_ != existing_spec.type_ || desired_spec.selector != existing_spec.selector
    {
        return false;
    }

    let desired_ports = desired_spec.ports.as_deref().unwrap_or_default();
    let existing_ports = existing_spec.ports.as_deref().unwrap_or_default();
    desired_ports.len() == existing_ports.len()
        && desired_ports.iter().all(|desired_port| {
            existing_ports.iter().any(|existing_port| {
                desired_port.name == existing_port.name
                    && desired_port.port == existing_port.port
                    && desired_port.protocol == existing_port.protocol
                    && desired_port.target_port == existing_port.target_port
                    && desired_port.app_protocol == existing_port.app_protocol
            })
        })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{Worker, WorkerCode};
    use alien_error::AlienError;
    use alien_k8s_clients::kubernetes::services::{MockServiceApi, ServiceApi};
    use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

    use crate::core::{
        kubernetes_manifest_test_support::KubernetesManifestTestHarness,
        MockPlatformServiceProvider,
    };

    use super::{
        build_command_service, command_service_is_compatible_shared_worker_service,
        command_service_is_owned, delete_command_service, reconcile_command_service,
        reconcile_ready_command_service,
    };

    fn worker(commands_enabled: bool) -> Worker {
        let builder = Worker::new("worker".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string());
        if commands_enabled {
            builder.commands_enabled(true).build()
        } else {
            builder.build()
        }
    }

    fn service(labels: BTreeMap<String, String>) -> Service {
        Service {
            metadata: ObjectMeta {
                name: Some("test-worker".to_string()),
                namespace: Some("test-ns".to_string()),
                labels: Some(labels),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn provider_with_service_client(
        service_client: Arc<dyn ServiceApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_kubernetes_service_client()
            .returning(move |_| Ok(service_client.clone()));
        Arc::new(provider)
    }

    #[test]
    fn internal_command_service_only_exists_when_commands_are_enabled() {
        let disabled = worker(false);
        let enabled = worker(true);

        assert!(build_command_service(&disabled, "test-worker", "test-namespace").is_none());
        let service = build_command_service(&enabled, "test-worker", "test-namespace")
            .expect("commands-enabled Worker needs an internal push Service");
        let spec = service.spec.expect("Service spec");
        assert_eq!(spec.type_.as_deref(), Some("ClusterIP"));
        assert!(spec.health_check_node_port.is_none());
        let ports = spec.ports.as_ref().expect("Service ports");
        assert_eq!(ports[0].port, 80);
        assert!(ports[0].node_port.is_none());
        assert_eq!(
            service
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("resource-id"))
                .map(String::as_str),
            Some("worker")
        );
        assert_eq!(
            spec.selector
                .as_ref()
                .and_then(|selector| selector.get("resource-id")),
            None,
            "ownership labels must not narrow the pod selector"
        );
    }

    #[test]
    fn legacy_deployment_selector_is_not_a_compatible_shared_worker_service() {
        let mut legacy_service = service(BTreeMap::from([(
            "resource-id".to_string(),
            "worker".to_string(),
        )]));
        legacy_service.spec = Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            selector: Some(BTreeMap::from([
                ("app".to_string(), "test-worker".to_string()),
                ("managed-by".to_string(), "deployment".to_string()),
                ("component".to_string(), "worker".to_string()),
            ])),
            ports: Some(vec![ServicePort {
                protocol: Some("TCP".to_string()),
                port: 80,
                target_port: Some(IntOrString::Int(8080)),
                ..Default::default()
            }]),
            ..Default::default()
        });

        assert!(!command_service_is_compatible_shared_worker_service(
            &legacy_service,
            "test-worker",
            "worker"
        ));
    }

    #[tokio::test]
    async fn ready_capability_repairs_deleted_owned_command_service() {
        let config = worker(true);
        let mut services = MockServiceApi::new();
        services
            .expect_create_service()
            .withf(|namespace, service| {
                namespace == "test-ns" && command_service_is_owned(service, "test-worker", "worker")
            })
            .times(1)
            .returning(|_, service| Ok(service.clone()));
        let services: Arc<dyn ServiceApi> = Arc::new(services);
        let harness =
            KubernetesManifestTestHarness::new(alien_core::Resource::new(config.clone()), vec![])
                .with_service_provider(provider_with_service_client(services));
        let mut commands_enabled = true;

        reconcile_ready_command_service(
            &mut commands_enabled,
            &config,
            "test-worker",
            "test-ns",
            &harness.ctx(),
        )
        .await
        .expect("repair command Service");

        assert!(commands_enabled);
    }

    #[tokio::test]
    async fn ready_disable_removes_service_and_stops_advertising_push() {
        let config = worker(false);
        let owned = service(BTreeMap::from([
            ("managed-by".to_string(), "runtime".to_string()),
            ("component".to_string(), "worker".to_string()),
            ("app".to_string(), "test-worker".to_string()),
            ("resource-id".to_string(), "worker".to_string()),
        ]));
        let mut services = MockServiceApi::new();
        services
            .expect_get_service()
            .times(1)
            .return_once(move |_, _| Ok(owned));
        services
            .expect_delete_service()
            .times(1)
            .return_once(|_, _| Ok(()));
        let services: Arc<dyn ServiceApi> = Arc::new(services);
        let harness =
            KubernetesManifestTestHarness::new(alien_core::Resource::new(config.clone()), vec![])
                .with_service_provider(provider_with_service_client(services));
        let mut commands_enabled = true;

        reconcile_ready_command_service(
            &mut commands_enabled,
            &config,
            "test-worker",
            "test-ns",
            &harness.ctx(),
        )
        .await
        .expect("disable command push");

        assert!(!commands_enabled);
    }

    #[tokio::test]
    async fn reconcile_rejects_foreign_same_name_service() {
        let config = worker(true);
        let foreign = service(BTreeMap::from([(
            "managed-by".to_string(),
            "someone-else".to_string(),
        )]));
        let mut services = MockServiceApi::new();
        services
            .expect_create_service()
            .times(1)
            .return_once(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "Service".to_string(),
                        resource_name: "test-worker".to_string(),
                        message: "AlreadyExists".to_string(),
                    },
                ))
            });
        services
            .expect_get_service()
            .times(1)
            .return_once(move |_, _| Ok(foreign));
        services.expect_update_service().times(0);
        let services: Arc<dyn ServiceApi> = Arc::new(services);
        let harness =
            KubernetesManifestTestHarness::new(alien_core::Resource::new(config.clone()), vec![])
                .with_service_provider(provider_with_service_client(services));

        let error = reconcile_command_service(&config, "test-worker", "test-ns", &harness.ctx())
            .await
            .expect_err("foreign Service must not be adopted");
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
    }

    #[tokio::test]
    async fn reconcile_reuses_compatible_helm_managed_worker_service() {
        let config = worker(true);
        let mut helm_service = service(BTreeMap::from([(
            "resource-id".to_string(),
            "worker".to_string(),
        )]));
        helm_service.spec = Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            selector: Some(BTreeMap::from([
                ("app".to_string(), "test-worker".to_string()),
                ("managed-by".to_string(), "runtime".to_string()),
                ("component".to_string(), "worker".to_string()),
            ])),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                protocol: Some("TCP".to_string()),
                port: 80,
                target_port: Some(IntOrString::Int(8080)),
                ..Default::default()
            }]),
            ..Default::default()
        });

        let mut services = MockServiceApi::new();
        services
            .expect_create_service()
            .times(1)
            .return_once(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "Service".to_string(),
                        resource_name: "test-worker".to_string(),
                        message: "AlreadyExists".to_string(),
                    },
                ))
            });
        services
            .expect_get_service()
            .times(1)
            .return_once(move |_, _| Ok(helm_service));
        services.expect_update_service().times(0);
        let services: Arc<dyn ServiceApi> = Arc::new(services);
        let harness =
            KubernetesManifestTestHarness::new(alien_core::Resource::new(config.clone()), vec![])
                .with_service_provider(provider_with_service_client(services));

        reconcile_command_service(&config, "test-worker", "test-ns", &harness.ctx())
            .await
            .expect("compatible Helm-managed Worker Service can carry command push traffic");
    }

    #[tokio::test]
    async fn reconcile_managed_drift_preserves_cluster_assigned_service_networking() {
        let config = worker(true);
        let mut existing = service(BTreeMap::from([
            ("managed-by".to_string(), "runtime".to_string()),
            ("component".to_string(), "worker".to_string()),
            ("app".to_string(), "test-worker".to_string()),
            ("resource-id".to_string(), "worker".to_string()),
        ]));
        existing.metadata.resource_version = Some("42".to_string());
        existing.spec = Some(ServiceSpec {
            type_: Some("ClusterIP".to_string()),
            cluster_ip: Some("10.96.12.34".to_string()),
            cluster_ips: Some(vec!["10.96.12.34".to_string(), "fd00:1234::20".to_string()]),
            ip_families: Some(vec!["IPv4".to_string(), "IPv6".to_string()]),
            ip_family_policy: Some("PreferDualStack".to_string()),
            ports: Some(vec![ServicePort {
                name: Some("http".to_string()),
                protocol: Some("TCP".to_string()),
                port: 80,
                ..Default::default()
            }]),
            ..Default::default()
        });

        let mut services = MockServiceApi::new();
        services
            .expect_create_service()
            .times(1)
            .return_once(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "Service".to_string(),
                        resource_name: "test-worker".to_string(),
                        message: "AlreadyExists".to_string(),
                    },
                ))
            });
        services
            .expect_get_service()
            .times(1)
            .return_once(move |_, _| Ok(existing));
        services
            .expect_update_service()
            .withf(|namespace, name, service| {
                let Some(spec) = service.spec.as_ref() else {
                    return false;
                };
                namespace == "test-ns"
                    && name == "test-worker"
                    && service.metadata.resource_version.as_deref() == Some("42")
                    && spec.cluster_ip.as_deref() == Some("10.96.12.34")
                    && spec.cluster_ips.as_deref()
                        == Some(&["10.96.12.34".to_string(), "fd00:1234::20".to_string()][..])
                    && spec.ip_families.as_deref()
                        == Some(&["IPv4".to_string(), "IPv6".to_string()][..])
                    && spec.ip_family_policy.as_deref() == Some("PreferDualStack")
                    && spec
                        .selector
                        .as_ref()
                        .and_then(|selector| selector.get("app"))
                        .map(String::as_str)
                        == Some("test-worker")
                    && spec
                        .ports
                        .as_ref()
                        .and_then(|ports| ports.first())
                        .and_then(|port| port.target_port.as_ref())
                        == Some(&IntOrString::Int(8080))
            })
            .times(1)
            .returning(|_, _, service| Ok(service.clone()));
        let services: Arc<dyn ServiceApi> = Arc::new(services);
        let harness =
            KubernetesManifestTestHarness::new(alien_core::Resource::new(config.clone()), vec![])
                .with_service_provider(provider_with_service_client(services));

        reconcile_command_service(&config, "test-worker", "test-ns", &harness.ctx())
            .await
            .expect("owned Service update preserves API-server allocations");
    }

    #[tokio::test]
    async fn reconcile_unchanged_owned_service_skips_update() {
        let config = worker(true);
        let mut existing =
            build_command_service(&config, "test-worker", "test-ns").expect("Service");
        existing.metadata.resource_version = Some("42".to_string());
        let spec = existing.spec.as_mut().expect("Service spec");
        spec.cluster_ip = Some("10.96.12.34".to_string());
        spec.cluster_ips = Some(vec!["10.96.12.34".to_string(), "fd00:1234::20".to_string()]);
        spec.ip_families = Some(vec!["IPv4".to_string(), "IPv6".to_string()]);
        spec.ip_family_policy = Some("PreferDualStack".to_string());
        spec.session_affinity = Some("None".to_string());
        spec.internal_traffic_policy = Some("Cluster".to_string());

        let mut services = MockServiceApi::new();
        services
            .expect_create_service()
            .times(1)
            .return_once(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        resource_type: "Service".to_string(),
                        resource_name: "test-worker".to_string(),
                        message: "AlreadyExists".to_string(),
                    },
                ))
            });
        services
            .expect_get_service()
            .times(1)
            .return_once(move |_, _| Ok(existing));
        services.expect_update_service().times(0);
        let services: Arc<dyn ServiceApi> = Arc::new(services);
        let harness =
            KubernetesManifestTestHarness::new(alien_core::Resource::new(config.clone()), vec![])
                .with_service_provider(provider_with_service_client(services));

        reconcile_command_service(&config, "test-worker", "test-ns", &harness.ctx())
            .await
            .expect("unchanged owned Service is already reconciled");
    }

    #[tokio::test]
    async fn delete_preserves_foreign_same_name_service() {
        let config = worker(true);
        let foreign = service(BTreeMap::from([(
            "managed-by".to_string(),
            "someone-else".to_string(),
        )]));
        let mut services = MockServiceApi::new();
        services
            .expect_get_service()
            .times(1)
            .return_once(move |_, _| Ok(foreign));
        services.expect_delete_service().times(0);
        let services: Arc<dyn ServiceApi> = Arc::new(services);
        let harness = KubernetesManifestTestHarness::new(alien_core::Resource::new(config), vec![])
            .with_service_provider(provider_with_service_client(services));

        delete_command_service("test-ns", "test-worker", "worker", &harness.ctx())
            .await
            .expect("foreign Service is preserved");
    }
}
