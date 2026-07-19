//! # AWS Worker Controller Tests
//!
//! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

use std::collections::HashMap;
use std::sync::Arc;

use alien_aws_clients::acm::{ImportCertificateResponse, MockAcmApi};
use alien_aws_clients::apigatewayv2::{
    Api, ApiMapping, DomainName, DomainNameConfiguration, Integration, MockApiGatewayV2Api, Route,
    Stage,
};
use alien_aws_clients::iam::MockIamApi;
use alien_aws_clients::lambda::{AddPermissionResponse, FunctionConfiguration, MockLambdaApi};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CertificateStatus, DnsRecordStatus, DomainMetadata, Platform, PublicEndpointUrls,
    ResourceDomainInfo, ResourceStatus, Worker, WorkerOutputs,
};
use alien_error::AlienError;
use httpmock::prelude::*;
use rstest::rstest;

use crate::core::controller_test::SingleControllerExecutor;
use crate::core::MockPlatformServiceProvider;
use crate::worker::{
    fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock, AwsWorkerController,
};

fn create_successful_function_response(worker_name: &str) -> FunctionConfiguration {
    FunctionConfiguration {
        function_name: Some(worker_name.to_string()),
        function_arn: Some(format!(
            "arn:aws:lambda:us-east-1:123456789012:function:{}",
            worker_name
        )),
        state: Some("Active".to_string()),
        last_update_status: Some("Successful".to_string()),
        kms_key_arn: None,
    }
}

fn create_test_domain_metadata(resource_id: &str) -> DomainMetadata {
    let mut resources = HashMap::new();
    resources.insert(
        resource_id.to_string(),
        ResourceDomainInfo {
            fqdn: format!("{}.test.example.com", resource_id),
            certificate_id: "test-cert-id".to_string(),
            certificate_status: CertificateStatus::Issued,
            dns_status: DnsRecordStatus::Active,
            dns_error: None,
            certificate_chain: Some(
                "-----BEGIN CERTIFICATE-----\nMIIBtest\n-----END CERTIFICATE-----\n".to_string(),
            ),
            private_key: Some(
                "-----BEGIN RSA PRIVATE KEY-----\nMIIBtest\n-----END RSA PRIVATE KEY-----\n"
                    .to_string(),
            ),
            endpoints: HashMap::new(),
            aliases: Vec::new(),
            issued_at: Some("2024-01-01T00:00:00Z".to_string()),
        },
    );
    DomainMetadata {
        base_domain: "test.example.com".to_string(),
        public_subdomain: "test".to_string(),
        hosted_zone_id: "Z1234567890ABC".to_string(),
        resources,
    }
}

fn create_acm_mock_for_creation() -> Arc<MockAcmApi> {
    let mut mock_acm = MockAcmApi::new();
    mock_acm.expect_import_certificate().returning(|_| {
        Ok(ImportCertificateResponse {
            certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                .to_string(),
        })
    });
    Arc::new(mock_acm)
}

fn create_acm_mock_for_creation_and_deletion() -> Arc<MockAcmApi> {
    let mut mock_acm = MockAcmApi::new();
    mock_acm.expect_import_certificate().returning(|_| {
        Ok(ImportCertificateResponse {
            certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                .to_string(),
        })
    });
    mock_acm.expect_delete_certificate().returning(|_| Ok(()));
    Arc::new(mock_acm)
}

