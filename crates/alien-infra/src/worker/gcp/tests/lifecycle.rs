use super::*;

#[derive(Clone, Copy, Debug)]
enum LoadBalancerDeleteDependency {
    TargetProxy,
    ServerlessNeg,
}

#[rstest]
#[case::target_proxy(LoadBalancerDeleteDependency::TargetProxy)]
#[case::serverless_neg(LoadBalancerDeleteDependency::ServerlessNeg)]
#[tokio::test]
async fn test_delete_retries_while_load_balancer_dependency_drains(
    #[case] dependency: LoadBalancerDeleteDependency,
) {
    let worker = function_public_ingress();
    let function_name = format!("test-{}", worker.id);
    let delete_attempts = Arc::new(AtomicUsize::new(0));
    let mock_cloudrun = setup_mock_client_for_creation_and_deletion(&function_name, true);

    let mut mock_compute = MockComputeApi::new();
    mock_compute
        .expect_delete_global_forwarding_rule()
        .returning(|_| Ok(Operation::default()));
    match dependency {
        LoadBalancerDeleteDependency::TargetProxy => {
            let delete_attempts = Arc::clone(&delete_attempts);
            mock_compute
                .expect_delete_target_https_proxy()
                .returning(move |_| {
                    if delete_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(resource_in_use_error())
                    } else {
                        Ok(Operation::default())
                    }
                });
            mock_compute
                .expect_delete_region_network_endpoint_group()
                .returning(|_, _| Ok(Operation::default()));
        }
        LoadBalancerDeleteDependency::ServerlessNeg => {
            mock_compute
                .expect_delete_target_https_proxy()
                .returning(|_| Ok(Operation::default()));
            let delete_attempts = Arc::clone(&delete_attempts);
            mock_compute
                .expect_delete_region_network_endpoint_group()
                .returning(move |_, _| {
                    if delete_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(resource_in_use_error())
                    } else {
                        Ok(Operation::default())
                    }
                });
        }
    }
    mock_compute
        .expect_delete_url_map()
        .returning(|_| Ok(Operation::default()));
    mock_compute
        .expect_delete_backend_service()
        .returning(|_| Ok(Operation::default()));
    mock_compute
        .expect_delete_global_address()
        .returning(|_| Ok(Operation::default()));

    let mock_provider = setup_mock_service_provider(mock_cloudrun, Some(Arc::new(mock_compute)));
    let mut controller = GcpWorkerController::mock_ready(&function_name);
    controller.forwarding_rule_name = Some("test-fwd".to_string());
    controller.target_https_proxy_name = Some("test-proxy".to_string());
    controller.url_map_name = Some("test-url-map".to_string());
    controller.backend_service_name = Some("test-backend".to_string());
    controller.serverless_neg_name = Some("test-neg".to_string());
    controller.global_address_name = Some("test-address".to_string());
    controller.global_address_ip = Some("203.0.113.9".to_string());

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Gcp)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.delete().unwrap();
    executor.run_until_terminal().await.unwrap();

    assert_eq!(executor.status(), ResourceStatus::Deleted);
    assert_eq!(delete_attempts.load(Ordering::SeqCst), 2);
}

// ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

