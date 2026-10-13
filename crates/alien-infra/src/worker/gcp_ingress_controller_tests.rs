use super::tests::{
    create_completed_operation_response, create_empty_iam_policy,
    create_successful_service_response, create_test_domain_metadata, mock_negs,
    setup_mock_service_provider, NegCloud,
};
use super::{CloudRunIngress, GcpWorkerController, GcpWorkerState, Service};
use crate::core::controller_test::SingleControllerExecutor;
use crate::core::{ResourceController, StackExecutor, StackResourceStateExt, StackStateExt};
use crate::gcp_https_ingress::tests::{provider, Provider};
use crate::worker::fixtures::function_public_ingress;
use crate::GcpNetworkController;
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    DnsRecordStatus, EndpointAccess, ManagedDomainInfo, Network, NetworkSettings, Platform,
    ResourceLifecycle, ResourceStatus, StackSettings, Worker,
};
use alien_error::AlienError;
use alien_gcp_clients::cloudrun::MockCloudRunApi;
use alien_gcp_clients::compute::{Subnetwork, SubnetworkPurpose, SubnetworkRole};
use rstest::rstest;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Services {
    live: Option<Service>,
    creates: usize,
    patches: usize,
    deletes: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    PendingDns,
    LostBackendResponse,
    LostNegResponse,
    LostBackendRetry,
    LostNegRetry,
    LegacyServiceHandoff,
    LegacyGraphHandoff,
}

// Reconstruct the previous controller's persisted graph ownership and provider
// descriptions from a real fixture graph, preserving every immutable child ID
// and reference. Legacy workers used HTTPS backends and native public ingress.
fn restore_legacy_https_graph(
    controller: &mut GcpWorkerController,
    cloud: &mut Provider,
    id: &str,
) {
    let mut child = |kind: &str, description: &str| {
        let (key, value) = cloud
            .objects
            .iter_mut()
            .find(|(key, _)| key.starts_with(&format!("{kind}/")))
            .unwrap();
        value["description"] = serde_json::json!(format!("{description} {id}"));
        if kind == "backend" {
            value["protocol"] = serde_json::json!("HTTPS");
        }
        key.split_once('/').unwrap().1.to_string()
    };
    controller.backend_service_name = Some(child("backend", "Backend service for worker"));
    controller.url_map_name = Some(child("url", "URL map for worker"));
    controller.target_https_proxy_name = Some(child("proxy", "HTTPS proxy for worker"));
    controller.global_address_name = Some(child("address", "Global IP for worker"));
    controller.forwarding_rule_name = Some(child("frontend", "Forwarding rule for worker"));
    controller.ssl_certificate_name = Some(child("certificate", "SSL certificate for worker"));
    controller.global_address_ip = controller.ingress.public.as_ref().unwrap().address.clone();
}