fn create_apigatewayv2_mock_for_creation() -> Arc<MockApiGatewayV2Api> {
    let mut mock_apigw = MockApiGatewayV2Api::new();
    mock_apigw.expect_create_api().returning(|_| {
        Ok(Api {
            api_id: Some("test-api-id".to_string()),
            api_endpoint: Some(
                "https://test-api-id.execute-api.us-east-1.amazonaws.com".to_string(),
            ),
            name: None,
            protocol_type: None,
        })
    });
    mock_apigw.expect_create_integration().returning(|_, _| {
        Ok(Integration {
            integration_id: Some("test-integration-id".to_string()),
            integration_type: None,
            integration_uri: None,
        })
    });
    mock_apigw.expect_create_route().returning(|_, _| {
        Ok(Route {
            route_id: Some("test-route-id".to_string()),
            route_key: None,
        })
    });
    mock_apigw.expect_create_stage().returning(|_, _| {
        Ok(Stage {
            stage_name: Some("$default".to_string()),
            auto_deploy: None,
        })
    });
    mock_apigw.expect_create_domain_name().returning(|_| {
        Ok(DomainName {
            domain_name: Some("test.example.com".to_string()),
            domain_name_configurations: Some(vec![DomainNameConfiguration {
                certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                    .to_string(),
                endpoint_type: "REGIONAL".to_string(),
                security_policy: "TLS_1_2".to_string(),
                api_gateway_domain_name: Some(
                    "test.execute-api.us-east-1.amazonaws.com".to_string(),
                ),
                hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
            }]),
        })
    });
    mock_apigw.expect_create_api_mapping().returning(|_, _| {
        Ok(ApiMapping {
            api_mapping_id: Some("test-mapping-id".to_string()),
            api_mapping_key: None,
            stage: None,
        })
    });
    Arc::new(mock_apigw)
}

fn create_apigatewayv2_mock_for_creation_and_deletion() -> Arc<MockApiGatewayV2Api> {
    let mut mock_apigw = MockApiGatewayV2Api::new();
    mock_apigw.expect_create_api().returning(|_| {
        Ok(Api {
            api_id: Some("test-api-id".to_string()),
            api_endpoint: Some(
                "https://test-api-id.execute-api.us-east-1.amazonaws.com".to_string(),
            ),
            name: None,
            protocol_type: None,
        })
    });
    mock_apigw.expect_create_integration().returning(|_, _| {
        Ok(Integration {
            integration_id: Some("test-integration-id".to_string()),
            integration_type: None,
            integration_uri: None,
        })
    });
    mock_apigw.expect_create_route().returning(|_, _| {
        Ok(Route {
            route_id: Some("test-route-id".to_string()),
            route_key: None,
        })
    });
    mock_apigw.expect_create_stage().returning(|_, _| {
        Ok(Stage {
            stage_name: Some("$default".to_string()),
            auto_deploy: None,
        })
    });
    mock_apigw.expect_create_domain_name().returning(|_| {
        Ok(DomainName {
            domain_name: Some("test.example.com".to_string()),
            domain_name_configurations: Some(vec![DomainNameConfiguration {
                certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                    .to_string(),
                endpoint_type: "REGIONAL".to_string(),
                security_policy: "TLS_1_2".to_string(),
                api_gateway_domain_name: Some(
                    "test.execute-api.us-east-1.amazonaws.com".to_string(),
                ),
                hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
            }]),
        })
    });
    mock_apigw.expect_create_api_mapping().returning(|_, _| {
        Ok(ApiMapping {
            api_mapping_id: Some("test-mapping-id".to_string()),
            api_mapping_key: None,
            stage: None,
        })
    });
    mock_apigw
        .expect_delete_api_mapping()
        .returning(|_, _| Ok(()));
    mock_apigw.expect_delete_domain_name().returning(|_| Ok(()));
    mock_apigw.expect_delete_api().returning(|_| Ok(()));
    Arc::new(mock_apigw)
}

fn setup_mock_client_for_creation_and_update(
    worker_name: &str,
    has_url: bool,
) -> Arc<MockLambdaApi> {
    let mut mock_lambda = MockLambdaApi::new();

    // Mock successful worker creation
    let worker_name = worker_name.to_string();
    let worker_name_for_create = worker_name.clone();
    mock_lambda
        .expect_create_function()
        .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

    // Mock worker status checks - first pending, then active
    let worker_name_for_get = worker_name.clone();
    mock_lambda
        .expect_get_function_configuration()
        .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)));

    // Mock API Gateway permission and self-binding env var update if public ingress
    if has_url {
        mock_lambda
            .expect_add_permission()
            .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

        let worker_name_for_self_binding = worker_name.clone();
        mock_lambda
            .expect_update_function_configuration()
            .returning(move |_, _| {
                Ok(create_successful_function_response(
                    &worker_name_for_self_binding,
                ))
            });
    }

    // Mock concurrency operations (may or may not be called depending on worker config)
    mock_lambda
        .expect_put_function_concurrency()
        .returning(|_, _| Ok(()));
    mock_lambda
        .expect_delete_function_concurrency()
        .returning(|_| Ok(()));

    // Mock successful updates
    let worker_name_for_code_update = worker_name.clone();
    mock_lambda
        .expect_update_function_code()
        .returning(move |_, _| {
            Ok(create_successful_function_response(
                &worker_name_for_code_update,
            ))
        });

    if !has_url {
        let worker_name_for_config_update = worker_name.clone();
        mock_lambda
            .expect_update_function_configuration()
            .returning(move |_, _| {
                Ok(create_successful_function_response(
                    &worker_name_for_config_update,
                ))
            });
    }

    Arc::new(mock_lambda)
}

