//! Delete failures retain the provider identifiers required for explicit retry.
use alien_azure_clients::azure::service_bus::MockServiceBusManagementApi;
use alien_core::{
    AzureClientConfig, AzureCredentials, AzureResourceGroup, AzureResourceGroupOutputs,
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
    Queue, Resource, ResourceLifecycle, ResourceOutputs, ResourceStatus, StackResourceState,
    StackSettings, StackState,
};
use alien_error::AlienError;
use alien_infra::{
    deserialize_controller, MockPlatformServiceProvider, StackExecutor, StackResourceStateExt,
};
use serde_json::json;
use std::sync::Arc;
#[path = "../src/core/aws_tag_scoped.rs"]
mod aws_tag_scoped;

#[tokio::test]
async fn denied_delete_and_denied_probe_preserve_the_error() {
    let remote_exists = true;
    let denied = || {
        AlienError::new(alien_client_core::ErrorData::RemoteAccessDenied {
            resource_type: "certificate".into(),
            resource_name: "synthetic-certificate".into(),
        })
    };
    let result = aws_tag_scoped::delete_tag_scoped::<(), _, _>(Err(denied()), || async {
        assert!(remote_exists);
        Err(denied())
    })
    .await;
    assert!(remote_exists);
    assert_eq!(result.unwrap_err().code, "REMOTE_ACCESS_DENIED");
}

