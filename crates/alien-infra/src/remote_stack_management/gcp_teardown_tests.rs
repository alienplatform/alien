use std::sync::Arc;

use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, GcpClientConfig, GcpCredentials,
    GcpImpersonationConfig, Platform, RemoteStackManagement, Resource, Stack, StackSettings,
    StackState,
};
use alien_gcp_clients::iam::MockIamApi;

use super::*;
use crate::core::{
    HeartbeatCollector, MockPlatformServiceProvider, PlatformServiceProvider, ResourceController,
    ResourceRegistry,
};

fn impersonated_target_config(service_account_email: &str) -> ClientConfig {
    let source = GcpClientConfig {
        project_id: "managing-project".to_string(),
        region: "us-central1".to_string(),
        credentials: GcpCredentials::AccessToken {
            token: "source-token".to_string(),
        },
        service_overrides: None,
        project_number: Some("123456789012".to_string()),
    };
    ClientConfig::Gcp(Box::new(GcpClientConfig {
        project_id: "target-project".to_string(),
        region: "us-central1".to_string(),
        credentials: GcpCredentials::ImpersonatedServiceAccount {
            source: Box::new(source),
            config: GcpImpersonationConfig {
                service_account_email: service_account_email.to_string(),
                target_project_id: Some("target-project".to_string()),
                ..GcpImpersonationConfig::default()
            },
        },
        service_overrides: None,
        project_number: Some("987654321098".to_string()),
    }))
}

#[tokio::test]
async fn teardown_revokes_bucket_access_by_deleting_identity_not_editing_bucket_iam() {
    let service_account_email = "test-stack-management@target-project.iam.gserviceaccount.com";
    let mut iam_client = MockIamApi::new();
    iam_client
        .expect_delete_service_account()
        .with(mockall::predicate::eq(service_account_email.to_string()))
        .times(1)
        .returning(|_| Ok(()));
    let iam_client = Arc::new(iam_client);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_gcp_iam_client()
        .times(1)
        .returning(move |_| Ok(iam_client.clone()));
    let provider: Arc<dyn PlatformServiceProvider> = Arc::new(provider);

    let management = RemoteStackManagement::new("management".to_string()).build();
    let desired_config = Resource::new(management);
    let state = StackState::new(Platform::Gcp);
    let stack = Stack::new("test-stack".to_string()).build();
    let registry = Arc::new(ResourceRegistry::new());
    let deployment_config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    let ctx = ResourceControllerContext {
        desired_config: &desired_config,
        platform: Platform::Gcp,
        client_config: impersonated_target_config(service_account_email),
        state: &state,
        resource_prefix: "test-stack",
        registry: &registry,
        desired_stack: &stack,
        service_provider: &provider,
        deployment_config: &deployment_config,
        heartbeat_collector: HeartbeatCollector::default(),
    };
    let mut controller = GcpRemoteStackManagementController {
        setup_managed: Some(false),
        state: GcpRemoteStackManagementState::DeleteStart,
        service_account_email: Some(service_account_email.to_string()),
        service_account_unique_id: Some("1234567890".to_string()),
        role_bound: false,
        impersonation_granted: true,
        applied_management_grant_fingerprint: Some("fingerprint".to_string()),
        remote_storage_bucket_names: vec!["setup-owned-bucket".to_string()],
        _internal_stay_count: None,
    };

    controller
        .delete_start(&ctx)
        .await
        .expect("delete start must not require bucket IAM administration");
    assert_eq!(
        controller.remote_storage_bucket_names,
        ["setup-owned-bucket".to_string()],
        "bucket grant ownership must remain checkpointed until the identity is deleted"
    );

    controller.state = GcpRemoteStackManagementState::DeletingServiceAccount;
    controller
        .deleting_service_account(&ctx)
        .await
        .expect("management identity deletion must revoke effective bucket access");
    assert!(controller.service_account_email.is_none());
    assert!(controller.remote_storage_bucket_names.is_empty());
}

#[tokio::test]
async fn setup_managed_lifecycle_never_calls_cloud_apis() {
    let service_account_email =
        "a-test-stack-managemen-12ab34cd@target-project.iam.gserviceaccount.com";
    let provider: Arc<dyn PlatformServiceProvider> = Arc::new(MockPlatformServiceProvider::new());

    let management = RemoteStackManagement::new("management".to_string()).build();
    let desired_config = Resource::new(management);
    let state = StackState::new(Platform::Gcp);
    let stack = Stack::new("test-stack".to_string()).build();
    let registry = Arc::new(ResourceRegistry::new());
    let deployment_config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build();
    let ctx = ResourceControllerContext {
        desired_config: &desired_config,
        platform: Platform::Gcp,
        client_config: impersonated_target_config(service_account_email),
        state: &state,
        resource_prefix: "test-stack",
        registry: &registry,
        desired_stack: &stack,
        service_provider: &provider,
        deployment_config: &deployment_config,
        heartbeat_collector: HeartbeatCollector::default(),
    };
    let mut controller = GcpRemoteStackManagementController {
        setup_managed: Some(true),
        state: GcpRemoteStackManagementState::UpdateStart,
        service_account_email: Some(service_account_email.to_string()),
        service_account_unique_id: Some("1234567890".to_string()),
        role_bound: true,
        impersonation_granted: true,
        applied_management_grant_fingerprint: None,
        remote_storage_bucket_names: vec!["setup-owned-bucket".to_string()],
        _internal_stay_count: None,
    };

    assert!(!controller.needs_update(&ctx).expect("ownership check"));
    controller.update_start(&ctx).await.expect("update skip");
    controller.state = GcpRemoteStackManagementState::DeleteStart;
    controller.delete_start(&ctx).await.expect("delete skip");
    assert_eq!(
        controller.service_account_email.as_deref(),
        Some(service_account_email),
        "runtime teardown must leave the setup-owned identity intact"
    );
    assert_eq!(
        controller.remote_storage_bucket_names,
        ["setup-owned-bucket".to_string()]
    );
}