fn setup_mock_client_for_creation_and_deletion(
    worker_name: &str,
    has_url: bool,
) -> Arc<MockLambdaApi> {
    let mut mock_lambda = MockLambdaApi::new();

    // Mock successful worker creation
    let worker_name = worker_name.to_string();
    let worker_name_for_create = worker_name.clone();
    mock_lambda
        .expect_create_function()
        .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

    // Mock worker status checks
    let worker_name_for_get = worker_name.clone();
    mock_lambda
        .expect_get_function_configuration()
        .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
        .times(1); // Only for creation flow

    // Mock API Gateway permission and self-binding env var update if public ingress
    if has_url {
        mock_lambda
            .expect_add_permission()
            .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

        // Mock update_function_configuration for self-binding env var update
        let worker_name_for_config_update = worker_name.clone();
        mock_lambda
            .expect_update_function_configuration()
            .returning(move |_, _| {
                Ok(create_successful_function_response(
                    &worker_name_for_config_update,
                ))
            });
    }

    // Mock concurrency operations (may or may not be called depending on worker config)
    mock_lambda
        .expect_put_function_concurrency()
        .returning(|_, _| Ok(()));
    mock_lambda
        .expect_delete_function_concurrency()
        .returning(|_| Ok(()));

    // Mock successful worker deletion
    mock_lambda
        .expect_delete_function()
        .returning(|_, _| Ok(()));

    // Mock worker not found during deletion check
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

    Arc::new(mock_lambda)
}

fn setup_mock_client_for_best_effort_deletion(
    _worker_name: &str,
    function_missing: bool,
) -> Arc<MockLambdaApi> {
    let mut mock_lambda = MockLambdaApi::new();

    // Mock worker deletion (might fail if worker missing)
    if function_missing {
        mock_lambda.expect_delete_function().returning(|_, _| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Worker".to_string(),
                    resource_name: "test-worker".to_string(),
                },
            ))
        });
    } else {
        mock_lambda
            .expect_delete_function()
            .returning(|_, _| Ok(()));
    }

    // Always return not found for final status check
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

    Arc::new(mock_lambda)
}

fn create_aws_iam_mock_for_resource_permissions() -> Arc<MockIamApi> {
    let mut mock_iam = MockIamApi::new();
    mock_iam
        .expect_put_role_policy()
        .returning(|_, _, _| Ok(()));
    Arc::new(mock_iam)
}

fn setup_mock_service_provider(
    mock_lambda: Arc<MockLambdaApi>,
    mock_acm: Option<Arc<MockAcmApi>>,
    mock_apigw: Option<Arc<MockApiGatewayV2Api>>,
) -> Arc<MockPlatformServiceProvider> {
    let mut mock_provider = MockPlatformServiceProvider::new();

    mock_provider
        .expect_get_aws_lambda_client()
        .returning(move |_| Ok(mock_lambda.clone()));

    // Mock IAM client for resource-scoped permissions (ApplyingResourcePermissions state)
    let mock_iam = create_aws_iam_mock_for_resource_permissions();
    mock_provider
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(mock_iam.clone()));

    if let Some(acm) = mock_acm {
        mock_provider
            .expect_get_aws_acm_client()
            .returning(move |_| Ok(acm.clone()));
    }

    if let Some(apigw) = mock_apigw {
        mock_provider
            .expect_get_aws_apigatewayv2_client()
            .returning(move |_| Ok(apigw.clone()));
    }

    Arc::new(mock_provider)
}

