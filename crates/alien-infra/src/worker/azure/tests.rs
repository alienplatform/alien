//! # Azure Worker Controller Tests
//!
//! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

use std::sync::Arc;
use std::time::Duration;

use alien_azure_clients::models::container_apps::{
    Configuration, ConfigurationActiveRevisionsMode, ContainerApp, ContainerAppProperties,
    ContainerAppPropertiesProvisioningState, TrafficWeight,
};
use alien_azure_clients::{
    container_apps::MockContainerAppsApi,
    long_running_operation::{LongRunningOperation, MockLongRunningOperationApi, OperationResult},
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Platform, ResourceStatus, Worker, WorkerOutputs};
use alien_error::{AlienError, ContextError};
use httpmock::MockServer;
use rstest::rstest;

use super::{
    azure_storage_event_types, current_unix_timestamp_secs, dns_name_from_url,
    get_azure_storage_event_subscription_name, AZURE_RBAC_WAIT_POLL_SECS,
};
use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
use crate::error::ErrorData;
use crate::infra_requirements::azure_utils::is_azure_authorization_propagation_error;
use crate::worker::{
    fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock, AzureWorkerController,
};
use crate::AzureWorkerState;

#[test]
fn azure_storage_trigger_maps_only_supported_event_types() {
    assert_eq!(
        azure_storage_event_types(
            &[
                "created".to_string(),
                "deleted".to_string(),
                "tierChanged".to_string(),
            ],
            "worker",
        )
        .unwrap(),
        vec![
            "Microsoft.Storage.BlobCreated",
            "Microsoft.Storage.BlobDeleted",
            "Microsoft.Storage.BlobTierChanged",
        ]
    );
    assert!(azure_storage_event_types(&["metadataUpdated".to_string()], "worker").is_err());
}

#[test]
fn azure_storage_event_subscription_name_is_stable_and_within_limit() {
    let first = get_azure_storage_event_subscription_name(
        "worker-with-a-very-long-name-that-needs-truncating",
        "storage-with-a-very-long-name-that-needs-truncating",
    );
    let second = get_azure_storage_event_subscription_name(
        "worker-with-a-very-long-name-that-needs-truncating",
        "storage-with-a-very-long-name-that-needs-truncating",
    );
    assert_eq!(first, second);
    assert!(first.len() <= 64);
    assert!(first
        .chars()
        .all(|character| character.is_ascii_alphanumeric()));
}

#[test]
fn strips_scheme_and_path_from_dns_endpoint_url() {
    assert_eq!(
        dns_name_from_url("https://app.example.azurecontainerapps.io/health"),
        "app.example.azurecontainerapps.io"
    );
    assert_eq!(
        dns_name_from_url("app.example.azurecontainerapps.io."),
        "app.example.azurecontainerapps.io"
    );
}

#[test]
fn platform_domain_outputs_target_container_app_host_not_public_fqdn() {
    let mut controller = AzureWorkerController::mock_ready("test-worker");
    controller.fqdn = Some("test-worker.public.example.com".to_string());
    controller.certificate_id = Some("cert_123".to_string());
    controller.url = Some("https://test-worker.azurecontainerapps.io".to_string());

    let outputs = controller.build_outputs().unwrap();
    let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    let endpoint = worker_outputs
        .public_endpoints
        .get("default")
        .expect("default public endpoint");

    assert_eq!(
        endpoint.url.as_str(),
        "https://test-worker.azurecontainerapps.io"
    );
    assert_eq!(
        endpoint
            .load_balancer_endpoint
            .as_ref()
            .map(|endpoint| endpoint.dns_name.as_str()),
        Some("test-worker.azurecontainerapps.io")
    );
}

#[test]
fn dns_target_is_ingress_host_when_url_is_overridden_to_public_fqdn() {
    // Regression: when `url` is overridden to the public display FQDN (from `public_urls`), the
    // CNAME target must still be the Container App ingress host. Otherwise the record name (the
    // public FQDN) and the target collide into a self-referential CNAME, which the DNS provider
    // rejects — the bug that deadlocked the Azure worker in `waitingForDns`.
    let mut controller = AzureWorkerController::mock_ready("test-worker");
    controller.url = Some("https://test-worker.abc123.dev.vpc.direct".to_string());
    controller.container_app_url =
        Some("https://test-worker.kindsky.eastus2.azurecontainerapps.io".to_string());

    let outputs = controller.build_outputs().unwrap();
    let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    let endpoint = worker_outputs
        .public_endpoints
        .get("default")
        .expect("default public endpoint");

    // Display URL stays the public FQDN.
    assert_eq!(
        endpoint.url.as_str(),
        "https://test-worker.abc123.dev.vpc.direct"
    );
    // The CNAME target is the ingress host — and crucially NOT the record's own public FQDN.
    let dns_name = endpoint
        .load_balancer_endpoint
        .as_ref()
        .map(|endpoint| endpoint.dns_name.as_str());
    assert_eq!(
        dns_name,
        Some("test-worker.kindsky.eastus2.azurecontainerapps.io")
    );
    assert_ne!(dns_name, Some("test-worker.abc123.dev.vpc.direct"));
}

