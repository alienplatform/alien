//! Recover the durable checkpoint written when a heartbeat entered the update flow.
use alien_aws_clients::{
    acm::{ImportCertificateResponse, MockAcmApi},
    lambda::{FunctionConfiguration, MockLambdaApi},
    AwsClientConfig, AwsClientConfigExt, ErrorData as CloudClientErrorData,
};
use alien_azure_clients::{AzureClientConfig, AzureClientConfigExt};
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, ExternalBindings, Platform, ReleaseInfo, Resource,
    ResourceLifecycle, ResourceStatus, RuntimeMetadata, Stack, StackResourceState, StackSettings,
    StackState, Worker, WorkerCode, WorkerPublicEndpoint,
};
use alien_error::AlienError;
use alien_gcp_clients::{GcpClientConfig, GcpClientConfigExt};
use alien_infra::{
    serialize_controller, AwsWorkerController, AzureWorkerController, GcpWorkerController,
    MockPlatformServiceProvider, StackExecutor,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn legacy_checkpoint(platform: Platform, failed: bool) -> Value {
    let (mut value, resume) = match platform {
        Platform::Aws => (
            serialize_controller(&AwsWorkerController::default()).unwrap(),
            "updateCodeStart",
        ),
        Platform::Gcp => (
            serialize_controller(&GcpWorkerController::default()).unwrap(),
            "updateStart",
        ),
        Platform::Azure => (
            serialize_controller(&AzureWorkerController::default()).unwrap(),
            "updateStart",
        ),
        _ => unreachable!(),
    };
    value["state"] = json!(if failed { "updateFailed" } else { resume });
    value["certificateIssuedAt"] = json!("2024-01-01T00:00:00Z");
    match platform {
        Platform::Aws => {
            value["arn"] = json!("arn:aws:lambda:us-east-1:123456789012:function:test-w");
            value["certificateArn"] = json!("arn:aws:acm:us-east-1:123456789012:certificate/test");
        }
        Platform::Gcp => {
            value["serviceName"] = json!("test-w");
            value["sslCertificateName"] = json!("test-w-cert");
            value["targetHttpsProxyName"] = json!("test-w-proxy");
        }
        Platform::Azure => {
            value["containerAppName"] = json!("test-w");
            value["containerAppsCertificateId"] = json!("/subscriptions/test/resourceGroups/test/providers/Microsoft.App/managedEnvironments/test/certificates/test-w-cert");
        }
        _ => unreachable!(),
    }
    value
}

#[tokio::test]
async fn unchanged_deployment_retry_recovers_legacy_certificate_update_failure() {
    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let worker = Worker::new("w".to_string())
            .code(WorkerCode::Image {
                image: "example.com/worker:v1".to_string(),
            })
            .permissions("default-profile".to_string())
            .public_endpoint(WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: None,
                wildcard_subdomains: false,
            })
            .build();
        let stack = Stack::new("test".to_string())
            .add(worker.clone(), ResourceLifecycle::Live)
            .build();
        let mut stack_state = StackState::new(platform);
        stack_state.resources.insert(
            "w".to_string(),
            StackResourceState::builder()
                .resource_type("worker".to_string())
                .config(Resource::new(worker))
                .status(ResourceStatus::UpdateFailed)
                .internal_state(legacy_checkpoint(platform, true))
                .last_failed_state(legacy_checkpoint(platform, false))
                .lifecycle(ResourceLifecycle::Live)
                .build(),
        );
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
        let state = DeploymentState {
            status: DeploymentStatus::RefreshFailed,
            platform,
            current_release: Some(ReleaseInfo {
                release_id: Some("v1".to_string()),
                version: None,
                description: None,
                stack: stack.clone(),
            }),
            target_release: None,
            stack_state: Some(stack_state),
            error: None,
            environment_info: None,
            runtime_metadata: Some(RuntimeMetadata {
                prepared_stack: Some(stack.clone()),
                ..Default::default()
            }),
            retry_requested: true,
            protocol_version: alien_core::deployment_protocol_version_for_platform(platform, None),
        };
        let client = match platform {
            Platform::Aws => ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            Platform::Gcp => ClientConfig::Gcp(Box::new(GcpClientConfig::mock())),
            Platform::Azure => ClientConfig::Azure(Box::new(AzureClientConfig::mock())),
            _ => unreachable!(),
        };
        // No provider mutations are allowed: the certificate was already installed.
        let services = Arc::new(MockPlatformServiceProvider::new());
        let state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        let retried = alien_deployment::step(
            state,
            config.clone(),
            client.clone(),
            Some(services.clone()),
        )
        .await
        .unwrap()
        .state;
        assert!(!retried.retry_requested);
        let executor = StackExecutor::builder(&stack, client.clone())
            .deployment_config(&config)
            .service_provider(services.clone())
            .build()
            .unwrap();
        let plan = executor
            .plan(retried.stack_state.as_ref().unwrap())
            .unwrap();
        assert!(
            plan.updates.is_empty(),
            "unchanged retry must not manufacture an update"
        );
        let recovered = alien_deployment::step(retried, config, client, Some(services))
            .await
            .unwrap();
        assert_eq!(
            recovered.state.status,
            DeploymentStatus::Running,
            "{platform:?}"
        );
        let resources = &recovered.state.stack_state.as_ref().unwrap().resources;
        assert_eq!(
            resources["w"].status,
            ResourceStatus::Running,
            "{platform:?}"
        );
        assert!(resources["w"].error.is_none());
        assert!(resources["w"].last_failed_state.is_none());
        assert!(resources["w"].previous_config.is_none());
        assert!(recovered.suggested_delay_ms.is_some_and(|delay| delay > 0));
    }
}