/// Sets up all mocks for a worker test, including Lambda, ACM, and API Gateway.
///
/// Returns `(mock_provider, optional_mock_server, optional_domain_metadata, optional_public_urls)`.
/// For public workers, `domain_metadata` and `public_urls` must be set on the executor builder.
/// When a readiness probe is configured, `public_urls` overrides the FQDN URL so the probe
/// hits the local mock HTTP server instead.
fn setup_mocks_for_function(
    worker: &Worker,
    worker_name: &str,
    for_deletion: bool,
) -> (
    Arc<MockPlatformServiceProvider>,
    Option<MockServer>,
    Option<DomainMetadata>,
    Option<PublicEndpointUrls>,
) {
    let has_url = !worker.public_endpoints.is_empty();
    let needs_readiness_probe = has_url && worker.readiness_probe.is_some();

    // Set up mock server for readiness probe if needed
    let mock_server = if needs_readiness_probe {
        Some(create_readiness_probe_mock(worker))
    } else {
        None
    };

    // Set up Lambda client mock (same for both flows; URL config calls are removed)
    let lambda_mock = if for_deletion {
        setup_mock_client_for_creation_and_deletion(worker_name, has_url)
    } else {
        setup_mock_client_for_creation_and_update(worker_name, has_url)
    };

    // Set up ACM and API Gateway mocks for public workers
    let (acm_mock, apigw_mock, domain_metadata, public_endpoints) = if has_url {
        let dm = create_test_domain_metadata(&worker.id);
        let acm = if for_deletion {
            create_acm_mock_for_creation_and_deletion()
        } else {
            create_acm_mock_for_creation()
        };
        let apigw = if for_deletion {
            create_apigatewayv2_mock_for_creation_and_deletion()
        } else {
            create_apigatewayv2_mock_for_creation()
        };
        // For readiness probe tests, override the FQDN URL with the mock server URL
        let pub_endpoints = mock_server.as_ref().map(|server| {
            HashMap::from([(
                worker.id.clone(),
                HashMap::from([("api".to_string(), server.base_url())]),
            )])
        });
        (Some(acm), Some(apigw), Some(dm), pub_endpoints)
    } else {
        (None, None, None, None)
    };

    let mock_provider = setup_mock_service_provider(lambda_mock, acm_mock, apigw_mock);

    (
        mock_provider,
        mock_server,
        domain_metadata,
        public_endpoints,
    )
}

// ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

#[rstest]
#[case::basic(basic_function(), false)]
#[case::env_vars(function_with_env_vars(), false)]
#[case::storage_link(function_with_storage_link(), false)]
#[case::env_and_storage(function_with_env_and_storage(), false)]
#[case::multiple_storages(function_with_multiple_storages(), false)]
#[case::public_ingress(function_public_ingress(), true)]
#[case::private_ingress(function_private_ingress(), false)]
#[case::concurrency(function_with_concurrency(), false)]
#[case::custom_config(function_custom_config(), false)]
#[case::readiness_probe(function_with_readiness_probe(), true)]
#[case::complete_test(function_complete_test(), true)]
#[tokio::test]
async fn test_create_and_delete_flow_succeeds(#[case] worker: Worker, #[case] _has_url: bool) {
    let worker_name = format!("test-{}", worker.id);
    let (mock_provider, _mock_server, domain_metadata, public_endpoints) =
        setup_mocks_for_function(&worker, &worker_name, true);

    let mut builder = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AwsWorkerController::default())
        .platform(Platform::Aws)
        .service_provider(mock_provider)
        .with_test_dependencies();

    if let Some(dm) = domain_metadata {
        builder = builder.domain_metadata(dm);
    }
    if let Some(urls) = public_endpoints {
        builder = builder.public_endpoints(urls);
    }

    let mut executor = builder.build().await.unwrap();

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

    let worker_name = format!("test-{}", worker_id);
    let (mock_provider, mock_server, domain_metadata, public_endpoints) =
        setup_mocks_for_function(&to_function, &worker_name, false);

    // Start with the "from" worker in Ready state
    let mut ready_controller = AwsWorkerController::mock_ready(&worker_name);

    // If the target worker has a readiness probe, update the controller URL to point to mock server
    if to_function.readiness_probe.is_some() && !to_function.public_endpoints.is_empty() {
        if let Some(ref server) = mock_server {
            ready_controller.url = Some(server.base_url());
        }
    }

    let mut builder = SingleControllerExecutor::builder()
        .resource(from_function)
        .controller(ready_controller)
        .platform(Platform::Aws)
        .service_provider(mock_provider)
        .with_test_dependencies();

    if let Some(dm) = domain_metadata {
        builder = builder.domain_metadata(dm);
    }
    if let Some(urls) = public_endpoints {
        builder = builder.public_endpoints(urls);
    }

    let mut executor = builder.build().await.unwrap();

    // Ensure we start in Running state
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Update to the new worker
    let target_is_public = !to_function.public_endpoints.is_empty();
    executor.update(to_function).unwrap();

    // Run the update flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    if target_is_public {
        let url = function_outputs
            .public_endpoints
            .get("default")
            .expect("default public endpoint")
            .url
            .as_str();
        assert!(url.starts_with("http://") || url.starts_with("https://"));
        assert!(!url.contains("lambda-url"));
    }
}

// ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

#[rstest]
#[case::basic(basic_function(), false)]
#[case::public_with_missing_function(function_public_ingress(), true)]
#[case::public(function_public_ingress(), false)]
#[case::private_with_missing_function(function_private_ingress(), true)]
#[tokio::test]
async fn test_best_effort_deletion_when_resources_missing(
    #[case] worker: Worker,
    #[case] function_missing: bool,
) {
    let worker_name = format!("test-{}", worker.id);
    let has_url = !worker.public_endpoints.is_empty();
    let mock_lambda = setup_mock_client_for_best_effort_deletion(&worker_name, function_missing);
    let mock_provider = setup_mock_service_provider(mock_lambda, None, None);

    // Start with a ready controller
    let mut ready_controller = AwsWorkerController::mock_ready(&worker_name);
    if has_url {
        ready_controller.url =
            Some("https://example.execute-api.us-east-1.amazonaws.com".to_string());
    }

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(ready_controller)
        .platform(Platform::Aws)
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

// ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

/// Test that verifies public workers go through ACM certificate import and API Gateway setup.
#[tokio::test]
async fn test_public_function_creates_api_gateway_and_certificate() {
    let worker = function_public_ingress();
    let worker_name = format!("test-{}", worker.id);
    let domain_metadata = create_test_domain_metadata(&worker.id);

    let mut mock_lambda = MockLambdaApi::new();

    // Mock worker creation
    let worker_name_for_create = worker_name.clone();
    mock_lambda
        .expect_create_function()
        .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

    let worker_name_for_get = worker_name.clone();
    mock_lambda
        .expect_get_function_configuration()
        .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
        .times(1);

    // Validate API Gateway permission is added with the correct apigateway principal
    mock_lambda
        .expect_add_permission()
        .withf(|_, request| {
            request.statement_id == "ApiGatewayInvoke"
                && request.action == "lambda:InvokeFunction"
                && request.principal == "apigateway.amazonaws.com"
        })
        .returning(|_, _| Ok(AddPermissionResponse { statement: None }));

    // Mock self-binding env var update
    let worker_name_for_config_update = worker_name.clone();
    mock_lambda
        .expect_update_function_configuration()
        .returning(move |_, _| {
            Ok(create_successful_function_response(
                &worker_name_for_config_update,
            ))
        });

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

    // Validate ACM certificate import
    let mut mock_acm = MockAcmApi::new();
    mock_acm
        .expect_import_certificate()
        .times(1)
        .returning(|_| {
            Ok(ImportCertificateResponse {
                certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                    .to_string(),
            })
        });
    mock_acm.expect_delete_certificate().returning(|_| Ok(()));

    // Validate API Gateway is created with the worker's name in the API name
    let mut mock_apigw = MockApiGatewayV2Api::new();
    mock_apigw
        .expect_create_api()
        .withf(|request| request.name.contains("public-func"))
        .returning(|_| {
            Ok(Api {
                api_id: Some("test-api-id".to_string()),
                api_endpoint: None,
                name: None,
                protocol_type: None,
            })
        });
    mock_apigw.expect_create_integration().returning(|_, _| {
        Ok(Integration {
            integration_id: Some("test-integration-id".to_string()),
            integration_type: None,
            integration_uri: None,
        })
    });
    mock_apigw.expect_create_route().returning(|_, _| {
        Ok(Route {
            route_id: Some("test-route-id".to_string()),
            route_key: None,
        })
    });
    mock_apigw.expect_create_stage().returning(|_, _| {
        Ok(Stage {
            stage_name: Some("$default".to_string()),
            auto_deploy: None,
        })
    });
    mock_apigw.expect_create_domain_name().returning(|_| {
        Ok(DomainName {
            domain_name: Some("public-func.test.example.com".to_string()),
            domain_name_configurations: Some(vec![DomainNameConfiguration {
                certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/test-cert-id"
                    .to_string(),
                endpoint_type: "REGIONAL".to_string(),
                security_policy: "TLS_1_2".to_string(),
                api_gateway_domain_name: Some(
                    "test.execute-api.us-east-1.amazonaws.com".to_string(),
                ),
                hosted_zone_id: Some("Z1D633PJN98FT9".to_string()),
            }]),
        })
    });
    mock_apigw.expect_create_api_mapping().returning(|_, _| {
        Ok(ApiMapping {
            api_mapping_id: Some("test-mapping-id".to_string()),
            api_mapping_key: None,
            stage: None,
        })
    });
    mock_apigw
        .expect_delete_api_mapping()
        .returning(|_, _| Ok(()));
    mock_apigw.expect_delete_domain_name().returning(|_| Ok(()));
    mock_apigw.expect_delete_api().returning(|_| Ok(()));

    let mock_provider = setup_mock_service_provider(
        Arc::new(mock_lambda),
        Some(Arc::new(mock_acm)),
        Some(Arc::new(mock_apigw)),
    );

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(AwsWorkerController::default())
        .platform(Platform::Aws)
        .service_provider(mock_provider)
        .domain_metadata(domain_metadata)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();

    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);

    // Verify URL is in outputs (derived from domain_metadata FQDN)
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    assert!(function_outputs.public_endpoints.contains_key("default"));
}