#[rstest]
#[case(EndpointAccess::Internet, EndpointAccess::Both, Fault::None)]
#[case(EndpointAccess::Private, EndpointAccess::Both, Fault::None)]
#[case(EndpointAccess::Both, EndpointAccess::Internet, Fault::None)]
#[case(EndpointAccess::Both, EndpointAccess::Private, Fault::None)]
#[case(EndpointAccess::Internet, EndpointAccess::Private, Fault::None)]
#[case(EndpointAccess::Private, EndpointAccess::Internet, Fault::None)]
#[case::private_dns_pending(EndpointAccess::Internet, EndpointAccess::Both, Fault::PendingDns)]
#[case::public_dns_pending(EndpointAccess::Private, EndpointAccess::Both, Fault::PendingDns)]
#[case::delete_after_lost_public_backend(
    EndpointAccess::Internet,
    EndpointAccess::Internet,
    Fault::LostBackendResponse
)]
#[case::delete_after_lost_private_backend(
    EndpointAccess::Private,
    EndpointAccess::Private,
    Fault::LostBackendResponse
)]
#[case::delete_after_lost_public_neg(
    EndpointAccess::Internet,
    EndpointAccess::Internet,
    Fault::LostNegResponse
)]
#[case::delete_after_lost_private_neg(
    EndpointAccess::Private,
    EndpointAccess::Private,
    Fault::LostNegResponse
)]
#[case::retry_public_backend(
    EndpointAccess::Internet,
    EndpointAccess::Internet,
    Fault::LostBackendRetry
)]
#[case::retry_private_backend(
    EndpointAccess::Private,
    EndpointAccess::Private,
    Fault::LostBackendRetry
)]
#[case::retry_public_neg(
    EndpointAccess::Internet,
    EndpointAccess::Internet,
    Fault::LostNegRetry
)]
#[case::retry_private_neg(EndpointAccess::Private, EndpointAccess::Private, Fault::LostNegRetry)]
#[case::legacy_public_service_adds_private(
    EndpointAccess::Internet,
    EndpointAccess::Both,
    Fault::LegacyServiceHandoff
)]
#[case::legacy_private_service_adds_public(
    EndpointAccess::Private,
    EndpointAccess::Both,
    Fault::LegacyServiceHandoff
)]
#[case::legacy_https_graph_adds_private(
    EndpointAccess::Internet,
    EndpointAccess::Both,
    Fault::LegacyGraphHandoff
)]
#[case::legacy_https_graph_becomes_private(
    EndpointAccess::Internet,
    EndpointAccess::Private,
    Fault::LegacyGraphHandoff
)]
#[tokio::test]
async fn access_changes_preserve_service_revision_and_retained_frontends(
    #[case] initial: EndpointAccess,
    #[case] desired: EndpointAccess,
    #[case] fault: Fault,
) {
    let pending_dns = fault == Fault::PendingDns;
    let fail_then_delete = matches!(fault, Fault::LostBackendResponse | Fault::LostNegResponse);
    let retry_lost_response = matches!(fault, Fault::LostBackendRetry | Fault::LostNegRetry);
    let expected_patches = usize::from(
        !fail_then_delete
            && (initial.internet_enabled() != desired.internet_enabled()
                || fault == Fault::LegacyGraphHandoff),
    );
    let services = Arc::new(Mutex::new(Services::default()));
    let mut run = MockCloudRunApi::new();
    let store = services.clone();
    run.expect_get_service().returning(move |_, _| {
        store.lock().unwrap().live.clone().ok_or_else(|| {
            AlienError::new(ClientErrorData::RemoteResourceNotFound {
                resource_type: "service".to_string(),
                resource_name: "api".to_string(),
            })
        })
    });
    let store = services.clone();
    run.expect_create_service()
        .times(1)
        .returning(move |_, name, request, _| {
            let mut store = store.lock().unwrap();
            assert!(store.live.is_none());
            let mut live = create_successful_service_response(&name);
            live.labels = request.labels;
            live.template = request.template;
            live.ingress = request.ingress;
            live.invoker_iam_disabled = request.invoker_iam_disabled;
            live.uid = Some("original-service".to_string());
            live.etag = Some("original-etag".to_string());
            store.live = Some(live);
            store.creates += 1;
            Ok(create_completed_operation_response("create"))
        });
    let store = services.clone();
    run.expect_patch_service()
        .times(expected_patches)
        .returning(move |_, _, request, mask, _, _| {
            assert_eq!(mask.as_deref(), Some("ingress,invokerIamDisabled,etag"));
            assert!(
                request.template.is_none(),
                "access changes must not restart a revision"
            );
            assert_eq!(request.etag.as_deref(), Some("original-etag"));
            let mut store = store.lock().unwrap();
            let live = store.live.as_mut().unwrap();
            live.ingress = request.ingress;
            live.invoker_iam_disabled = request.invoker_iam_disabled;
            store.patches += 1;
            Ok(create_completed_operation_response("update"))
        });
    let store = services.clone();
    run.expect_delete_service()
        .times(1)
        .returning(move |_, _, _, etag| {
            assert_eq!(etag.as_deref(), Some("original-etag"));
            let mut store = store.lock().unwrap();
            assert!(store.live.take().is_some());
            store.deletes += 1;
            Ok(create_completed_operation_response("delete"))
        });
    run.expect_get_operation()
        .returning(|_, _| Ok(create_completed_operation_response("done")));
    run.expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));
    run.expect_set_service_iam_policy()
        .returning(|_, _, policy| Ok(policy));

    let graphs = Arc::new(Mutex::new(Provider::default()));
    graphs.lock().unwrap().lose_backend_response =
        matches!(fault, Fault::LostBackendResponse | Fault::LostBackendRetry);
    graphs.lock().unwrap().deny_backend_recovery_reads = fault == Fault::LostBackendRetry;
    let negs = Arc::new(Mutex::new(NegCloud::default()));
    negs.lock().unwrap().deny_recovery_reads = fault == Fault::LostNegRetry;
    negs.lock().unwrap().lose_create =
        matches!(fault, Fault::LostNegResponse | Fault::LostNegRetry);
    let mut compute = provider(&graphs);
    mock_negs(&mut compute, negs.clone());
    let mut network = GcpNetworkController::mock_ready("default-network");
    network.network_self_link = Some(
        "https://www.googleapis.com/compute/v1/projects/test-project-123/global/networks/test-vpc"
            .to_string(),
    );
    network.subnetwork_self_link = Some("https://www.googleapis.com/compute/v1/projects/test-project-123/regions/us-central1/subnetworks/test-subnet".to_string());
    network.proxy_subnet_self_link = Some("https://www.googleapis.com/compute/v1/projects/test-project-123/regions/us-central1/subnetworks/test-proxy".to_string());
    network.proxy_subnet_cidr = Some("10.20.0.0/23".to_string());
    let network_link = network.network_self_link.clone();
    compute
        .expect_get_subnetwork()
        .returning(move |region, name| {
            assert_eq!(region, "us-central1");
            assert_eq!(name, "test-proxy");
            Ok(Subnetwork {
                purpose: Some(SubnetworkPurpose::RegionalManagedProxy),
                role: Some(SubnetworkRole::Active),
                network: network_link.clone(),
                ip_cidr_range: Some("10.20.0.0/23".to_string()),
                ..Default::default()
            })
        });
    let provider = setup_mock_service_provider(Arc::new(run), Some(Arc::new(compute)));
    let worker = function_public_ingress();
    let mut metadata = create_test_domain_metadata(&worker.id);
    let domain = metadata.resources.get_mut(&worker.id).unwrap();
    domain.private_endpoints.insert(
        "api".to_string(),
        ManagedDomainInfo {
            fqdn: "private-api.test.example.com".to_string(),
            certificate_id: domain.certificate_id.clone(),
            certificate_status: domain.certificate_status,
            dns_status: domain.dns_status,
            dns_error: None,
            certificate_chain: domain.certificate_chain.clone(),
            private_key: domain.private_key.clone(),
            issued_at: domain.issued_at.clone(),
        },
    );
    let mut controller = GcpWorkerController::default();
    let mut retained = Vec::new();
    let mut previous_worker = worker.clone();
    for (index, access) in [initial, desired].into_iter().enumerate() {
        if index == 1 {
            if matches!(
                fault,
                Fault::LegacyServiceHandoff | Fault::LegacyGraphHandoff
            ) {
                // Previous-version Cloud Run checkpoints and setup imports have
                // no create token. Preserve their actual service identity on upgrade.
                controller.service_owner = None;
                services
                    .lock()
                    .unwrap()
                    .live
                    .as_mut()
                    .unwrap()
                    .labels
                    .as_mut()
                    .unwrap()
                    .remove("alien-create-token");
                controller =
                    serde_json::from_value(serde_json::to_value(controller).unwrap()).unwrap();
            }
            if fault == Fault::LegacyGraphHandoff {
                restore_legacy_https_graph(
                    &mut controller,
                    &mut graphs.lock().unwrap(),
                    &worker.id,
                );
                let mut cloud = negs.lock().unwrap();
                assert_eq!(cloud.objects.len(), 1);
                cloud.objects.values_mut().next().unwrap().description =
                    Some(format!("Serverless NEG for worker {}", worker.id));
                services.lock().unwrap().live.as_mut().unwrap().ingress =
                    Some(CloudRunIngress::IngressTrafficAll);
                retained = graphs
                    .lock()
                    .unwrap()
                    .objects
                    .iter()
                    .filter(|(name, _)| name.starts_with("frontend/"))
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect();
                // Remove fields absent from the old release, rather than saving
                // a new owner alongside the old graph's ownership fields.
                let mut checkpoint = serde_json::to_value(controller).unwrap();
                for field in [
                    "ingress",
                    "ingressNegs",
                    "serviceOwner",
                    "nativePublicAccess",
                ] {
                    checkpoint.as_object_mut().unwrap().remove(field);
                }
                controller = serde_json::from_value(checkpoint).unwrap();
            }
            controller.state = GcpWorkerState::UpdateStart;
            if pending_dns {
                let domain = metadata.resources.get_mut(&worker.id).unwrap();
                if !initial.private_enabled() {
                    domain.private_endpoints.get_mut("api").unwrap().dns_status =
                        DnsRecordStatus::Pending;
                } else {
                    domain.dns_status = DnsRecordStatus::Pending;
                }
            }
        }
        let mut executor = SingleControllerExecutor::builder()
            .resource(worker.clone())
            .previous_resource(previous_worker.clone())
            .controller(controller)
            .platform(Platform::Gcp)
            .service_provider(provider.clone())
            .with_dependency(
                Network::new("default-network".to_string())
                    .settings(NetworkSettings::Create {
                        cidr: None,
                        availability_zones: 2,
                    })
                    .build(),
                network.clone(),
            )
            .stack_settings(StackSettings {
                endpoint_access: access,
                ..Default::default()
            })
            .domain_metadata(metadata.clone())
            .build()
            .await
            .unwrap();
        if index == 1 {
            executor.with_context(|ctx| {
                assert_eq!(
                    ctx.desired_resource_config::<Worker>().unwrap(),
                    ctx.previous_resource_config::<Worker>().unwrap(),
                    "access-only fixture must preserve the processed workload configuration"
                )
            });
        }
        if fail_then_delete || retry_lost_response {
            if retry_lost_response {
                let (mut stack, config, client_config) = executor.with_context(|ctx| {
                    (
                        ctx.desired_stack.clone(),
                        ctx.deployment_config.clone(),
                        ctx.client_config.clone(),
                    )
                });
                let mut state = executor.stack_state().clone();
                state.resources.get_mut(&worker.id).unwrap().config =
                    stack.resources[&worker.id].config.clone();
                state.resources.get_mut(&worker.id).unwrap().previous_config = None;
                // The generic fixture labels explicit dependencies Live. Runtime
                // receives this network from setup as Frozen.
                stack
                    .resources
                    .get_mut("default-network")
                    .unwrap()
                    .lifecycle = ResourceLifecycle::Frozen;
                state
                    .resources
                    .get_mut("default-network")
                    .unwrap()
                    .lifecycle = Some(ResourceLifecycle::Frozen);
                let runtime = StackExecutor::builder(&stack, client_config)
                    .deployment_config(&config)
                    .service_provider(provider.clone())
                    .step_running_resources(false)
                    .lifecycle_filter(vec![ResourceLifecycle::Live])
                    .step_out_of_scope_resources(false)
                    .build()
                    .unwrap();
                // After the committed create's response is lost, recovery reads
                // keep failing until the real executor exhausts its retry budget.
                // It owns terminal and post-mutation resume checkpoint storage.
                for _ in 0..200 {
                    state = runtime.step(state).await.unwrap().next_state;
                    if state.resources[&worker.id].status == ResourceStatus::ProvisionFailed {
                        break;
                    }
                }
                assert_eq!(
                    state.resources[&worker.id].status,
                    ResourceStatus::ProvisionFailed
                );
                assert!(state.resources[&worker.id].error.is_some());
                assert_eq!(state.resources[&worker.id].retry_attempt, 10);
                graphs.lock().unwrap().deny_backend_recovery_reads = false;
                negs.lock().unwrap().deny_recovery_reads = false;
                let failed: GcpWorkerController = state.resources[&worker.id]
                    .get_last_failed_controller()
                    .unwrap()
                    .unwrap()
                    .as_any()
                    .downcast_ref::<GcpWorkerController>()
                    .unwrap()
                    .clone();
                assert!(matches!(
                    failed.state,
                    GcpWorkerState::ReconcilingWorkerIngress
                ));
                state = serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
                assert_eq!(state.retry_failed().unwrap(), vec![worker.id.clone()]);
                let resumed = state.resources[&worker.id]
                    .get_internal_controller_typed::<GcpWorkerController>()
                    .unwrap();
                assert!(matches!(
                    resumed.state,
                    GcpWorkerState::ReconcilingWorkerIngress
                ));
                let plan = runtime.plan(&state).unwrap();
                assert!(plan.creates.is_empty());
                assert!(
                    plan.updates.is_empty(),
                    "retry unexpectedly planned updates: {:?}",
                    plan.updates
                );
                assert!(plan.replaces.is_empty());
                assert!(plan.deletes.is_empty());
                for _ in 0..200 {
                    if state.resources[&worker.id].status == ResourceStatus::Running {
                        break;
                    }
                    state = runtime.step(state).await.unwrap().next_state;
                }
                assert_eq!(state.resources[&worker.id].status, ResourceStatus::Running);
                assert!(state.resources[&worker.id].last_failed_state.is_none());
                controller = state.resources[&worker.id]
                    .get_internal_controller_typed::<GcpWorkerController>()
                    .unwrap();
                assert_eq!(
                    !controller.ingress.public_endpoints.is_empty(),
                    initial.internet_enabled()
                );
                assert_eq!(
                    !controller.ingress.private_endpoints.is_empty(),
                    initial.private_enabled()
                );
                assert_eq!(services.lock().unwrap().creates, 1);
                assert_eq!(services.lock().unwrap().patches, 0);
            } else {
                let mut failed = false;
                for _ in 0..200 {
                    if executor.step().await.is_err() {
                        failed = true;
                        break;
                    }
                }
                assert!(failed, "lost create response was not exercised");
                controller = serde_json::from_value(
                    serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap())
                        .unwrap(),
                )
                .unwrap();
                assert!(matches!(
                    controller.state,
                    GcpWorkerState::ReconcilingWorkerIngress
                ));
            }
            if matches!(fault, Fault::LostBackendResponse | Fault::LostBackendRetry) {
                assert!(!graphs.lock().unwrap().lose_backend_response);
                assert!(graphs
                    .lock()
                    .unwrap()
                    .objects
                    .keys()
                    .any(|name| name.starts_with("backend/")));
            } else {
                assert!(!negs.lock().unwrap().lose_create);
                assert_eq!(negs.lock().unwrap().objects.len(), 1);
            }
            break;
        }
        if index == 1 && pending_dns {
            let mut waiting = false;
            for _ in 0..200 {
                executor.step().await.unwrap();
                let saved = executor.internal_state::<GcpWorkerController>().unwrap();
                let prepared = if !initial.private_enabled() {
                    !saved.ingress.private_endpoints.is_empty()
                } else {
                    !saved.ingress.public_endpoints.is_empty()
                };
                if prepared {
                    waiting = true;
                    break;
                }
            }
            assert!(waiting, "replacement frontend was never published");
            assert_eq!(
                services.lock().unwrap().patches,
                0,
                "service ingress narrowed before DNS was ready"
            );
            for (name, frontend) in &retained {
                assert_eq!(graphs.lock().unwrap().objects.get(name), Some(frontend));
            }
            let domain = metadata.resources.get_mut(&worker.id).unwrap();
            domain.dns_status = DnsRecordStatus::Active;
            domain.private_endpoints.get_mut("api").unwrap().dns_status = DnsRecordStatus::Active;
            executor.set_domain_metadata(metadata.clone());
        }
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        executor.step().await.unwrap();
        assert!(
            matches!(
                executor
                    .internal_state::<GcpWorkerController>()
                    .unwrap()
                    .state,
                GcpWorkerState::Ready
            ),
            "heartbeat must not re-enter the retired certificate lifecycle"
        );
        assert!(!executor.needs_update().unwrap());
        previous_worker =
            executor.with_context(|ctx| ctx.desired_resource_config::<Worker>().unwrap().clone());
        controller = serde_json::from_value(
            serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            !controller.ingress.public_endpoints.is_empty(),
            access.internet_enabled()
        );
        assert_eq!(
            !controller.ingress.private_endpoints.is_empty(),
            access.private_enabled()
        );
        let live = services.lock().unwrap().live.clone().unwrap();
        assert_eq!(
            live.ingress,
            Some(if access.internet_enabled() {
                CloudRunIngress::IngressTrafficInternalLoadBalancer
            } else {
                CloudRunIngress::IngressTrafficInternalOnly
            })
        );
        assert_eq!(live.uid.as_deref(), Some("original-service"));
        let objects = &graphs.lock().unwrap().objects;
        if index == 0 {
            retained = objects
                .iter()
                .filter(|(name, _)| name.starts_with("frontend/"))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
        } else if initial.internet_enabled() && desired.internet_enabled()
            || initial.private_enabled() && desired.private_enabled()
        {
            for (name, frontend) in &retained {
                if name.contains("public-") && !desired.internet_enabled()
                    || name.contains("private-") && !desired.private_enabled()
                {
                    continue;
                }
                assert_eq!(
                    objects.get(name),
                    Some(frontend),
                    "retained frontend changed"
                );
            }
        }
    }
    assert_eq!(services.lock().unwrap().creates, 1);
    assert_eq!(services.lock().unwrap().patches, expected_patches);
    assert_eq!(negs.lock().unwrap().objects.len(), 1);
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Gcp)
        .service_provider(provider)
        .build()
        .await
        .unwrap();
    executor.delete().unwrap();
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);
    assert!(graphs.lock().unwrap().objects.is_empty());
    assert!(negs.lock().unwrap().objects.is_empty());
    assert!(services.lock().unwrap().live.is_none());
    assert_eq!(services.lock().unwrap().deletes, 1);
}

