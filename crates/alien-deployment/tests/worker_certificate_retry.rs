//! Recover the durable checkpoint written when a heartbeat entered the update flow.
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};
use alien_azure_clients::{AzureClientConfig, AzureClientConfigExt};
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, ExternalBindings, Platform, ReleaseInfo, Resource,
    ResourceLifecycle, ResourceStatus, RuntimeMetadata, Stack, StackResourceState, StackSettings,
    StackState, Worker, WorkerCode, WorkerPublicEndpoint,
};
use alien_gcp_clients::{GcpClientConfig, GcpClientConfigExt};
use alien_infra::{
    serialize_controller, AwsWorkerController, AzureWorkerController, GcpWorkerController,
    MockPlatformServiceProvider, StackExecutor,
};
use serde_json::{json, Value};
use std::sync::Arc;

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
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
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
