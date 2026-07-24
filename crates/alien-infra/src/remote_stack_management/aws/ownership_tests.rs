use std::sync::Arc;

use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, Platform, RemoteStackManagement,
    Resource, Stack, StackSettings, StackState,
};

use super::*;
use crate::core::{
    HeartbeatCollector, MockPlatformServiceProvider, PlatformServiceProvider, ResourceController,
    ResourceRegistry,
};

fn controller(setup_managed: Option<bool>) -> AwsRemoteStackManagementController {
    AwsRemoteStackManagementController {
        setup_managed,
        state: AwsRemoteStackManagementState::Ready,
        role_arn: Some("arn:aws:iam::123456789012:role/test-stack-management".to_string()),
        role_name: Some("test-stack-management".to_string()),
        management_permissions_applied: true,
        applied_management_grant_fingerprint: None,
        _internal_stay_count: None,
    }
}

#[test]
fn legacy_and_explicit_ownership_are_preserved() {
    assert!(
        !controller(None).setup_managed_resources(),
        "ambiguous legacy checkpoints must retain their original runtime ownership"
    );
    assert!(!controller(Some(false)).setup_managed_resources());
    assert!(controller(Some(true)).setup_managed_resources());
}

#[tokio::test]
async fn setup_managed_lifecycle_never_calls_cloud_apis() {
    let provider: Arc<dyn PlatformServiceProvider> = Arc::new(MockPlatformServiceProvider::new());
    let management = RemoteStackManagement::new("management".to_string()).build();
    let desired_config = Resource::new(management);
    let state = StackState::new(Platform::Aws);
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
        platform: Platform::Aws,
        client_config: ClientConfig::Test,
        state: &state,
        resource_prefix: "test-stack",
        registry: &registry,
        desired_stack: &stack,
        service_provider: &provider,
        deployment_config: &deployment_config,
        heartbeat_collector: HeartbeatCollector::default(),
    };
    let mut controller = controller(Some(true));

    assert!(!controller.needs_update(&ctx).expect("ownership check"));

    controller.state = AwsRemoteStackManagementState::UpdateStart;
    controller.update_start(&ctx).await.expect("update skip");

    controller.state = AwsRemoteStackManagementState::DeleteStart;
    controller.delete_start(&ctx).await.expect("delete skip");
    assert_eq!(
        controller.role_name.as_deref(),
        Some("test-stack-management"),
        "runtime teardown must leave the setup-owned role intact"
    );
}
