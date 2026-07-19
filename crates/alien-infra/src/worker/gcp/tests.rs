//! # GCP Worker Controller Tests
//!
//! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::{
    CertificateStatus, DnsRecordStatus, DomainMetadata, HttpMethod, Platform, ResourceDomainInfo,
    ResourceStatus, Worker, WorkerOutputs,
};
use alien_error::AlienError;
use alien_gcp_clients::cloudrun::{Condition, ConditionState, MockCloudRunApi, Service};
use alien_gcp_clients::gcp::compute::{Address, MockComputeApi, Operation, OperationStatus};
use alien_gcp_clients::iam::{IamPolicy, MockIamApi};
use alien_gcp_clients::longrunning::Operation as LongRunningOperation;
use alien_gcp_clients::longrunning::{OperationResult, Status};
use alien_gcp_clients::pubsub::MockPubSubApi;
use httpmock::{prelude::*, Mock};
use rstest::rstest;

use super::{
    get_cloudrun_service_name, get_gcp_worker_resource_name,
    is_cross_project_image_pull_permission_error, CLOUD_RUN_SERVICE_NAME_MAX_LEN,
    GCP_RESOURCE_NAME_MAX_LEN,
};
use crate::core::MockPlatformServiceProvider;
use crate::core::{
    controller_test::{SingleControllerExecutor, SingleControllerExecutorBuilder},
    PlatformServiceProvider,
};
use crate::worker::readiness_probe::test_utils::create_readiness_probe_mock;
use crate::worker::{fixtures::*, GcpWorkerController};
use crate::GcpWorkerState;

#[test]
fn cloudrun_service_name_preserves_valid_short_names() {
    assert_eq!(
        get_cloudrun_service_name("test-stack", "worker"),
        "test-stack-worker"
    );
}

#[test]
fn image_pull_retry_only_matches_cross_project_gar_permission_denials() {
    assert!(is_cross_project_image_pull_permission_error(
        "Google Cloud Run Service Agent service-123@serverless-robot-prod.iam.gserviceaccount.com \
         was denied artifactregistry.repositories.downloadArtifacts"
    ));
    assert!(!is_cross_project_image_pull_permission_error(
        "artifactregistry.repositories.downloadArtifacts denied for a user service account"
    ));
    assert!(!is_cross_project_image_pull_permission_error(
        "Cloud Run revision failed its startup probe"
    ));
}