#[tokio::test]
async fn queue_push_subscription_uses_fully_qualified_topic_name() {
    use crate::queue::gcp::{GcpQueueController, GcpQueueState};

    let worker = function_with_queue_trigger();
    let function_name = format!("test-{}", worker.id);
    let mock_cloudrun = setup_mock_client_for_creation_and_update(&function_name, false);

    let mut mock_pubsub = MockPubSubApi::new();
    mock_pubsub
        .expect_create_subscription()
        .withf(|subscription_id, subscription| {
            subscription_id == "test-queue-func-test-queue"
                && subscription.topic.as_deref()
                    == Some("projects/test-project-123/topics/test-test-queue")
        })
        .times(1)
        .returning(|_, subscription| Ok(subscription));

    let mock_provider =
        setup_mock_service_provider_with_pubsub(mock_cloudrun, None, Arc::new(mock_pubsub));
    let queue_controller = GcpQueueController {
        state: GcpQueueState::Ready,
        topic_name: Some("test-test-queue".to_string()),
        subscription_name: Some("test-test-queue-sub".to_string()),
        _internal_stay_count: None,
    };

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(GcpWorkerController::default())
        .platform(Platform::Gcp)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .with_dependency(test_queue(), queue_controller)
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

/// Test that verifies public workers get IAM policy update
#[tokio::test]
async fn test_public_function_sets_iam_policy() {
    let worker = function_public_ingress();
    let function_name = format!("test-{}", worker.id);

    let mut mock_cloudrun = MockCloudRunApi::new();

    // Mock service creation
    let operation_name = format!("create-{}", function_name);
    let operation_name_for_get = operation_name.clone();
    mock_cloudrun
        .expect_create_service()
        .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

    mock_cloudrun
        .expect_get_operation()
        .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
        .times(1);

    let function_name_for_get = function_name.clone();
    mock_cloudrun
        .expect_get_service()
        .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
        .times(1);

    // Validate IAM policy operations are called for resource-scoped permissions
    mock_cloudrun
        .expect_get_service_iam_policy()
        .withf(|location, service_name| {
            location == "us-central1" && service_name.starts_with("test-")
        })
        .returning(|_, _| Ok(create_empty_iam_policy()));

    mock_cloudrun
        .expect_set_service_iam_policy()
        .withf(|location, service_name, _policy| {
            location == "us-central1" && service_name.starts_with("test-")
        })
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

    // Mock deletion
    let delete_operation_name = format!("delete-{}", function_name);
    let delete_operation_name_for_get = delete_operation_name.clone();
    mock_cloudrun
        .expect_delete_service()
        .returning(move |_, _, _, _| {
            Ok(create_successful_operation_response(&delete_operation_name))
        });

    mock_cloudrun.expect_get_operation().returning(move |_, _| {
        Ok(create_completed_operation_response(
            &delete_operation_name_for_get,
        ))
    });

    mock_cloudrun.expect_get_service().returning(|_, _| {
        Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Service".to_string(),
                resource_name: "test-service".to_string(),
            },
        ))
    });

    let compute_mock = create_ssl_compute_mock_for_creation_and_deletion();
    let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), Some(compute_mock));
    let domain_metadata = create_test_domain_metadata(&worker.id);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(GcpWorkerController::default())
        .platform(Platform::Gcp)
        .service_provider(mock_provider)
        .domain_metadata(domain_metadata)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify URL is in outputs
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    assert!(function_outputs.public_endpoints.contains_key("default"));
}

/// Test that verifies private workers handle resource-scoped permissions correctly
#[tokio::test]
async fn test_private_function_skips_iam_policy() {
    let worker = function_private_ingress();
    let function_name = format!("test-{}", worker.id);

    let mut mock_cloudrun = MockCloudRunApi::new();

    // Mock service creation
    let operation_name = format!("create-{}", function_name);
    let operation_name_for_get = operation_name.clone();
    mock_cloudrun
        .expect_create_service()
        .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

    mock_cloudrun
        .expect_get_operation()
        .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
        .times(1);

    let function_name_for_get = function_name.clone();
    mock_cloudrun
        .expect_get_service()
        .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
        .times(1);

    // IAM policy operations are now called for all workers (for resource-scoped permissions)
    mock_cloudrun
        .expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));

    mock_cloudrun
        .expect_set_service_iam_policy()
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

    // Mock deletion
    let delete_operation_name = format!("delete-{}", function_name);
    let delete_operation_name_for_get = delete_operation_name.clone();
    mock_cloudrun
        .expect_delete_service()
        .returning(move |_, _, _, _| {
            Ok(create_successful_operation_response(&delete_operation_name))
        });

    mock_cloudrun.expect_get_operation().returning(move |_, _| {
        Ok(create_completed_operation_response(
            &delete_operation_name_for_get,
        ))
    });

    mock_cloudrun.expect_get_service().returning(|_, _| {
        Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Service".to_string(),
                resource_name: "test-service".to_string(),
            },
        ))
    });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(GcpWorkerController::default())
        .platform(Platform::Gcp)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify URL is still available for private workers (internal access)
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    assert!(function_outputs.public_endpoints.contains_key("default"));
}