#[tokio::test]
async fn imported_worker_heartbeat_rebuilds_ingress_host_for_dns() {
    // Regression for the create-path-only gap: an imported worker starts Ready with
    // `container_app_url = None` and `url` = the public display FQDN (the importer skips the
    // create flow). The heartbeat must rebuild `container_app_url` from the live Container App,
    // so the DNS CNAME targets the ingress host rather than the self-referential public FQDN.
    let app_name = "test-imported-worker";
    let mut mock = MockContainerAppsApi::new();
    mock.expect_get_container_app()
        .returning(move |_, _| Ok(create_successful_container_app_response(app_name, true)))
        .times(0..);
    let mock_provider = setup_mock_service_provider(Arc::new(mock), None);

    // Imported shape: ingress host unset, url is the public display FQDN.
    let mut controller = AzureWorkerController::mock_ready(app_name);
    controller.container_app_url = None;
    controller.url = Some("https://test-imported-worker.abc123.dev.vpc.direct".to_string());

    let mut executor = SingleControllerExecutor::builder()
        .resource(basic_function())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    // The heartbeat rebuilt the ingress host…
    assert_eq!(
        controller.container_app_url.as_deref(),
        Some("https://test-imported-worker.azurecontainerapps.io")
    );
    // …so build_outputs targets it, NOT the public display FQDN.
    let outputs = controller.build_outputs().unwrap();
    let worker_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    let endpoint = worker_outputs
        .public_endpoints
        .get("default")
        .expect("default public endpoint");
    let dns_name = endpoint
        .load_balancer_endpoint
        .as_ref()
        .map(|endpoint| endpoint.dns_name.as_str());
    assert_eq!(dns_name, Some("test-imported-worker.azurecontainerapps.io"));
    assert_ne!(dns_name, Some("test-imported-worker.abc123.dev.vpc.direct"));
}

#[test]
fn detects_azure_authorization_propagation_error_from_http_context() {
    let http_error = AlienError::new(CloudClientErrorData::HttpResponseError {
        message: "Azure CreateOrUpdateDaprComponent failed: HTTP 403 Forbidden".to_string(),
        url: "https://management.azure.com/test".to_string(),
        http_status: 403,
        http_request_text: None,
        http_response_text: Some(
            "{\"error\":{\"code\":\"AuthorizationFailed\",\"message\":\"The client does not have authorization to perform action. If access was recently granted, please refresh your credentials.\"}}"
                .to_string(),
        ),
    });

    let error = http_error.context(ErrorData::CloudPlatformError {
        message: "Failed to create commands Dapr component".to_string(),
        resource_id: Some("alien-rs-fn".to_string()),
    });

    assert!(is_azure_authorization_propagation_error(&error));
}

#[test]
fn ignores_non_authorization_cloud_platform_errors() {
    let error = AlienError::new(ErrorData::CloudPlatformError {
        message: "Failed to create commands Dapr component".to_string(),
        resource_id: Some("alien-rs-fn".to_string()),
    });

    assert!(!is_azure_authorization_propagation_error(&error));
}

fn create_successful_container_app_response(app_name: &str, has_url: bool) -> ContainerApp {
    let fqdn = if has_url {
        Some(format!("{}.azurecontainerapps.io", app_name))
    } else {
        None
    };

    let ingress = if has_url {
        Some(alien_azure_clients::models::container_apps::Ingress {
            external: true,
            target_port: Some(8080),
            fqdn: fqdn.clone(),
            traffic: vec![alien_azure_clients::models::container_apps::TrafficWeight {
                latest_revision: true,
                weight: Some(100),
                revision_name: None,
                label: None,
            }],
            transport: alien_azure_clients::models::container_apps::IngressTransport::Auto,
            allow_insecure: false,
            additional_port_mappings: vec![],
            custom_domains: vec![],
            ip_security_restrictions: vec![],
            cors_policy: None,
            client_certificate_mode: None,
            exposed_port: None,
            sticky_sessions: None,
        })
    } else {
        None
    };

    ContainerApp {
        id: Some(format!(
            "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
            app_name
        )),
        name: Some(app_name.to_string()),
        location: "East US".to_string(),
        properties: Some(ContainerAppProperties {
            provisioning_state: Some(ContainerAppPropertiesProvisioningState::Succeeded),
            configuration: Some(Configuration {
                ingress,
                active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
                identity_settings: vec![],
                registries: vec![],
                secrets: vec![],
                dapr: None,
                max_inactive_revisions: None,
                runtime: None,
                service: None,
            }),
            outbound_ip_addresses: vec![],
            custom_domain_verification_id: None,
            environment_id: None,
            event_stream_endpoint: None,
            latest_ready_revision_name: None,
            latest_revision_fqdn: None,
            latest_revision_name: None,
            managed_environment_id: None,
            running_status: None,
            template: None,
            workload_profile_name: None,
        }),
        tags: std::collections::HashMap::new(),
        extended_location: None,
        identity: None,
        managed_by: None,
        system_data: None,
        type_: None,
    }
}