#[rstest]
#[case(CloudRunIngress::IngressTrafficAll, true, true)]
#[case(CloudRunIngress::IngressTrafficAll, false, false)]
#[case(CloudRunIngress::IngressTrafficInternalOnly, true, false)]
#[case(CloudRunIngress::IngressTrafficInternalLoadBalancer, true, false)]
#[tokio::test]
async fn legacy_native_outputs_refresh_from_actual_service_access(
    #[case] ingress: CloudRunIngress,
    #[case] invoker_disabled: bool,
    #[case] publicly_accessible: bool,
) {
    let mut worker = crate::worker::fixtures::basic_function();
    worker.id = "api".to_string();
    let mut checkpoint = serde_json::to_value(GcpWorkerController::mock_ready("test-api")).unwrap();
    checkpoint
        .as_object_mut()
        .unwrap()
        .remove("nativePublicAccess");
    let controller: GcpWorkerController = serde_json::from_value(checkpoint).unwrap();
    assert!(!controller.native_public_access);
    let mut run = MockCloudRunApi::new();
    run.expect_get_service().times(1).returning(move |_, name| {
        let mut service = create_successful_service_response(&name);
        service.ingress = Some(ingress.clone());
        service.invoker_iam_disabled = Some(invoker_disabled);
        Ok(service)
    });
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Gcp)
        .service_provider(setup_mock_service_provider(Arc::new(run), None))
        .with_test_dependencies()
        .build()
        .await
        .unwrap();
    executor.step().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
    let controller = executor.internal_state::<GcpWorkerController>().unwrap();
    let outputs = controller.build_outputs().unwrap();
    let outputs = outputs.downcast_ref::<alien_core::WorkerOutputs>().unwrap();
    assert_eq!(!outputs.public_endpoints.is_empty(), publicly_accessible);
    assert_eq!(!outputs.private_endpoints.is_empty(), !publicly_accessible);
    let binding: alien_core::bindings::WorkerBinding = serde_json::from_value(
        crate::core::ResourceController::get_binding_params(controller)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let alien_core::bindings::WorkerBinding::CloudRun(binding) = binding else {
        panic!("expected Cloud Run binding");
    };
    assert_eq!(binding.public_url.is_some(), publicly_accessible);
    assert_eq!(
        binding.private_url,
        alien_core::bindings::BindingValue::Value(
            "https://test-api-abcd1234-uc.a.run.app".to_string()
        )
    );
}