/// Test that verifies correct service configuration parameters
#[tokio::test]
async fn test_service_configuration_validation() {
    let worker = function_custom_config();
    let function_name = format!("test-{}", worker.id);

    let mut mock_cloudrun = MockCloudRunApi::new();

    // Validate service creation request has correct parameters
    let operation_name = format!("create-{}", function_name);
    let operation_name_for_get = operation_name.clone();
    mock_cloudrun
        .expect_create_service()
        .withf(|_, _, service, _| {
            // Check if the service has the expected configuration
            if let Some(template) = &service.template {
                let containers = &template.containers;
                if let Some(container) = containers.first() {
                    // Check memory configuration
                    if let Some(resources) = &container.resources {
                        if let Some(limits) = &resources.limits {
                            if let Some(memory) = limits.get("memory") {
                                return memory == "512Mi";
                            }
                        }
                    }
                }
            }
            false
        })
        .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

    mock_cloudrun
        .expect_get_operation()
        .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
        .times(1);

    let function_name_for_get = function_name.clone();
    mock_cloudrun
        .expect_get_service()
        .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
        .times(1);

    // Mock IAM policy operations for resource-scoped permissions
    mock_cloudrun
        .expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));

    mock_cloudrun
        .expect_set_service_iam_policy()
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

    // Mock deletion
    let delete_operation_name = format!("delete-{}", function_name);
    let delete_operation_name_for_get = delete_operation_name.clone();
    mock_cloudrun
        .expect_delete_service()
        .returning(move |_, _, _, _| {
            Ok(create_successful_operation_response(&delete_operation_name))
        });

    mock_cloudrun.expect_get_operation().returning(move |_, _| {
        Ok(create_completed_operation_response(
            &delete_operation_name_for_get,
        ))
    });

    mock_cloudrun.expect_get_service().returning(|_, _| {
        Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Service".to_string(),
                resource_name: "test-service".to_string(),
            },
        ))
    });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(GcpWorkerController::default())
        .platform(Platform::Gcp)
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
    let function_name = format!("test-{}", worker.id);

    let mut mock_cloudrun = MockCloudRunApi::new();

    // Validate service creation request has environment variables
    let operation_name = format!("create-{}", function_name);
    let operation_name_for_get = operation_name.clone();
    mock_cloudrun
        .expect_create_service()
        .withf(|_, _, service, _| {
            if let Some(template) = &service.template {
                let containers = &template.containers;
                if let Some(container) = containers.first() {
                    // Check environment variables
                    let env_vars: HashMap<String, String> = container
                        .env
                        .iter()
                        .filter_map(|env| env.value.as_ref().map(|v| (env.name.clone(), v.clone())))
                        .collect();

                    return env_vars.get("APP_ENV") == Some(&"production".to_string())
                        && env_vars.get("LOG_LEVEL") == Some(&"debug".to_string())
                        && env_vars.get("DB_NAME") == Some(&"myapp".to_string());
                }
            }
            false
        })
        .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

    mock_cloudrun
        .expect_get_operation()
        .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
        .times(1);

    let function_name_for_get = function_name.clone();
    mock_cloudrun
        .expect_get_service()
        .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
        .times(1);

    // Mock IAM policy operations for resource-scoped permissions
    mock_cloudrun
        .expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));

    mock_cloudrun
        .expect_set_service_iam_policy()
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

    // Mock deletion
    let delete_operation_name = format!("delete-{}", function_name);
    let delete_operation_name_for_get = delete_operation_name.clone();
    mock_cloudrun
        .expect_delete_service()
        .returning(move |_, _, _, _| {
            Ok(create_successful_operation_response(&delete_operation_name))
        });

    mock_cloudrun.expect_get_operation().returning(move |_, _| {
        Ok(create_completed_operation_response(
            &delete_operation_name_for_get,
        ))
    });

    mock_cloudrun.expect_get_service().returning(|_, _| {
        Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Service".to_string(),
                resource_name: "test-service".to_string(),
            },
        ))
    });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_cloudrun), None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(GcpWorkerController::default())
        .platform(Platform::Gcp)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

/// Test that verifies deletion works when service_name is not set (early creation failure)
#[tokio::test]
async fn test_delete_with_no_service_name_succeeds() {
    let worker = basic_function();

    // Create a controller with no service name set (simulating early creation failure)
    let controller = GcpWorkerController {
        state: GcpWorkerState::CreateFailed,
        service_name: None, // This is the key - no service name set
        url: None,
        operation_name: None,
        image_pull_permission_retries: 0,
        compute_operation_name: None,
        compute_operation_region: None,
        push_subscriptions: Vec::new(),
        fqdn: None,
        certificate_id: None,
        ssl_certificate_name: None,
        uses_custom_domain: false,
        certificate_issued_at: None,
        serverless_neg_name: None,
        backend_service_name: None,
        url_map_name: None,
        target_https_proxy_name: None,
        global_address_name: None,
        global_address_ip: None,
        forwarding_rule_name: None,
        commands_topic_name: None,
        commands_subscription_name: None,
        storage_notification_topics: Vec::new(),
        gcs_notification_ids: Vec::new(),
        scheduler_job_names: Vec::new(),
        project_id: None,
        region: None,
        _internal_stay_count: None,
    };

    // Mock provider - no expectations since no API calls should be made
    let mock_provider = Arc::new(MockPlatformServiceProvider::new());

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(controller)
        .platform(Platform::Gcp)
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