fn create_in_progress_container_app_response(app_name: &str) -> ContainerApp {
    ContainerApp {
        id: Some(format!(
            "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
            app_name
        )),
        name: Some(app_name.to_string()),
        location: "East US".to_string(),
        properties: Some(ContainerAppProperties {
            provisioning_state: Some(ContainerAppPropertiesProvisioningState::InProgress),
            outbound_ip_addresses: vec![],
            custom_domain_verification_id: None,
            environment_id: None,
            event_stream_endpoint: None,
            latest_ready_revision_name: None,
            latest_revision_fqdn: None,
            latest_revision_name: None,
            managed_environment_id: None,
            running_status: None,
            template: None,
            workload_profile_name: None,
            configuration: None,
        }),
        tags: std::collections::HashMap::new(),
        extended_location: None,
        identity: None,
        managed_by: None,
        system_data: None,
        type_: None,
    }
}

fn setup_mock_client_for_creation_and_update(
    app_name: &str,
    has_url: bool,
) -> Arc<MockContainerAppsApi> {
    let mut mock_container_apps = MockContainerAppsApi::new();

    // Mock successful app creation - immediate completion
    let app_name = app_name.to_string();
    let app_name_for_create = app_name.clone();
    mock_container_apps
        .expect_create_or_update_container_app()
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_create, has_url),
            ))
        });

    // Mock successful updates - immediate completion
    let app_name_for_update = app_name.clone();
    mock_container_apps
        .expect_update_container_app()
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_update, has_url),
            ))
        });

    // Mock get operations - may be called multiple times during creation and update flows
    let app_name_for_get = app_name.clone();
    mock_container_apps
        .expect_get_container_app()
        .returning(move |_, _| {
            Ok(create_successful_container_app_response(
                &app_name_for_get,
                has_url,
            ))
        })
        .times(0..); // Allow 0 or more calls

    Arc::new(mock_container_apps)
}

fn setup_mock_client_for_creation_and_deletion(
    app_name: &str,
    has_url: bool,
) -> Arc<MockContainerAppsApi> {
    let mut mock_container_apps = MockContainerAppsApi::new();

    // Mock successful app creation - immediate completion
    let app_name = app_name.to_string();
    let app_name_for_create = app_name.clone();
    mock_container_apps
        .expect_create_or_update_container_app()
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_create, has_url),
            ))
        });

    // Mock successful deletion - immediate completion
    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    // Mock get operations during creation (may be called multiple times)
    let app_name_for_get_creation = app_name.clone();
    mock_container_apps
        .expect_get_container_app()
        .returning(move |_, _| {
            Ok(create_successful_container_app_response(
                &app_name_for_get_creation,
                has_url,
            ))
        })
        .times(0..); // Allow 0 or more calls during creation

    // Mock get operation failure for deletion verification
    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        })
        .times(0..);

    Arc::new(mock_container_apps)
}

