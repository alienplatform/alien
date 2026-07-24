use super::*;

#[tokio::test]
async fn test_worker_configuration_validation() {
    let worker = function_custom_config();
    let worker_name = format!("test-{}", worker.id);

    let mut mock_lambda = MockLambdaApi::new();

    let worker_name_for_create = worker_name.clone();
    mock_lambda
        .expect_create_function()
        .withf(|request| {
            request.memory_size == Some(512)
                && request.timeout == Some(120)
                && request.package_type == "Image"
                && request
                    .architectures
                    .as_ref()
                    .map(|a| a.contains(&"arm64".to_string()))
                    .unwrap_or(false)
        })
        .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

    let worker_name_for_get = worker_name.clone();
    mock_lambda
        .expect_get_function_configuration()
        .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
        .times(1);

    mock_lambda
        .expect_delete_function()
        .returning(|_, _| Ok(()));
    mock_lambda
        .expect_get_function_configuration()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Worker".to_string(),
                    resource_name: "test-worker".to_string(),
                },
            ))
        });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AwsWorkerController::default())
        .platform(Platform::Aws)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}

#[tokio::test]
async fn test_environment_variable_handling() {
    let worker = function_with_env_vars();
    let worker_name = format!("test-{}", worker.id);

    let mut mock_lambda = MockLambdaApi::new();

    let worker_name_for_create = worker_name.clone();
    mock_lambda
        .expect_create_function()
        .withf(|request| {
            if let Some(env) = &request.environment {
                if let Some(vars) = &env.variables {
                    vars.get("APP_ENV") == Some(&"production".to_string())
                        && vars.get("LOG_LEVEL") == Some(&"debug".to_string())
                        && vars.get("DB_NAME") == Some(&"myapp".to_string())
                } else {
                    false
                }
            } else {
                false
            }
        })
        .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

    let worker_name_for_get = worker_name.clone();
    mock_lambda
        .expect_get_function_configuration()
        .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
        .times(1);

    mock_lambda
        .expect_delete_function()
        .returning(|_, _| Ok(()));
    mock_lambda
        .expect_get_function_configuration()
        .returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Worker".to_string(),
                    resource_name: "test-worker".to_string(),
                },
            ))
        });

    let mock_provider = setup_mock_service_provider(Arc::new(mock_lambda), None, None);

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AwsWorkerController::default())
        .platform(Platform::Aws)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
}