#[rstest]
#[case(GcpWorkerState::SettingIamPolicy)]
#[case(GcpWorkerState::DeletingService)]
#[case(GcpWorkerState::WaitingForServiceDeletion)]
#[case(GcpWorkerState::RetryingImagePull)]
#[case(GcpWorkerState::WaitingForServiceUpdate)]
#[case(GcpWorkerState::ReconcilingWorkerIngress)]
#[case(GcpWorkerState::UpdateStart)]
#[tokio::test]
async fn handed_off_legacy_service_refuses_replacement_at_resumed_handlers(
    #[case] resumed: GcpWorkerState,
) {
    let mut worker = crate::worker::fixtures::basic_function();
    worker.id = "api".to_string();
    let mut run = MockCloudRunApi::new();
    run.expect_get_service()
        .times(1)
        .returning(|_, name| Ok(create_successful_service_response(&name)));
    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(GcpWorkerController::mock_ready("test-api"))
        .platform(Platform::Gcp)
        .service_provider(setup_mock_service_provider(Arc::new(run), None))
        .build()
        .await
        .unwrap();
    executor.step().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
    let mut controller: GcpWorkerController = serde_json::from_value(
        serde_json::to_value(executor.internal_state::<GcpWorkerController>().unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        controller.service_owner.is_some(),
        "heartbeat did not checkpoint legacy UID"
    );
    controller.state = resumed;
    let mut worker = function_public_ingress();
    worker.id = "api".to_string();
    let metadata = create_test_domain_metadata(&worker.id);
    let mut run = MockCloudRunApi::new();
    run.expect_get_service().times(1).returning(|_, name| {
        let mut replacement = create_successful_service_response(&name);
        replacement.uid = Some("replacement-uid".to_string());
        Ok(replacement)
    });
    // No mutation or Compute API expectations: replacement must fail before
    // changing service IAM, revisions, exposure or deleting either service.
    let mut executor = SingleControllerExecutor::builder()
        .previous_resource(worker.clone())
        .resource(worker)
        .controller(controller)
        .platform(Platform::Gcp)
        .domain_metadata(metadata)
        .service_provider(setup_mock_service_provider(Arc::new(run), None))
        .build()
        .await
        .unwrap();
    let error = executor.step().await.unwrap_err();
    assert!(
        error.message.contains("replaced"),
        "unexpected error: {error}"
    );
}