fn setup_mock_client_for_long_running_creation(
    app_name: &str,
    has_url: bool,
) -> (Arc<MockContainerAppsApi>, Arc<MockLongRunningOperationApi>) {
    let mut mock_container_apps = MockContainerAppsApi::new();
    let mut mock_lro = MockLongRunningOperationApi::new();

    // Mock creation that starts as long-running
    // Use minimal retry_after for fast tests (actual Azure would use seconds)
    let app_name = app_name.to_string();
    mock_container_apps
        .expect_create_or_update_container_app()
        .returning(|_, _, _| {
            Ok(OperationResult::LongRunning(LongRunningOperation {
                url: "https://management.azure.com/subscriptions/.../operations/test-op"
                    .to_string(),
                retry_after: Some(Duration::from_millis(10)),
                location_url: None,
            }))
        });

    // Mock LRO polling - first incomplete, then complete
    mock_lro
        .expect_check_status()
        .returning(|_, _, _| Ok(None)) // Still running
        .times(1);

    mock_lro
        .expect_check_status()
        .returning(|_, _, _| Ok(Some("completed".to_string()))) // Completed
        .times(1);

    // Mock get operations showing progression
    let app_name_for_get1 = app_name.clone();
    mock_container_apps
        .expect_get_container_app()
        .returning(move |_, _| {
            Ok(create_in_progress_container_app_response(
                &app_name_for_get1,
            ))
        })
        .times(1);

    let app_name_for_get2 = app_name.clone();
    mock_container_apps
        .expect_get_container_app()
        .returning(move |_, _| {
            Ok(create_successful_container_app_response(
                &app_name_for_get2,
                has_url,
            ))
        });

    (Arc::new(mock_container_apps), Arc::new(mock_lro))
}

fn setup_mock_client_for_best_effort_deletion(
    _app_name: &str,
    app_missing: bool,
) -> Arc<MockContainerAppsApi> {
    let mut mock_container_apps = MockContainerAppsApi::new();

    // Mock deletion (might fail if app missing)
    if app_missing {
        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            });
    } else {
        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));
    }

    // Always return not found for final status check
    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        });

    Arc::new(mock_container_apps)
}

fn setup_mock_service_provider(
    mock_container_apps: Arc<MockContainerAppsApi>,
    mock_lro: Option<Arc<MockLongRunningOperationApi>>,
) -> Arc<MockPlatformServiceProvider> {
    let mut mock_provider = MockPlatformServiceProvider::new();

    mock_provider
        .expect_get_azure_container_apps_client()
        .returning(move |_| Ok(mock_container_apps.clone()));

    if let Some(lro_client) = mock_lro {
        mock_provider
            .expect_get_azure_long_running_operation_client()
            .returning(move |_| Ok(lro_client.clone()));
    }

    // Mock Azure authorization client for resource-scoped permissions
    mock_provider
        .expect_get_azure_authorization_client()
        .returning(|_| {
            use alien_azure_clients::authorization::MockAuthorizationApi;
            let mut mock_auth = MockAuthorizationApi::new();
            mock_auth
                .expect_create_or_update_role_definition()
                .returning(|_, _, role_def| Ok(role_def.clone()));
            mock_auth
                .expect_build_role_assignment_id()
                .returning(|_, name| {
                    format!(
                        "/test/providers/Microsoft.Authorization/roleAssignments/{}",
                        name
                    )
                });
            mock_auth
                .expect_create_or_update_role_assignment_by_id()
                .returning(|_, role_assignment| Ok(role_assignment.clone()));
            mock_auth
                .expect_delete_role_assignment_by_id()
                .returning(|_| Ok(None));
            Ok(Arc::new(mock_auth))
        });

    Arc::new(mock_provider)
}

/// Sets up mock Container Apps client and optional readiness probe mock server
/// Returns (container_apps_mock_provider, optional_mock_server)
fn setup_mocks_for_function(
    worker: &Worker,
    app_name: &str,
    for_deletion: bool,
) -> (Arc<MockPlatformServiceProvider>, Option<MockServer>) {
    let has_url = !worker.public_endpoints.is_empty();
    let needs_readiness_probe = has_url && worker.readiness_probe.is_some();

    // Set up mock server for readiness probe if needed
    let mock_server = if needs_readiness_probe {
        Some(create_readiness_probe_mock(worker))
    } else {
        None
    };

    // Set up Container Apps client mock - create custom response if we need to override URL
    let container_apps_mock = if needs_readiness_probe && mock_server.is_some() {
        // Create custom mock that returns the mock server URL
        let mock_server_url = mock_server.as_ref().unwrap().base_url();
        setup_mock_client_with_custom_url(app_name, &mock_server_url, for_deletion)
    } else if for_deletion {
        setup_mock_client_for_creation_and_deletion(app_name, has_url)
    } else {
        setup_mock_client_for_creation_and_update(app_name, has_url)
    };

    let mock_provider = setup_mock_service_provider(container_apps_mock, None);

    (mock_provider, mock_server)
}