async fn assert_azure_delete_failure(status: u16) {
    let mut client = MockServiceBusManagementApi::new();
    client
        .expect_delete_queue()
        .times(1)
        .returning(move |rg, ns, q| {
            assert_eq!(
                (rg.as_str(), ns.as_str(), q.as_str()),
                ("synthetic-rg", "synthetic-ns", "synthetic-jobs")
            );
            let mut error = AlienError::new(alien_azure_clients::ErrorData::RemoteAccessDenied {
                resource_type: "queue".into(),
                resource_name: "synthetic-jobs".into(),
            });
            error.http_status_code = Some(status);
            error.code = format!("SYNTHETIC_HTTP_{status}");
            error.retryable = false;
            Err(error)
        });
    let client = Arc::new(client);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_service_bus_management_client()
        .returning(move |_| Ok(client.clone()));
    let cfg = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: "synthetic-subscription".into(),
        tenant_id: "synthetic-tenant".into(),
        region: Some("westus2".into()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: "synthetic".into(),
            client_secret: "synthetic".into(),
        },
        service_overrides: None,
    }));
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let mut state = StackState::new(Platform::Azure);
    let mut group = StackResourceState::new_pending(
        "azure_resource_group".into(),
        Resource::new(AzureResourceGroup::new("default-resource-group".into()).build()),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    group.status = ResourceStatus::Running;
    group.outputs = Some(ResourceOutputs::new(AzureResourceGroupOutputs {
        name: "synthetic-rg".into(),
        resource_id: "synthetic-resource-group".into(),
        location: "westus2".into(),
    }));
    state
        .resources
        .insert("default-resource-group".into(), group);
    let mut queue = StackResourceState::new_pending(
        "queue".into(),
        Resource::new(Queue::new("jobs".into()).build()),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    queue.status = ResourceStatus::Running;
    queue.set_internal_controller(Some(deserialize_controller(json!({"type":"AzureQueueController","_controllerStateVersion":1,"state":"ready","namespaceName":"synthetic-ns","queueName":"synthetic-jobs"})).unwrap())).unwrap();
    state.resources.insert("jobs".into(), queue);
    let executor = StackExecutor::for_deletion_with_service_provider(
        cfg,
        &config,
        Arc::new(provider),
        Some(vec![ResourceLifecycle::Live]),
    )
    .unwrap();
    for _ in 0..12 {
        state = executor.step(state).await.unwrap().next_state;
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        if matches!(
            state.resources["jobs"].status,
            ResourceStatus::Deleted | ResourceStatus::DeleteFailed
        ) {
            break;
        }
    }
    let queue = &state.resources["jobs"];
    println!(
        "Azure HTTP {status}: {:?}, IDs present={}",
        queue.status,
        queue.internal_state.is_some()
    );
    assert_eq!(queue.status, ResourceStatus::DeleteFailed);
    assert_eq!(
        queue.internal_state.as_ref().unwrap()["queueName"],
        "synthetic-jobs"
    );
    assert_eq!(
        queue.last_failed_state.as_ref().unwrap()["namespaceName"],
        "synthetic-ns"
    );
    assert!(queue.error.is_some());
}
#[tokio::test]
async fn azure_denied_delete_preserves_ids() {
    assert_azure_delete_failure(403).await;
}
#[tokio::test]
async fn azure_unknown_delete_failure_preserves_ids() {
    assert_azure_delete_failure(500).await;
}

#[tokio::test]
async fn failed_gcp_delete_operation_retries_after_saved_checkpoint() {
    use alien_core::{GcpClientConfig, GcpCredentials, Network};
    use alien_gcp_clients::compute::{MockComputeApi, Operation};
    let mut compute = MockComputeApi::new();
    compute.expect_get_global_operation().withf(|name| name == "failed-operation").times(1)
        .returning(|_| Ok(serde_json::from_value::<Operation>(json!({"name":"failed-operation","status":"DONE","httpErrorStatusCode":403,"error":{"errors":[{"code":"PERMISSION_DENIED","message":"synthetic denial"}]}})).unwrap()));
    compute
        .expect_delete_network()
        .withf(|name| name == "synthetic-network")
        .times(1)
        .returning(|_| {
            Ok(serde_json::from_value::<Operation>(
                json!({"name":"fresh-operation","status":"PENDING"}),
            )
            .unwrap())
        });
    compute
        .expect_get_global_operation()
        .withf(|name| name == "fresh-operation")
        .times(1)
        .returning(|_| {
            Ok(serde_json::from_value::<Operation>(
                json!({"name":"fresh-operation","status":"DONE"}),
            )
            .unwrap())
        });
    let compute = Arc::new(compute);
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_gcp_compute_client()
        .returning(move |_| Ok(compute.clone()));
    let cfg = ClientConfig::Gcp(Box::new(GcpClientConfig {
        project_id: "synthetic-project".into(),
        region: "us-central1".into(),
        project_number: Some("123456789012".into()),
        credentials: GcpCredentials::AccessToken {
            token: "synthetic".into(),
        },
        service_overrides: None,
    }));
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let mut state = StackState::new(Platform::Gcp);
    let mut resource = StackResourceState::new_pending(
        "network".into(),
        Resource::new(
            Network::new("network".into())
                .settings(alien_core::NetworkSettings::UseDefault)
                .build(),
        ),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    resource.status = ResourceStatus::Deleting;
    resource.set_internal_controller(Some(deserialize_controller(json!({"type":"GcpNetworkController","_controllerStateVersion":1,"state":"waitingForNetworkDeletion","networkName":"synthetic-network","pendingOperationName":"failed-operation","isByoVpc":false})).unwrap())).unwrap();
    state.resources.insert("network".into(), resource);
    let executor = StackExecutor::for_deletion_with_service_provider(
        cfg,
        &config,
        Arc::new(services),
        Some(vec![ResourceLifecycle::Live]),
    )
    .unwrap();
    state = executor.step(state).await.unwrap().next_state;
    assert_eq!(
        state.resources["network"].status,
        ResourceStatus::DeleteFailed
    );
    assert_eq!(
        state.resources["network"]
            .last_failed_state
            .as_ref()
            .unwrap()["networkName"],
        "synthetic-network"
    );
    state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    use alien_infra::StackStateExt;
    state.retry_failed().unwrap();
    for _ in 0..8 {
        state = executor.step(state).await.unwrap().next_state;
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        if state.resources["network"].status == ResourceStatus::Deleted {
            break;
        }
    }
    assert_eq!(state.resources["network"].status, ResourceStatus::Deleted);
}

#[tokio::test]
async fn denied_gcp_management_policy_read_preserves_cleanup_checkpoint() {
    use alien_core::{GcpClientConfig, GcpCredentials, RemoteStackManagement};
    use alien_gcp_clients::resource_manager::MockResourceManagerApi;
    let mut manager = MockResourceManagerApi::new();
    manager
        .expect_get_project_iam_policy()
        .times(1)
        .returning(|_, _| {
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteAccessDenied {
                    resource_type: "project".into(),
                    resource_name: "synthetic-project".into(),
                },
            ))
        });
    let manager = Arc::new(manager);
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_gcp_resource_manager_client()
        .returning(move |_| Ok(manager.clone()));
    let cfg = ClientConfig::Gcp(Box::new(GcpClientConfig {
        project_id: "synthetic-project".into(),
        region: "us-central1".into(),
        project_number: Some("123456789012".into()),
        credentials: GcpCredentials::AccessToken {
            token: "synthetic".into(),
        },
        service_overrides: None,
    }));
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let mut resource = StackResourceState::new_pending(
        "remote-stack-management".into(),
        Resource::new(RemoteStackManagement::new("management".into()).build()),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    resource.status = ResourceStatus::Deleting;
    resource.set_internal_controller(Some(deserialize_controller(json!({"type":"GcpRemoteStackManagementController","_controllerStateVersion":1,"state":"deleteStart","serviceAccountEmail":"synthetic-management@synthetic-project.iam.gserviceaccount.com","roleBound":true,"impersonationGranted":true})).unwrap())).unwrap();
    let mut state = StackState::new(Platform::Gcp);
    state.resources.insert("management".into(), resource);
    let executor = StackExecutor::for_deletion_with_service_provider(
        cfg,
        &config,
        Arc::new(services),
        Some(vec![ResourceLifecycle::Frozen]),
    )
    .unwrap();
    state = executor.step(state).await.unwrap().next_state;
    state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    let resource = &state.resources["management"];
    assert_eq!(resource.status, ResourceStatus::DeleteFailed);
    assert!(resource.error.is_some());
    assert_eq!(
        resource.last_failed_state.as_ref().unwrap()["roleBound"],
        true
    );
    assert_eq!(
        resource.last_failed_state.as_ref().unwrap()["federationGranted"],
        false,
        "legacy impersonation must not imply a federation grant"
    );
    assert_eq!(
        resource.last_failed_state.as_ref().unwrap()["serviceAccountEmail"],
        "synthetic-management@synthetic-project.iam.gserviceaccount.com"
    );
}

#[tokio::test]
async fn expired_cloud_run_delete_operation_reissues_delete_after_reload_and_retry() {
    use alien_core::{GcpClientConfig, GcpCredentials, Worker, WorkerCode};
    use alien_gcp_clients::{
        cloudrun::{MockCloudRunApi, Service},
        longrunning::Operation,
    };
    use alien_infra::StackStateExt;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let exists = Arc::new(AtomicBool::new(true));
    let deletes = Arc::new(AtomicUsize::new(0));
    let mut cloudrun = MockCloudRunApi::new();
    cloudrun.expect_get_operation().returning(|_, operation| {
        if operation == "expired-operation" {
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "operation".into(),
                    resource_name: operation,
                },
            ))
        } else {
            assert_eq!(operation, "fresh-operation");
            Ok(Operation::builder().name(operation).done(true).build())
        }
    });
    let observed = exists.clone();
    cloudrun.expect_get_service().returning(move |_, name| {
        if observed.load(Ordering::SeqCst) {
            Ok(Service::builder().name(name).build())
        } else {
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "service".into(),
                    resource_name: name,
                },
            ))
        }
    });
    let deleted = exists.clone();
    let attempts = deletes.clone();
    cloudrun
        .expect_delete_service()
        .returning(move |_, name, _, _| {
            assert_eq!(name, "synthetic-service");
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                let mut error = AlienError::new(alien_client_core::ErrorData::RemoteAccessDenied {
                    resource_type: "service".into(),
                    resource_name: name,
                });
                error.retryable = false;
                return Err(error);
            }
            deleted.store(false, Ordering::SeqCst);
            Ok(Operation::builder()
                .name("fresh-operation".to_string())
                .done(false)
                .build())
        });
    let cloudrun = Arc::new(cloudrun);
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_gcp_cloudrun_client()
        .returning(move |_| Ok(cloudrun.clone()));
    let cfg = ClientConfig::Gcp(Box::new(GcpClientConfig {
        project_id: "synthetic-project".into(),
        region: "us-central1".into(),
        project_number: Some("123456789012".into()),
        credentials: GcpCredentials::AccessToken {
            token: "synthetic".into(),
        },
        service_overrides: None,
    }));
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let mut resource = StackResourceState::new_pending(
        "worker".into(),
        Resource::new(
            Worker::new("worker".into())
                .code(WorkerCode::Image {
                    image: "example.invalid/worker:latest".into(),
                })
                .permissions("synthetic-profile".into())
                .build(),
        ),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    resource.status = ResourceStatus::Deleting;
    resource
        .set_internal_controller(Some(
            deserialize_controller(json!({
                "type":"GcpWorkerController", "_controllerStateVersion":1,
                "state":"waitingForDeleteOperation", "serviceName":"synthetic-service",
                "operationName":"expired-operation", "pushSubscriptions":[],
                "storageNotificationTopics":[], "gcsNotificationIds":[], "schedulerJobNames":[],
                "usesCustomDomain":false
            }))
            .unwrap(),
        ))
        .unwrap();
    let mut state = StackState::new(Platform::Gcp);
    state.resources.insert("worker".into(), resource);
    let executor = StackExecutor::for_deletion_with_service_provider(
        cfg,
        &config,
        Arc::new(services),
        Some(vec![ResourceLifecycle::Live]),
    )
    .unwrap();
    for _ in 0..30 {
        state = executor.step(state).await.unwrap().next_state;
        if state.resources["worker"].status == ResourceStatus::DeleteFailed {
            break;
        }
    }
    assert_eq!(
        deletes.load(Ordering::SeqCst),
        1,
        "an expired operation must reissue deletion, not poll the existing service forever"
    );
    assert!(exists.load(Ordering::SeqCst));
    assert_eq!(
        state.resources["worker"].status,
        ResourceStatus::DeleteFailed
    );
    assert_eq!(
        state.resources["worker"]
            .last_failed_state
            .as_ref()
            .unwrap()["serviceName"],
        "synthetic-service"
    );
    let disk = tempfile::tempdir().unwrap();
    let checkpoint = disk.path().join("checkpoint.json");
    std::fs::write(&checkpoint, serde_json::to_vec(&state).unwrap()).unwrap();
    state = serde_json::from_slice(&std::fs::read(&checkpoint).unwrap()).unwrap();
    state = executor.step(state).await.unwrap().next_state;
    assert_eq!(
        deletes.load(Ordering::SeqCst),
        1,
        "saved denial requires explicit retry"
    );
    state.retry_failed().unwrap();
    for _ in 0..8 {
        state = executor.step(state).await.unwrap().next_state;
        if state.resources["worker"].status == ResourceStatus::Deleted {
            break;
        }
    }
    assert_eq!(state.resources["worker"].status, ResourceStatus::Deleted);
    assert_eq!(deletes.load(Ordering::SeqCst), 2);
    assert!(!exists.load(Ordering::SeqCst));
}