/// Test that verifies private workers don't get URL creation
#[tokio::test]
async fn test_private_function_skips_url_creation() {
    let worker = function_private_ingress();
    let worker_name = format!("test-{}", worker.id);

    let mut mock_lambda = MockLambdaApi::new();

    // Mock worker creation
    let worker_name_for_create = worker_name.clone();
    mock_lambda
        .expect_create_function()
        .returning(move |_| Ok(create_successful_function_response(&worker_name_for_create)));

    let worker_name_for_get = worker_name.clone();
    mock_lambda
        .expect_get_function_configuration()
        .returning(move |_, _| Ok(create_successful_function_response(&worker_name_for_get)))
        .times(1);

    // API Gateway and permission should NOT be called for private workers
    mock_lambda.expect_add_permission().times(0);

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

    // Verify no URL in outputs
    let outputs = executor.outputs().unwrap();
    let function_outputs = outputs.downcast_ref::<WorkerOutputs>().unwrap();
    assert!(function_outputs.public_endpoints.is_empty());
}

/// Test that verifies correct worker configuration parameters
#[tokio::test]
async fn test_worker_configuration_validation() {
    let worker = function_custom_config();
    let worker_name = format!("test-{}", worker.id);

    let mut mock_lambda = MockLambdaApi::new();

    // Validate worker creation request has correct parameters
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

/// Test that verifies environment variables are correctly passed
#[tokio::test]
async fn test_environment_variable_handling() {
    let worker = function_with_env_vars();
    let worker_name = format!("test-{}", worker.id);

    let mut mock_lambda = MockLambdaApi::new();

    // Validate worker creation request has environment variables
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