#[test]
fn cloudrun_service_name_caps_long_e2e_names_with_stable_hash() {
    let service_name = get_cloudrun_service_name(
        "e2e-gcp-terraform-worker-mpfa2f19-15fb",
        "test-alien-ts-function",
    );

    assert!(service_name.len() <= CLOUD_RUN_SERVICE_NAME_MAX_LEN);
    assert!(service_name.starts_with("e"));
    assert!(!service_name.ends_with('-'));
    assert!(service_name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
    assert_eq!(
        service_name,
        get_cloudrun_service_name(
            "e2e-gcp-terraform-worker-mpfa2f19-15fb",
            "test-alien-ts-function",
        )
    );
}

#[test]
fn cloudrun_service_name_sanitizes_invalid_input() {
    let service_name = get_cloudrun_service_name("123_Test.Stack", "Worker_Name_");

    assert!(service_name.len() <= CLOUD_RUN_SERVICE_NAME_MAX_LEN);
    assert!(service_name.starts_with('a'));
    assert!(!service_name.ends_with('-'));
    assert!(service_name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
}

#[test]
fn gcp_worker_resource_name_caps_long_certificate_names_with_stable_hash() {
    let cert_name = get_gcp_worker_resource_name(
        "e2e-gcp-terraform-worker-mpfgzubr-tux",
        "test-alien-ts-function",
        "cert",
    );

    assert!(cert_name.len() <= GCP_RESOURCE_NAME_MAX_LEN);
    assert!(cert_name.starts_with('e'));
    assert!(!cert_name.ends_with('-'));
    assert!(cert_name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'));
    assert_eq!(
        cert_name,
        get_gcp_worker_resource_name(
            "e2e-gcp-terraform-worker-mpfgzubr-tux",
            "test-alien-ts-function",
            "cert",
        )
    );
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
            issued_at: Some("2024-01-01T00:00:00Z".to_string()),
            aliases: Vec::new(),
        },
    );
    DomainMetadata {
        base_domain: "test.example.com".to_string(),
        public_subdomain: "test".to_string(),
        hosted_zone_id: "Z1234567890ABC".to_string(),
        resources,
    }
}

fn create_ssl_compute_mock_for_creation_and_deletion() -> Arc<MockComputeApi> {
    fn completed_compute_operation() -> Operation {
        Operation {
            name: Some("test-compute-operation".to_string()),
            status: Some(OperationStatus::Done),
            ..Default::default()
        }
    }

    let mut mock = MockComputeApi::new();
    mock.expect_insert_ssl_certificate()
        .returning(|_| Ok(completed_compute_operation()));
    mock.expect_insert_region_network_endpoint_group()
        .returning(|_, _| Ok(completed_compute_operation()));
    mock.expect_insert_backend_service()
        .returning(|_| Ok(completed_compute_operation()));
    mock.expect_insert_url_map()
        .returning(|_| Ok(completed_compute_operation()));
    mock.expect_insert_target_https_proxy()
        .returning(|_| Ok(completed_compute_operation()));
    mock.expect_insert_global_address()
        .returning(|_| Ok(completed_compute_operation()));
    mock.expect_get_global_address().returning(|_| {
        Ok(Address {
            address: Some("203.0.113.1".to_string()),
            ..Default::default()
        })
    });
    mock.expect_insert_global_forwarding_rule()
        .returning(|_| Ok(completed_compute_operation()));
    mock.expect_delete_global_forwarding_rule()
        .returning(|_| Ok(Operation::default()));
    mock.expect_delete_target_https_proxy()
        .returning(|_| Ok(Operation::default()));
    mock.expect_delete_url_map()
        .returning(|_| Ok(Operation::default()));
    mock.expect_delete_backend_service()
        .returning(|_| Ok(Operation::default()));
    mock.expect_delete_region_network_endpoint_group()
        .returning(|_, _| Ok(Operation::default()));
    mock.expect_delete_ssl_certificate()
        .returning(|_| Ok(Operation::default()));
    mock.expect_delete_global_address()
        .returning(|_| Ok(Operation::default()));
    Arc::new(mock)
}

fn resource_in_use_error() -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::InvalidInput {
        message: "The targetHttpsProxy resource is already being used by forwardingRules/test-fwd resourceInUseByAnotherResource".to_string(),
        field_name: None,
    })
}

fn create_successful_service_response(service_name: &str) -> Service {
    use alien_gcp_clients::cloudrun::Service;

    Service::builder()
        .name(format!(
            "projects/test-project/locations/us-central1/services/{}",
            service_name
        ))
        .uri(format!("https://{}-abcd1234-uc.a.run.app", service_name))
        .urls(vec![format!(
            "https://{}-abcd1234-uc.a.run.app",
            service_name
        )])
        .conditions(vec![Condition::builder()
            .r#type("Ready".to_string())
            .state(ConditionState::ConditionSucceeded)
            .build()])
        .build()
}

fn create_successful_operation_response(operation_name: &str) -> LongRunningOperation {
    LongRunningOperation::builder()
        .name(format!(
            "projects/test-project/locations/us-central1/operations/{}",
            operation_name
        ))
        .done(false)
        .build()
}

fn create_completed_operation_response(operation_name: &str) -> LongRunningOperation {
    LongRunningOperation::builder()
        .name(format!("projects/test-project/locations/us-central1/operations/{}", operation_name))
        .done(true)
        .result(OperationResult::Response {
            response: serde_json::json!({
                "name": format!("projects/test-project/locations/us-central1/services/test-{}", operation_name)
            })
        })
        .build()
}

fn create_image_pull_permission_denied_operation(operation_name: &str) -> LongRunningOperation {
    LongRunningOperation::builder()
        .name(format!(
            "projects/test-project/locations/us-central1/operations/{operation_name}"
        ))
        .done(true)
        .result(OperationResult::Error {
            error: Status::builder()
                .code(7)
                .message(
                    "Google Cloud Run Service Agent \
                     service-123@serverless-robot-prod.iam.gserviceaccount.com must have \
                     artifactregistry.repositories.downloadArtifacts permission"
                        .to_string(),
                )
                .build(),
        })
        .build()
}

fn create_empty_iam_policy() -> IamPolicy {
    IamPolicy::builder().version(1).bindings(vec![]).build()
}

