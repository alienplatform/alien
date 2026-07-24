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

    expect_dapr_components_missing(&mut mock_container_apps);
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

    expect_dapr_components_missing(&mut mock_container_apps);
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

fn setup_commands_toggle_provider(
    container_apps: Arc<MockContainerAppsApi>,
    service_bus: Arc<MockServiceBusManagementApi>,
    role_assignment_created: Option<Arc<AtomicBool>>,
) -> Arc<MockPlatformServiceProvider> {
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_container_apps_client()
        .returning(move |_| Ok(container_apps.clone()));
    provider
        .expect_get_azure_service_bus_management_client()
        .returning(move |_| Ok(service_bus.clone()));
    provider
        .expect_get_azure_caller_principal_id()
        .returning(|_| Ok("test-manager-principal".to_string()));
    provider
        .expect_get_azure_authorization_client()
        .returning(move |_| {
            let mut authorization = MockAuthorizationApi::new();
            authorization
                .expect_create_or_update_role_definition()
                .returning(|_, _, role_definition| Ok(role_definition.clone()));
            authorization
                .expect_build_role_assignment_id()
                .returning(|_, name| {
                    format!("/test/providers/Microsoft.Authorization/roleAssignments/{name}")
                });
            authorization
                .expect_list_role_assignments()
                .returning(|_, _| Ok(Vec::new()));
            let role_assignment_created = role_assignment_created.clone();
            authorization
                .expect_create_or_update_role_assignment_by_id()
                .returning(move |_, role_assignment| {
                    if let Some(created) = &role_assignment_created {
                        created.store(true, Ordering::SeqCst);
                    }
                    Ok(role_assignment.clone())
                });
            authorization
                .expect_delete_role_assignment_by_id()
                .returning(|_| Ok(None));
            Ok(Arc::new(authorization))
        });
    Arc::new(provider)
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
        expect_dapr_components_missing(&mut mock_container_apps);
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

fn expect_dapr_components_missing(mock_container_apps: &mut MockContainerAppsApi) {
    mock_container_apps
        .expect_get_dapr_component()
        .returning(|_, _, component_name| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Dapr component".to_string(),
                    resource_name: component_name.to_string(),
                },
            ))
        })
        .times(0..);
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