fn setup_mock_client_with_custom_url(
    app_name: &str,
    custom_url: &str,
    for_deletion: bool,
) -> Arc<MockContainerAppsApi> {
    let mut mock_container_apps = MockContainerAppsApi::new();

    // Create a container app response with custom URL
    let custom_response = create_container_app_with_custom_url(app_name, custom_url);

    // Mock successful app creation
    let response_for_create = custom_response.clone();
    mock_container_apps
        .expect_create_or_update_container_app()
        .returning(move |_, _, _| Ok(OperationResult::Completed(response_for_create.clone())));

    if for_deletion {
        // Mock successful deletion
        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        // Mock get operations during creation (may be called multiple times)
        let response_for_get_creation = custom_response.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| Ok(response_for_get_creation.clone()))
            .times(0..);

        // Mock get operation failure for deletion verification
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            })
            .times(0..);
    } else {
        // Mock successful updates
        let response_for_update = custom_response.clone();
        mock_container_apps
            .expect_update_container_app()
            .returning(move |_, _, _| Ok(OperationResult::Completed(response_for_update.clone())));

        // Mock get operations (may be called multiple times)
        let response_for_get = custom_response.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| Ok(response_for_get.clone()))
            .times(0..);
    }

    Arc::new(mock_container_apps)
}

fn create_container_app_with_custom_url(app_name: &str, custom_url: &str) -> ContainerApp {
    // For tests, just extract the host and port from the URL string
    let url_without_protocol = custom_url.strip_prefix("http://").unwrap_or(custom_url);
    let (host, _port) = if let Some(colon_pos) = url_without_protocol.find(':') {
        let host = &url_without_protocol[..colon_pos];
        let port_str = &url_without_protocol[colon_pos + 1..];
        let port = port_str.parse::<u16>().unwrap_or(80);
        (host, Some(port))
    } else {
        (url_without_protocol, None)
    };

    // Create FQDN that matches the custom URL
    let _fqdn = if let Some(port) = _port {
        format!("{}:{}", host, port)
    } else {
        host.to_string()
    };

    let ingress = Some(alien_azure_clients::models::container_apps::Ingress {
        external: true,
        target_port: Some(8080),
        fqdn: Some(custom_url.to_string()), // Use the full URL as FQDN for the test
        traffic: vec![alien_azure_clients::models::container_apps::TrafficWeight {
            latest_revision: true,
            weight: Some(100),
            revision_name: None,
            label: None,
        }],
        transport: alien_azure_clients::models::container_apps::IngressTransport::Auto,
        allow_insecure: false,
        additional_port_mappings: vec![],
        custom_domains: vec![],
        ip_security_restrictions: vec![],
        cors_policy: None,
        client_certificate_mode: None,
        exposed_port: None,
        sticky_sessions: None,
    });

    ContainerApp {
        id: Some(format!(
            "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
            app_name
        )),
        name: Some(app_name.to_string()),
        location: "East US".to_string(),
        properties: Some(ContainerAppProperties {
            provisioning_state: Some(ContainerAppPropertiesProvisioningState::Succeeded),
            configuration: Some(Configuration {
                ingress,
                active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
                identity_settings: vec![],
                registries: vec![],
                secrets: vec![],
                dapr: None,
                max_inactive_revisions: None,
                runtime: None,
                service: None,
            }),
            outbound_ip_addresses: vec![],
            custom_domain_verification_id: None,
            environment_id: None,
            event_stream_endpoint: None,
            latest_ready_revision_name: None,
            latest_revision_fqdn: None,
            latest_revision_name: None,
            managed_environment_id: None,
            running_status: None,
            template: None,
            workload_profile_name: None,
        }),
        tags: std::collections::HashMap::new(),
        extended_location: None,
        identity: None,
        managed_by: None,
        system_data: None,
        type_: None,
    }
}

async fn executor_for_wait_state(controller: AzureWorkerController) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .resource(basic_function())
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .with_test_dependencies()
        .build()
        .await
        .unwrap()
}

#[tokio::test]
async fn test_pre_container_app_rbac_wait_holds_state_when_woken_early() {
    let deadline = current_unix_timestamp_secs().saturating_add(60);
    let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
    controller.state = AzureWorkerState::WaitingBeforeContainerAppCreation;
    controller.pre_container_app_rbac_wait_until_epoch_secs = Some(deadline);

    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Provisioning);
    assert_eq!(
        controller.state,
        AzureWorkerState::WaitingBeforeContainerAppCreation
    );
    assert_eq!(
        step_result.suggested_delay,
        Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
    );
    assert_eq!(
        controller.pre_container_app_rbac_wait_until_epoch_secs,
        Some(deadline)
    );
}

#[tokio::test]
async fn test_ready_rbac_wait_holds_state_when_woken_early() {
    let deadline = current_unix_timestamp_secs().saturating_add(60);
    let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
    controller.state = AzureWorkerState::WaitingForRbacPropagation;
    controller.ready_rbac_wait_until_epoch_secs = Some(deadline);

    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Provisioning);
    assert_eq!(
        controller.state,
        AzureWorkerState::WaitingForRbacPropagation
    );
    assert_eq!(
        step_result.suggested_delay,
        Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
    );
    assert_eq!(controller.ready_rbac_wait_until_epoch_secs, Some(deadline));
}