fn setup_mock_client_for_creation_and_update(
    function_name: &str,
    _has_public_access: bool,
) -> Arc<MockCloudRunApi> {
    let mut mock_cloudrun = MockCloudRunApi::new();

    // Mock successful service creation
    let operation_name = format!("create-{}", function_name);
    let operation_name_for_get = operation_name.clone();
    mock_cloudrun
        .expect_create_service()
        .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

    // Mock operation status checks - first pending, then completed
    mock_cloudrun
        .expect_get_operation()
        .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)));

    // Mock service retrieval after creation
    let function_name_for_get = function_name.to_string();
    mock_cloudrun
        .expect_get_service()
        .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)));

    // Mock IAM policy operations for all workers (resource-scoped permissions + optional public access)
    mock_cloudrun
        .expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));

    mock_cloudrun
        .expect_set_service_iam_policy()
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

    // Mock successful updates
    let function_name_for_update = function_name.to_string();
    let update_operation_name = format!("update-{}", function_name_for_update);
    mock_cloudrun
        .expect_patch_service()
        .returning(move |_, _, _, _, _, _| {
            Ok(create_successful_operation_response(&update_operation_name))
        });

    Arc::new(mock_cloudrun)
}

fn setup_mock_client_for_creation_and_deletion(
    function_name: &str,
    _has_public_access: bool,
) -> Arc<MockCloudRunApi> {
    let mut mock_cloudrun = MockCloudRunApi::new();

    // Mock successful service creation
    let operation_name = format!("create-{}", function_name);
    let operation_name_for_get = operation_name.clone();
    mock_cloudrun
        .expect_create_service()
        .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

    // Mock operation status checks for creation
    mock_cloudrun
        .expect_get_operation()
        .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
        .times(1); // Only for creation flow

    // Mock service retrieval after creation
    let function_name_for_get = function_name.to_string();
    mock_cloudrun
        .expect_get_service()
        .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)))
        .times(1); // Only for creation flow

    // Mock IAM policy operations for all workers (resource-scoped permissions + optional public access)
    mock_cloudrun
        .expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));

    mock_cloudrun
        .expect_set_service_iam_policy()
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

    // Mock successful service deletion
    let function_name_for_delete = function_name.to_string();
    let delete_operation_name = format!("delete-{}", function_name_for_delete);
    let delete_operation_name_for_get = delete_operation_name.clone();
    mock_cloudrun
        .expect_delete_service()
        .returning(move |_, _, _, _| {
            Ok(create_successful_operation_response(&delete_operation_name))
        });

    // Mock operation status checks for deletion
    mock_cloudrun.expect_get_operation().returning(move |_, _| {
        Ok(create_completed_operation_response(
            &delete_operation_name_for_get,
        ))
    });

    // Mock service not found during deletion check
    mock_cloudrun.expect_get_service().returning(|_, _| {
        Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Service".to_string(),
                resource_name: "test-service".to_string(),
            },
        ))
    });

    Arc::new(mock_cloudrun)
}

fn setup_mock_client_for_best_effort_deletion(
    _function_name: &str,
    service_missing: bool,
) -> Arc<MockCloudRunApi> {
    let mut mock_cloudrun = MockCloudRunApi::new();

    // Mock service deletion (might fail if service missing)
    if service_missing {
        mock_cloudrun
            .expect_delete_service()
            .returning(|_, _, _, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "Service".to_string(),
                        resource_name: "test-service".to_string(),
                    },
                ))
            });
    } else {
        let delete_operation_name = "delete-test".to_string();
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
    }

    // Always return not found for final status check
    mock_cloudrun.expect_get_service().returning(|_, _| {
        Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Service".to_string(),
                resource_name: "test-service".to_string(),
            },
        ))
    });

    Arc::new(mock_cloudrun)
}

fn create_gcp_iam_mock_for_resource_permissions() -> Arc<MockIamApi> {
    Arc::new(MockIamApi::new())
}

fn setup_mock_service_provider(
    mock_cloudrun: Arc<MockCloudRunApi>,
    mock_compute: Option<Arc<MockComputeApi>>,
) -> Arc<MockPlatformServiceProvider> {
    setup_mock_service_provider_with_pubsub(
        mock_cloudrun,
        mock_compute,
        Arc::new(MockPubSubApi::new()),
    )
}