#[tokio::test]
async fn failed_certificate_renewal_preserves_delay_through_deployment_step() {
    let worker = Worker::new("w".to_string())
        .code(WorkerCode::Image {
            image: "example.com/worker:v1".to_string(),
        })
        .permissions("default-profile".to_string())
        .public_endpoint(WorkerPublicEndpoint {
            name: "api".to_string(),
            host_label: None,
            wildcard_subdomains: false,
        })
        .build();
    let stack = Stack::new("test".to_string())
        .add(worker.clone(), ResourceLifecycle::Live)
        .build();
    let mut checkpoint = legacy_checkpoint(Platform::Aws, false);
    checkpoint["state"] = json!("ready");
    let mut stack_state = StackState::with_resource_prefix(Platform::Aws, "test".to_string());
    stack_state.resources.insert(
        "w".to_string(),
        StackResourceState::builder()
            .resource_type("worker".to_string())
            .config(Resource::new(worker))
            .status(ResourceStatus::Running)
            .internal_state(checkpoint)
            .retry_attempt(9)
            .lifecycle(ResourceLifecycle::Live)
            .build(),
    );
    let config = DeploymentConfig::builder().stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot { variables: vec![], hash: String::new(), created_at: String::new() })
        .external_bindings(ExternalBindings::default()).allow_frozen_changes(false)
        .domain_metadata(serde_json::from_value(json!({
            "baseDomain": "example.com", "publicSubdomain": "test", "hostedZoneId": "test-zone",
            "resources": { "w": { "fqdn": "w.example.com", "certificateId": "test-cert",
                "certificateStatus": "issued", "dnsStatus": "active", "dnsError": null,
                "issuedAt": "2025-01-01T00:00:00Z", "endpoints": {}, "aliases": [],
                "certificateChain": "-----BEGIN CERTIFICATE-----\nMIIBtest\n-----END CERTIFICATE-----",
                "privateKey": "-----BEGIN PRIVATE KEY-----\nMIIBtest\n-----END PRIVATE KEY-----"
            }}
        })).unwrap()).build();
    let mut state = DeploymentState {
        status: DeploymentStatus::Running,
        platform: Platform::Aws,
        current_release: Some(ReleaseInfo {
            release_id: Some("v1".to_string()),
            version: None,
            description: None,
            stack: stack.clone(),
        }),
        target_release: None,
        stack_state: Some(stack_state),
        error: None,
        environment_info: None,
        runtime_metadata: Some(RuntimeMetadata {
            prepared_stack: Some(stack),
            ..Default::default()
        }),
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };
    let mut lambda = MockLambdaApi::new();
    lambda
        .expect_get_function_configuration()
        .returning(|name, _| {
            Ok(FunctionConfiguration {
                function_name: Some(name.to_string()),
                function_arn: Some(
                    "arn:aws:lambda:us-east-1:123456789012:function:test-w".to_string(),
                ),
                state: Some("Active".to_string()),
                last_update_status: Some("Successful".to_string()),
                ..Default::default()
            })
        });
    let attempts = Arc::new(AtomicUsize::new(0));
    let writes = attempts.clone();
    let mut acm = MockAcmApi::new();
    acm.expect_reimport_certificate()
        .times(3)
        .returning(move |request| {
            assert_eq!(
                request.certificate_arn,
                "arn:aws:acm:us-east-1:123456789012:certificate/test"
            );
            if writes.fetch_add(1, Ordering::SeqCst) < 2 {
                let mut error = AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "Certificate".to_string(),
                    resource_name: request.certificate_arn,
                });
                error.retryable = true;
                return Err(error);
            }
            Ok(ImportCertificateResponse {
                certificate_arn: request.certificate_arn,
            })
        });
    let mut provider = MockPlatformServiceProvider::new();
    let lambda = Arc::new(lambda);
    provider
        .expect_get_aws_lambda_client()
        .returning(move |_| Ok(lambda.clone()));
    let acm = Arc::new(acm);
    provider
        .expect_get_aws_acm_client()
        .returning(move |_| Ok(acm.clone()));
    let provider = Arc::new(provider);
    let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
    for attempt in 0..3 {
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        let result = alien_deployment::step(
            state,
            config.clone(),
            client.clone(),
            Some(provider.clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            result.suggested_delay_ms,
            Some(30_000),
            "deployment must retain the executor's retry delay"
        );
        assert_eq!(result.heartbeats.len(), 1);
        state = result.state;
        let resource = &state.stack_state.as_ref().unwrap().resources["w"];
        if attempt < 2 {
            assert_eq!(state.status, DeploymentStatus::RefreshFailed);
            assert_eq!(resource.status, ResourceStatus::RefreshFailed);
            assert!(resource
                .error
                .as_ref()
                .unwrap()
                .message
                .contains("certificate"));
            assert_eq!(
                resource.internal_state.as_ref().unwrap()["certificateIssuedAt"],
                json!("2024-01-01T00:00:00Z")
            );
        } else {
            assert_eq!(state.status, DeploymentStatus::Running);
            assert_eq!(resource.status, ResourceStatus::Running);
            assert!(resource.error.is_none());
            assert_eq!(
                resource.internal_state.as_ref().unwrap()["certificateIssuedAt"],
                json!("2025-01-01T00:00:00Z")
            );
        }
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}
