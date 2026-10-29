//! The installed ceiling is checked before any runtime controller can call a provider.

use super::helpers::default_deployment_config;
use crate::{
    core::{
        aws_permissions_boundary_for_stack, MockPlatformServiceProvider, StackExecutor,
        StackResourceStateExt,
    },
    remote_stack_management::AwsRemoteStackManagementController,
};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};
use alien_core::{
    AwsManagementConfig, ClientConfig, DeploymentConfig, InitialSetupAuthority, ManagementConfig,
    PermissionProfile, PermissionSetReference, Platform, RemoteStackManagement, Resource,
    ResourceLifecycle, ResourceStatus, Stack, StackResourceState, StackState, Storage,
};
use alien_permissions::PermissionContext;
use serde_json::{json, Value};
use std::sync::Arc;

const ARN: &str = "arn:aws:iam::123456789012:policy/boundary-test-permissions-boundary";

fn fixture() -> (Stack, StackState, DeploymentConfig) {
    let management = RemoteStackManagement::new("management".to_string()).build();
    let stack = Stack::new("boundary".to_string())
        .add(management.clone(), ResourceLifecycle::Frozen)
        .build();
    let mut state = StackState::new(Platform::Aws);
    state.resource_prefix = "boundary-test".to_string();
    let context = PermissionContext::new()
        .with_stack_prefix(state.resource_prefix.clone())
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
        .with_managing_account_id("210987654321")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager");
    let mut controller = AwsRemoteStackManagementController::mock_ready("management");
    controller.permissions_boundary_arn = Some(ARN.to_string());
    controller.permissions_boundary_document =
        Some(aws_permissions_boundary_for_stack(&stack, &state, &context).unwrap());
    let mut saved = StackResourceState::new_pending(
        "remote-stack-management".to_string(),
        Resource::new(management),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    saved.status = ResourceStatus::Running;
    saved
        .set_internal_controller(Some(Box::new(controller)))
        .unwrap();
    state.resources.insert("management".to_string(), saved);
    let mut config = default_deployment_config();
    config.management_config = Some(ManagementConfig::Aws(AwsManagementConfig {
        managing_role_arn: "arn:aws:iam::210987654321:role/manager".to_string(),
    }));
    (stack, state, config)
}

fn executor(
    stack: &Stack,
    config: &DeploymentConfig,
    authority: InitialSetupAuthority,
) -> StackExecutor {
    StackExecutor::builder(stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
        .deployment_config(config)
        .initial_setup_authority(authority)
        .step_running_resources(false)
        .step_out_of_scope_resources(false)
        .lifecycle_filter(vec![ResourceLifecycle::Live])
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .build()
        .unwrap()
}

fn installed(state: &mut StackState) -> &mut Value {
    state
        .resources
        .get_mut("management")
        .unwrap()
        .internal_state
        .as_mut()
        .unwrap()
}

#[tokio::test]
async fn missing_malformed_and_insufficient_documents_stop_step_and_refresh() {
    for document in [
        None,
        Some(json!({"Statement": "invalid"})),
        Some(json!({
            "Version": "2012-10-17", "Statement": []
        })),
    ] {
        let (_, mut state, config) = fixture();
        // This would create a bucket if the check happened after stepping controllers.
        let stack = Stack::new("boundary".to_string())
            .add(
                Storage::new("objects".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .add(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        installed(&mut state)["permissionsBoundaryDocument"] = document.unwrap_or(Value::Null);
        let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
        for refresh in [false, true] {
            let result = if refresh {
                executor.refresh(state.clone()).await
            } else {
                executor.step(state.clone()).await
            };
            let error = result.err().expect("must reject before provider access");
            assert_eq!(error.code, "SETUP_RERUN_REQUIRED");
            assert!(!error.retryable);
            assert!(error.to_string().contains("rerun setup"));
        }
    }
}

#[tokio::test]
async fn covered_and_legacy_deployments_continue_without_provider_calls() {
    for legacy in [false, true] {
        let (stack, mut state, config) = fixture();
        if legacy {
            installed(&mut state)["permissionsBoundaryArn"] = Value::Null;
            installed(&mut state)["permissionsBoundaryDocument"] = Value::Null;
        }
        let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
        executor.step(state.clone()).await.unwrap();
        executor.refresh(state).await.unwrap();
    }
}

#[tokio::test]
async fn new_external_grant_requires_setup_until_installed_document_is_replaced() {
    let (mut stack, mut state, config) = fixture();
    let grant = PermissionSetReference::from_inline(
        serde_json::from_value(json!({
            "id": "custom/external", "description": "Approved archive",
            "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]}, "binding": {
                "stack": {"resources": ["arn:aws:s3:::approved-archive/*"],
                    "condition": {"StringEquals": {"s3:ExistingObjectTag/approved": "yes"}}}
            }}]}
        }))
        .unwrap(),
    );
    let mut profile = PermissionProfile::new();
    profile.0.insert("*".to_string(), vec![grant]);
    stack
        .permissions
        .profiles
        .insert("reader".to_string(), profile);
    let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
    assert_eq!(
        executor.step(state.clone()).await.err().unwrap().code,
        "SETUP_RERUN_REQUIRED"
    );
    installed(&mut state)["permissionsBoundaryDocument"]["Statement"].as_array_mut().unwrap().push(json!({
        "Effect": "Allow", "Action": "s3:GetObject", "Resource": "arn:aws:s3:::approved-archive/*",
        "Condition": {"StringEquals": {"s3:ExistingObjectTag/approved": "yes"}}
    }));
    executor.step(state.clone()).await.unwrap();
    executor.refresh(state).await.unwrap();
}

#[tokio::test]
async fn setup_repair_and_full_teardown_do_not_require_new_runtime_permissions() {
    let (stack, mut state, config) = fixture();
    installed(&mut state)["permissionsBoundaryDocument"] = Value::Null;
    executor(&stack, &config, InitialSetupAuthority::DirectSetup)
        .step(state.clone())
        .await
        .unwrap();
    let empty = Stack::new("delete".to_string()).build();
    executor(&empty, &config, InitialSetupAuthority::ImportedHandoff)
        .step(state)
        .await
        .unwrap();
}

#[tokio::test]
async fn missing_controller_state_cannot_disable_the_check() {
    let (stack, mut state, config) = fixture();
    state
        .resources
        .get_mut("management")
        .unwrap()
        .internal_state = None;
    let error = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff)
        .step(state)
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "SETUP_RERUN_REQUIRED");
}