#[tokio::test]
async fn test_ready_rbac_wait_advances_after_deadline() {
    let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
    controller.state = AzureWorkerState::WaitingForRbacPropagation;
    controller.ready_rbac_wait_until_epoch_secs =
        Some(current_unix_timestamp_secs().saturating_sub(1));

    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Provisioning);
    assert_eq!(controller.state, AzureWorkerState::RunningReadinessProbe);
    assert_eq!(step_result.suggested_delay, None);
    assert_eq!(controller.ready_rbac_wait_until_epoch_secs, None);

    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Running);
    assert_eq!(controller.state, AzureWorkerState::Ready);
    assert_eq!(step_result.suggested_delay, None);
}

#[tokio::test]
async fn test_update_rbac_wait_holds_and_clears() {
    let deadline = current_unix_timestamp_secs().saturating_add(60);
    let mut controller = AzureWorkerController::mock_ready("test-basic-worker");
    controller.state = AzureWorkerState::UpdateWaitingForRbacPropagation;
    controller.ready_rbac_wait_until_epoch_secs = Some(deadline);
    controller.update_rbac_wait_required = true;

    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Updating);
    assert_eq!(
        controller.state,
        AzureWorkerState::UpdateWaitingForRbacPropagation
    );
    assert_eq!(
        step_result.suggested_delay,
        Some(Duration::from_secs(AZURE_RBAC_WAIT_POLL_SECS))
    );
    assert_eq!(controller.ready_rbac_wait_until_epoch_secs, Some(deadline));
    assert!(controller.update_rbac_wait_required);

    let mut controller = controller.clone();
    controller.ready_rbac_wait_until_epoch_secs =
        Some(current_unix_timestamp_secs().saturating_sub(1));
    let mut executor = executor_for_wait_state(controller).await;
    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Updating);
    assert_eq!(
        controller.state,
        AzureWorkerState::UpdateRunningReadinessProbe
    );
    assert_eq!(step_result.suggested_delay, None);
    assert_eq!(controller.ready_rbac_wait_until_epoch_secs, None);
    assert!(!controller.update_rbac_wait_required);

    let step_result = executor.step().await.unwrap();
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();

    assert_eq!(executor.status(), ResourceStatus::Running);
    assert_eq!(controller.state, AzureWorkerState::Ready);
    assert_eq!(step_result.suggested_delay, None);
}

// ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

#[rstest]
#[case::basic(basic_function())]
#[case::env_vars(function_with_env_vars())]
#[case::storage_link(function_with_storage_link())]
#[case::env_and_storage(function_with_env_and_storage())]
#[case::multiple_storages(function_with_multiple_storages())]
#[case::public_ingress(function_public_ingress())]
#[case::private_ingress(function_private_ingress())]
#[case::concurrency(function_with_concurrency())]
#[case::custom_config(function_custom_config())]
#[case::readiness_probe(function_with_readiness_probe())]
#[case::complete_test(function_complete_test())]
#[tokio::test]
async fn test_create_and_delete_flow_succeeds(#[case] worker: Worker) {
    let app_name = format!("test-{}", worker.id);
    let (mock_provider, _mock_server) = setup_mocks_for_function(&worker, &app_name, true);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Run create flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify outputs are available
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    assert!(function_outputs.identifier.is_some());
    assert!(function_outputs.worker_name.starts_with("test-"));

    // Delete the worker
    executor.delete().unwrap();

    // Run delete flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);

    // Verify outputs are no longer available
    assert!(executor.outputs().is_none());
}

// ─────────────── UPDATE FLOW TESTS ────────────────────────────────