fn setup_mock_service_provider_with_pubsub(
    mock_cloudrun: Arc<MockCloudRunApi>,
    mock_compute: Option<Arc<MockComputeApi>>,
    mock_pubsub: Arc<MockPubSubApi>,
) -> Arc<MockPlatformServiceProvider> {
    let mut mock_provider = MockPlatformServiceProvider::new();

    mock_provider
        .expect_get_gcp_cloudrun_client()
        .returning(move |_| Ok(mock_cloudrun.clone()));

    if let Some(compute) = mock_compute {
        mock_provider
            .expect_get_gcp_compute_client()
            .returning(move |_| Ok(compute.clone()));
    }

    // Mock IAM client for resource-scoped permissions.
    let mock_iam = create_gcp_iam_mock_for_resource_permissions();
    mock_provider
        .expect_get_gcp_iam_client()
        .returning(move |_| Ok(mock_iam.clone()));

    mock_provider
        .expect_get_gcp_pubsub_client()
        .returning(move |_| Ok(mock_pubsub.clone()));

    Arc::new(mock_provider)
}

/// Sets up mock CloudRun client and optional readiness probe mock server
/// Returns (cloudrun_mock_provider, optional_mock_server, optional_domain_metadata)
fn setup_mocks_for_function(
    worker: &Worker,
    function_name: &str,
    for_deletion: bool,
) -> (
    Arc<MockPlatformServiceProvider>,
    Option<MockServer>,
    Option<DomainMetadata>,
) {
    let has_public_access = !worker.public_endpoints.is_empty();
    let needs_readiness_probe = has_public_access && worker.readiness_probe.is_some();

    // Set up mock server for readiness probe if needed
    let mock_server = if needs_readiness_probe {
        Some(create_readiness_probe_mock(worker))
    } else {
        None
    };

    // Set up CloudRun client mock
    let cloudrun_mock = if for_deletion {
        if let Some(ref _server) = mock_server {
            setup_mock_client_for_creation_and_deletion_with_mock_url(
                function_name,
                has_public_access,
                &_server.base_url(),
            )
        } else {
            setup_mock_client_for_creation_and_deletion(function_name, has_public_access)
        }
    } else {
        if let Some(ref _server) = mock_server {
            setup_mock_client_for_creation_and_update_with_mock_url(
                function_name,
                has_public_access,
                &_server.base_url(),
            )
        } else {
            setup_mock_client_for_creation_and_update(function_name, has_public_access)
        }
    };

    // For public workers, also set up compute mock and domain metadata
    let (compute_mock, domain_metadata) = if has_public_access {
        let dm = create_test_domain_metadata(&worker.id);
        let compute = create_ssl_compute_mock_for_creation_and_deletion();
        (Some(compute), Some(dm))
    } else {
        (None, None)
    };

    let mock_provider = setup_mock_service_provider(cloudrun_mock, compute_mock);

    (mock_provider, mock_server, domain_metadata)
}

fn setup_mock_client_for_creation_and_update_with_mock_url(
    function_name: &str,
    has_public_access: bool,
    mock_url: &str,
) -> Arc<MockCloudRunApi> {
    let mut mock_cloudrun = MockCloudRunApi::new();

    // Mock successful service creation
    let operation_name = format!("create-{}", function_name);
    let operation_name_for_get = operation_name.clone();
    mock_cloudrun
        .expect_create_service()
        .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

    // Mock operation status checks
    mock_cloudrun
        .expect_get_operation()
        .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)));

    // Mock service retrieval after creation - use mock URL
    let mock_url = mock_url.to_string();
    let function_name_for_get = function_name.to_string();
    mock_cloudrun.expect_get_service().returning(move |_, _| {
        let mut service = create_successful_service_response(&function_name_for_get);
        service.uri = Some(mock_url.clone());
        service.urls = vec![mock_url.clone()];
        Ok(service)
    });

    // Mock IAM policy operations for all workers (resource-scoped permissions + optional public access)
    mock_cloudrun
        .expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));

    mock_cloudrun
        .expect_set_service_iam_policy()
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

    // Mock successful updates
    let function_name_for_update = function_name.to_string();
    let update_operation_name = format!("update-{}", function_name_for_update);
    mock_cloudrun
        .expect_patch_service()
        .returning(move |_, _, _, _, _, _| {
            Ok(create_successful_operation_response(&update_operation_name))
        });

    Arc::new(mock_cloudrun)
}