#[tokio::test]
async fn wrong_controller_type_cannot_look_like_an_unbounded_setup() {
    let (stack, mut state, config) = fixture();
    state
        .resources
        .get_mut("management")
        .unwrap()
        .set_internal_controller(Some(Box::new(
            crate::storage::AwsStorageController::default(),
        )))
        .unwrap();
    let error = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff)
        .step(state)
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "SETUP_RERUN_REQUIRED");
}

#[tokio::test]
async fn unresolved_or_malformed_extra_grants_stop_before_provider_access() {
    for extra in [
        json!({"Effect": "Allow", "Action": "s3:GetObject",
            "Resource": "arn:aws:s3:::${local.resource_prefix}-*"}),
        json!({"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*",
            "Condition": {"StringEquals": {"aws:RequestedRegion": "${AWS::Region}"}}}),
        json!({"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*",
            "Condition": {"StringEquals": {"aws:RequestedRegion": {"Ref": "Region"}}}}),
        json!({"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*",
            "Condition": {"StringEquals": {"aws:RequestedRegion": null}}}),
    ] {
        let (_, mut state, config) = fixture();
        let stack = Stack::new("boundary".to_string())
            .add(
                Storage::new("objects".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .add(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        // The valid catalog grants already cover this stack. A malformed extra
        // statement must still invalidate the recorded document before planning.
        installed(&mut state)["permissionsBoundaryDocument"]["Statement"]
            .as_array_mut()
            .unwrap()
            .push(extra);
        let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
        for refresh in [false, true] {
            let error = if refresh {
                executor.refresh(state.clone()).await
            } else {
                executor.step(state.clone()).await
            }
            .err()
            .expect("must reject before provider access");
            assert_eq!(error.code, "SETUP_RERUN_REQUIRED");
            assert!(!error.retryable);
        }
    }
}