#[rstest]
#[case::basic_to_env(basic_function(), function_with_env_vars())]
#[case::env_to_storage(function_with_env_vars(), function_with_storage_link())]
#[case::storage_to_custom(function_with_storage_link(), function_custom_config())]
#[case::custom_to_public(function_custom_config(), function_public_ingress())]
#[case::public_to_complete(function_public_ingress(), function_complete_test())]
#[case::complete_to_basic(function_complete_test(), basic_function())]
#[tokio::test]
async fn test_update_flow_succeeds(#[case] from_function: Worker, #[case] to_function: Worker) {
    // Ensure both workers have the same ID for valid updates
    let worker_id = "test-update-worker".to_string();
    let mut from_function = from_function;
    from_function.id = worker_id.clone();

    let mut to_function = to_function;
    to_function.id = worker_id.clone();

    let app_name = format!("test-{}", worker_id);
    let (mock_provider, mock_server) = setup_mocks_for_function(&to_function, &app_name, false);

    // Start with the "from" worker in Ready state
    let mut ready_controller = AzureWorkerController::mock_ready(&app_name);

    // If the target worker has a readiness probe, update the controller URL to point to mock server
    if to_function.readiness_probe.is_some() && !to_function.public_endpoints.is_empty() {
        if let Some(ref server) = mock_server {
            ready_controller.url = Some(server.base_url());
        }
    } else if !to_function.public_endpoints.is_empty() {
        // Ensure the controller has a URL for public workers
        ready_controller.url = Some(format!("https://{}.azurecontainerapps.io", app_name));
    }

    let mut executor = SingleControllerExecutor::builder()
        .resource(from_function)
        .controller(ready_controller)
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Ensure we start in Running state
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Update to the new worker
    executor.update(to_function).unwrap();

    // Run the update flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

// ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

#[rstest]
#[case::basic(basic_function(), false)]
#[case::public_with_missing_app(function_public_ingress(), true)]
#[case::private_with_missing_app(function_private_ingress(), true)]
#[tokio::test]
async fn test_best_effort_deletion_when_resources_missing(
    #[case] worker: Worker,
    #[case] app_missing: bool,
) {
    let app_name = format!("test-{}", worker.id);
    let mock_container_apps = setup_mock_client_for_best_effort_deletion(&app_name, app_missing);
    let mock_provider = setup_mock_service_provider(mock_container_apps, None);

    // Start with a ready controller
    let mut ready_controller = AzureWorkerController::mock_ready(&app_name);
    if !worker.public_endpoints.is_empty() {
        ready_controller.url = Some(format!("https://{}.azurecontainerapps.io", app_name));
    }

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(ready_controller)
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Ensure we start in Running state
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Delete the worker
    executor.delete().unwrap();

    // Run the delete flow - it should succeed even when resources are missing
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);

    // Verify outputs are no longer available
    assert!(executor.outputs().is_none());
}

// ─────────────── LONG RUNNING OPERATION TESTS ──────────────────────

#[tokio::test]
async fn test_long_running_creation_operation() {
    let worker = basic_function();
    let app_name = format!("test-{}", worker.id);
    let (mock_container_apps, mock_lro) =
        setup_mock_client_for_long_running_creation(&app_name, false);
    let mock_provider = setup_mock_service_provider(mock_container_apps, Some(mock_lro));

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Run create flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify the controller went through LRO states
    let controller = executor.internal_state::<AzureWorkerController>().unwrap();
    assert!(controller.container_app_name.is_some());
    assert!(controller.resource_id.is_some());
}

// ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

/// Test that verifies public workers get URL in outputs
#[tokio::test]
async fn test_public_function_gets_url_in_outputs() {
    let worker = function_public_ingress();
    let app_name = format!("test-{}", worker.id);

    let mut mock_container_apps = MockContainerAppsApi::new();

    // Mock creation with URL
    mock_container_apps
        .expect_create_or_update_container_app()
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name, true),
            ))
        });

    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify URL is in outputs
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    let endpoint = function_outputs
        .public_endpoints
        .get("default")
        .expect("default public endpoint");
    assert!(endpoint.url.contains("azurecontainerapps.io"));
}

/// Test that verifies private workers don't get URL in outputs
#[tokio::test]
async fn test_private_function_has_no_url_in_outputs() {
    let worker = function_private_ingress();
    let app_name = format!("test-{}", worker.id);

    let mut mock_container_apps = MockContainerAppsApi::new();

    // Mock creation without URL
    mock_container_apps
        .expect_create_or_update_container_app()
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name, false),
            ))
        });

    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify no URL in outputs
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    assert!(function_outputs.public_endpoints.is_empty());
}

/// Test that verifies correct container app configuration parameters
#[tokio::test]
async fn test_container_app_configuration_validation() {
    let worker = function_custom_config();
    let app_name = format!("test-{}", worker.id);

    let mut mock_container_apps = MockContainerAppsApi::new();

    // Validate container app creation request has correct parameters
    let app_name_for_response = app_name.clone();
    mock_container_apps
        .expect_create_or_update_container_app()
        .withf(|_rg, _name, container_app| {
            // Check that the container has correct resource configuration
            if let Some(properties) = &container_app.properties {
                if let Some(template) = &properties.template {
                    if let Some(container) = template.containers.first() {
                        if let Some(resources) = &container.resources {
                            // function_custom_config has 512MB memory
                            let expected_memory = format!("{}Gi", 512.0 / 1024.0);
                            return resources.memory.as_ref() == Some(&expected_memory)
                                && resources.cpu == Some(0.25);
                        }
                    }
                }
            }
            false
        })
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_response, false),
            ))
        });

    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    // Allow get_container_app calls during creation (may be called 0 or more times)
    mock_container_apps
        .expect_get_container_app()
        .returning(move |_, _| Ok(create_successful_container_app_response(&app_name, false)))
        .times(0..);

    // Mock get operation failure for deletion verification
    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        })
        .times(0..);

    let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