fn setup_mock_client_for_creation_and_deletion_with_mock_url(
    function_name: &str,
    has_public_access: bool,
    mock_url: &str,
) -> Arc<MockCloudRunApi> {
    let mut mock_cloudrun = MockCloudRunApi::new();

    // Mock successful service creation
    let operation_name = format!("create-{}", function_name);
    let operation_name_for_get = operation_name.clone();
    mock_cloudrun
        .expect_create_service()
        .returning(move |_, _, _, _| Ok(create_successful_operation_response(&operation_name)));

    // Mock operation status checks for creation
    mock_cloudrun
        .expect_get_operation()
        .returning(move |_, _| Ok(create_completed_operation_response(&operation_name_for_get)))
        .times(1); // Only for creation flow

    // Mock service retrieval after creation - use mock URL
    let mock_url = mock_url.to_string();
    let function_name_for_get = function_name.to_string();
    mock_cloudrun
        .expect_get_service()
        .returning(move |_, _| {
            let mut service = create_successful_service_response(&function_name_for_get);
            service.uri = Some(mock_url.clone());
            service.urls = vec![mock_url.clone()];
            Ok(service)
        })
        .times(1); // Only for creation flow

    // Mock IAM policy operations for all workers (resource-scoped permissions + optional public access)
    mock_cloudrun
        .expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));

    mock_cloudrun
        .expect_set_service_iam_policy()
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

    // Mock successful service deletion
    let function_name_for_delete = function_name.to_string();
    let delete_operation_name = format!("delete-{}", function_name_for_delete);
    let delete_operation_name_for_get = delete_operation_name.clone();
    mock_cloudrun
        .expect_delete_service()
        .returning(move |_, _, _, _| {
            Ok(create_successful_operation_response(&delete_operation_name))
        });

    // Mock operation status checks for deletion
    mock_cloudrun.expect_get_operation().returning(move |_, _| {
        Ok(create_completed_operation_response(
            &delete_operation_name_for_get,
        ))
    });

    // Mock service not found during deletion check
    mock_cloudrun.expect_get_service().returning(|_, _| {
        Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Service".to_string(),
                resource_name: "test-service".to_string(),
            },
        ))
    });

    Arc::new(mock_cloudrun)
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
    let worker_id = worker.id.clone();
    let worker_is_public = !worker.public_endpoints.is_empty();
    let function_name = format!("test-{}", worker.id);
    let (mock_provider, _mock_server, domain_metadata) =
        setup_mocks_for_function(&worker, &function_name, true);

    let mut builder = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(GcpWorkerController::default())
        .platform(Platform::Gcp)
        .service_provider(mock_provider)
        .with_test_dependencies();

    if let Some(dm) = domain_metadata {
        builder = builder.domain_metadata(dm);
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
    if worker_is_public {
        let expected_url = format!("https://{}.test.example.com", worker_id);
        let endpoint = function_outputs
            .public_endpoints
            .get("default")
            .expect("public endpoint output should exist");
        assert_eq!(endpoint.url, expected_url);
        assert_eq!(
            endpoint
                .load_balancer_endpoint
                .as_ref()
                .map(|endpoint| endpoint.dns_name.as_str()),
            Some("203.0.113.1")
        );
    }

    // Delete the worker
    executor.delete().unwrap();

    // Run delete flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);

    // Verify outputs are no longer available
    assert!(executor.outputs().is_none());
}