/// Test that verifies environment variables are correctly passed
#[tokio::test]
async fn test_environment_variable_handling() {
    let worker = function_with_env_vars();
    let app_name = format!("test-{}", worker.id);

    let mut mock_container_apps = MockContainerAppsApi::new();

    // Validate container app creation request has environment variables
    let app_name_for_response = app_name.clone();
    mock_container_apps
        .expect_create_or_update_container_app()
        .withf(|_rg, _name, container_app| {
            if let Some(properties) = &container_app.properties {
                if let Some(template) = &properties.template {
                    if let Some(container) = template.containers.first() {
                        // Check that environment variables are present
                        let has_app_env = container.env.iter().any(|env_var| {
                            env_var.name.as_deref() == Some("APP_ENV")
                                && env_var.value.as_deref() == Some("production")
                        });
                        let has_log_level = container.env.iter().any(|env_var| {
                            env_var.name.as_deref() == Some("LOG_LEVEL")
                                && env_var.value.as_deref() == Some("debug")
                        });
                        let has_db_name = container.env.iter().any(|env_var| {
                            env_var.name.as_deref() == Some("DB_NAME")
                                && env_var.value.as_deref() == Some("myapp")
                        });
                        return has_app_env && has_log_level && has_db_name;
                    }
                }
            }
            false
        })
        .returning(move |_, _, _| {
            Ok(OperationResult::Completed(
                create_successful_container_app_response(&app_name_for_response, false),
            ))
        });

    mock_container_apps
        .expect_delete_container_app()
        .returning(|_, _| Ok(OperationResult::Completed(())));

    // Allow get_container_app calls during creation (may be called 0 or more times)
    mock_container_apps
        .expect_get_container_app()
        .returning(move |_, _| Ok(create_successful_container_app_response(&app_name, false)))
        .times(0..);

    // Mock get operation failure for deletion verification
    mock_container_apps
        .expect_get_container_app()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ContainerApp".to_string(),
                    resource_name: "test-app".to_string(),
                },
            ))
        })
        .times(0..);

    let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AzureWorkerController::default())
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

/// Test that verifies deletion works when container_app_name is not set (early creation failure)
#[tokio::test]
async fn test_delete_with_no_container_app_name_succeeds() {
    let worker = basic_function();

    // Create a controller with no container app name set (simulating early creation failure)
    let controller = AzureWorkerController {
        state: AzureWorkerState::CreateFailed,
        container_app_name: None, // This is the key - no container app name set
        resource_id: None,
        url: None,
        container_app_url: None,
        pending_operation_url: None,
        pending_operation_retry_after: None,
        dapr_components: Vec::new(),
        storage_trigger_infrastructure: Vec::new(),
        fqdn: None,
        certificate_id: None,
        keyvault_cert_id: None,
        container_apps_certificate_id: None,
        uses_custom_domain: false,
        certificate_issued_at: None,
        commands_namespace_name: None,
        commands_queue_name: None,
        commands_dapr_component: None,
        commands_sender_role_assignment_id: None,
        commands_receiver_role_assignment_id: None,
        commands_infrastructure_auth_wait_until_epoch_secs: None,
        container_apps_environment_wake_wait_until_epoch_secs: None,
        container_apps_environment_wake_retry_after_epoch_secs: None,
        pre_container_app_rbac_wait_until_epoch_secs: None,
        ready_rbac_wait_until_epoch_secs: None,
        update_rbac_wait_required: false,
        update_dapr_components_deleted: false,
        _internal_stay_count: None,
    };

    // Mock provider - no expectations since no API calls should be made
    let mock_provider = Arc::new(MockPlatformServiceProvider::new());

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Azure)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    // Start in CreateFailed state
    assert_eq!(executor.status(), ResourceStatus::ProvisionFailed);

    // Delete the worker
    executor.delete().unwrap();

    // Run the delete flow - should succeed without making any API calls
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);

    // Verify outputs are no longer available
    assert!(executor.outputs().is_none());
}