#[tokio::test]
async fn retries_cloud_run_revision_after_gar_reader_grant_propagates() {
    let worker = basic_function();
    let function_name = format!("test-{}", worker.id);
    let operation_checks = Arc::new(AtomicUsize::new(0));
    let operation_checks_for_mock = Arc::clone(&operation_checks);
    let patch_calls = Arc::new(AtomicUsize::new(0));
    let patch_calls_for_mock = Arc::clone(&patch_calls);

    let mut mock_cloudrun = MockCloudRunApi::new();
    mock_cloudrun
        .expect_create_service()
        .times(1)
        .returning(|_, _, _, _| Ok(create_successful_operation_response("create-worker")));
    mock_cloudrun
        .expect_get_operation()
        .times(2)
        .returning(move |_, _| {
            if operation_checks_for_mock.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(create_image_pull_permission_denied_operation(
                    "create-worker",
                ))
            } else {
                Ok(create_completed_operation_response("retry-worker"))
            }
        });
    mock_cloudrun
        .expect_patch_service()
        .times(1)
        .withf(|_, _, service, update_mask, _, allow_missing| {
            let has_retry_label = service
                .template
                .as_ref()
                .and_then(|template| template.labels.as_ref())
                .is_some_and(|labels| {
                    labels.get("alien-image-pull-retry").map(String::as_str) == Some("1")
                });
            has_retry_label
                && update_mask.as_deref() == Some("template")
                && *allow_missing == Some(false)
        })
        .returning(move |_, _, _, _, _, _| {
            patch_calls_for_mock.fetch_add(1, Ordering::SeqCst);
            Ok(create_successful_operation_response("retry-worker"))
        });
    let function_name_for_get = function_name.clone();
    mock_cloudrun
        .expect_get_service()
        .returning(move |_, _| Ok(create_successful_service_response(&function_name_for_get)));
    mock_cloudrun
        .expect_get_service_iam_policy()
        .returning(|_, _| Ok(create_empty_iam_policy()));
    mock_cloudrun
        .expect_set_service_iam_policy()
        .returning(|_, _, _| Ok(create_empty_iam_policy()));

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

    for _ in 0..30 {
        if executor.status() == ResourceStatus::Running {
            break;
        }
        executor.step().await.unwrap();
    }

    assert_eq!(executor.status(), ResourceStatus::Running);
    assert_eq!(operation_checks.load(Ordering::SeqCst), 2);
    assert_eq!(patch_calls.load(Ordering::SeqCst), 1);
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

    let function_name = format!("test-{}", worker_id);
    let (mock_provider, mock_server, domain_metadata) =
        setup_mocks_for_function(&to_function, &function_name, false);

    // Start with the "from" worker in Ready state
    let mut ready_controller = GcpWorkerController::mock_ready(&function_name);

    // If the target worker has a readiness probe, update the controller URL to point to mock server
    if to_function.readiness_probe.is_some() && !to_function.public_endpoints.is_empty() {
        if let Some(ref server) = mock_server {
            ready_controller.url = Some(server.base_url());
        }
    }

    let mut builder = SingleControllerExecutor::builder()
        .resource(from_function)
        .controller(ready_controller)
        .platform(Platform::Gcp)
        .service_provider(mock_provider)
        .with_test_dependencies();

    if let Some(dm) = domain_metadata {
        builder = builder.domain_metadata(dm);
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
        let expected_url = format!("https://{}.test.example.com", worker_id);
        assert_eq!(
            function_outputs
                .public_endpoints
                .get("default")
                .map(|endpoint| endpoint.url.as_str()),
            Some(expected_url.as_str())
        );
    }
}

// ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

#[rstest]
#[case::basic(basic_function(), false)]
#[case::public_with_missing_service(function_public_ingress(), true)]
#[case::private_with_missing_service(function_private_ingress(), true)]
#[tokio::test]
async fn test_best_effort_deletion_when_resources_missing(
    #[case] worker: Worker,
    #[case] service_missing: bool,
) {
    let function_name = format!("test-{}", worker.id);
    let has_public_access = !worker.public_endpoints.is_empty();
    let mock_cloudrun = setup_mock_client_for_best_effort_deletion(&function_name, service_missing);
    let mock_provider = setup_mock_service_provider(mock_cloudrun, None);

    // Start with a ready controller
    let mut ready_controller = GcpWorkerController::mock_ready(&function_name);
    if has_public_access {
        ready_controller.url = Some("https://example-abcd1234-uc.a.run.app".to_string());
    }

    let mut executor = SingleControllerExecutor::builder()
        .resource(worker)
        .controller(ready_controller)
        .platform(Platform::Gcp)
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

#[tokio::test]
async fn test_delete_retries_target_proxy_while_forwarding_rule_reference_drains() {
    let worker = function_public_ingress();
    let function_name = format!("test-{}", worker.id);
    let proxy_delete_attempts = Arc::new(AtomicUsize::new(0));
    let proxy_delete_attempts_for_mock = Arc::clone(&proxy_delete_attempts);
    let mock_cloudrun = setup_mock_client_for_creation_and_deletion(&function_name, true);

    let mut mock_compute = MockComputeApi::new();
    mock_compute
        .expect_delete_global_forwarding_rule()
        .returning(|_| Ok(Operation::default()));
    mock_compute
        .expect_delete_target_https_proxy()
        .returning(move |_| {
            if proxy_delete_attempts_for_mock.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(resource_in_use_error())
            } else {
                Ok(Operation::default())
            }
        });
    mock_compute
        .expect_delete_url_map()
        .returning(|_| Ok(Operation::default()));
    mock_compute
        .expect_delete_backend_service()
        .returning(|_| Ok(Operation::default()));
    mock_compute
        .expect_delete_region_network_endpoint_group()
        .returning(|_, _| Ok(Operation::default()));
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
    assert_eq!(proxy_delete_attempts.load(Ordering::SeqCst), 2);
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
