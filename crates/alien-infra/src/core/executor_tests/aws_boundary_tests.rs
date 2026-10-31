//! The installed ceiling is checked before any runtime controller can call a provider.

use super::helpers::default_deployment_config;
use crate::{
    core::{
        aws_permissions_boundary_for_stack, validate_installed_aws_permissions_boundary,
        MockPlatformServiceProvider, StackExecutor, StackResourceStateExt,
    },
    key::AwsKeyController,
    remote_stack_management::AwsRemoteStackManagementController,
    service_account::AwsServiceAccountController,
};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};
use alien_core::{
    AwsManagementConfig, ClientConfig, DeploymentConfig, InitialSetupAuthority, Key,
    ManagementConfig, PermissionProfile, PermissionSetReference, Platform, RemoteStackManagement,
    Resource, ResourceEntry, ResourceLifecycle, ResourceRef, ResourceStatus, ServiceAccount,
    SetupManagement, Stack, StackResourceState, StackState, Storage,
};
use alien_permissions::{generators::aws_permissions_boundary_covers, PermissionContext};
use serde_json::{json, Value};
use std::sync::Arc;

const ARN: &str = "arn:aws:iam::123456789012:policy/boundary-test-deployment-management-boundary";

fn permission_context(prefix: &str) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix(prefix)
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
        .with_managing_account_id("210987654321")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager")
}

fn fixture() -> (Stack, StackState, DeploymentConfig) {
    let management = RemoteStackManagement::new("management".to_string()).build();
    let stack = Stack::new("boundary".to_string())
        .add(management.clone(), ResourceLifecycle::Frozen)
        .build();
    let mut state = StackState::new(Platform::Aws);
    state.resource_prefix = "boundary-test".to_string();
    let context = permission_context(&state.resource_prefix);
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

async fn assert_setup_required(executor: &StackExecutor, state: &StackState, scenario: &str) {
    for refresh in [false, true] {
        let result = if refresh {
            executor.refresh(state.clone()).await
        } else {
            executor.step(state.clone()).await
        };
        let error = result.err().unwrap_or_else(|| {
            panic!("{scenario}: must reject before provider access (refresh={refresh})")
        });
        assert_eq!(
            error.code, "SETUP_RERUN_REQUIRED",
            "{scenario}, refresh={refresh}"
        );
        assert!(!error.retryable);
        assert!(error.to_string().contains("rerun setup"));
    }
}

#[tokio::test]
async fn invalid_installed_boundaries_stop_step_and_refresh() {
    let (_, state, config) = fixture();
    let valid = state.resources["management"]
        .internal_state
        .clone()
        .unwrap();
    let mut wrong_controller = state.resources["management"].clone();
    wrong_controller
        .set_internal_controller(Some(Box::new(
            crate::storage::AwsStorageController::default(),
        )))
        .unwrap();
    let mut cases = vec![
        ("missing controller", None),
        ("wrong controller type", wrong_controller.internal_state),
    ];
    for (name, document) in [
        ("missing document", Value::Null),
        ("malformed document", json!({"Statement": "invalid"})),
        (
            "insufficient document",
            json!({"Version": "2012-10-17", "Statement": []}),
        ),
    ] {
        let mut controller = valid.clone();
        controller["permissionsBoundaryDocument"] = document;
        cases.push((name, Some(controller)));
    }
    // Even an otherwise sufficient document must reject unresolved extra grants.
    for (name, extra) in [
        (
            "unresolved resource",
            json!({"Effect": "Allow", "Action": "s3:GetObject",
            "Resource": "arn:aws:s3:::${local.resource_prefix}-*"}),
        ),
        (
            "unresolved condition",
            json!({"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*",
            "Condition": {"StringEquals": {"aws:RequestedRegion": "${AWS::Region}"}}}),
        ),
        (
            "object condition",
            json!({"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*",
            "Condition": {"StringEquals": {"aws:RequestedRegion": {"Ref": "Region"}}}}),
        ),
        (
            "null condition",
            json!({"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*",
            "Condition": {"StringEquals": {"aws:RequestedRegion": null}}}),
        ),
    ] {
        let mut controller = valid.clone();
        controller["permissionsBoundaryDocument"]["Statement"]
            .as_array_mut()
            .unwrap()
            .push(extra);
        cases.push((name, Some(controller)));
    }
    // This bucket would call the provider if validation happened after planning.
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
    let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
    for (name, controller) in cases {
        let mut state = state.clone();
        state
            .resources
            .get_mut("management")
            .unwrap()
            .internal_state = controller;
        assert_setup_required(&executor, &state, name).await;
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

// Keep these old controller documents literal: serializing today's controller would
// silently add defaults and stop testing whether old persisted records still load.
#[tokio::test]
async fn legacy_management_records_load_and_continue_without_setup() {
    for revision in [None, Some("old-management-revision")] {
        let (mut stack, mut state, config) = fixture();
        let mut old = json!({
            "type": "AwsRemoteStackManagementController",
            "state": "ready",
            "roleArn": "arn:aws:iam::123456789012:role/boundary-test-management",
            "roleName": "boundary-test-management",
            "internalStayCount": null,
            "_controllerStateVersion": 1,
            "managementPermissionsApplied": true
        });
        if let Some(revision) = revision {
            old["managementPermissionsRevision"] = json!(revision);
            // Exercise a populated direct-setup state as well as the minimal import.
            for index in 0..29 {
                let id = format!("objects-{index}");
                let resource = Storage::new(id.clone()).build();
                stack.resources.insert(
                    id.clone(),
                    ResourceEntry::new(resource.clone(), ResourceLifecycle::Frozen),
                );
                let mut saved = StackResourceState::new_pending(
                    "storage".to_string(),
                    Resource::new(resource),
                    Some(ResourceLifecycle::Frozen),
                    vec![],
                );
                saved.status = ResourceStatus::Running;
                saved
                    .set_internal_controller(Some(Box::new(
                        crate::storage::AwsStorageController::mock_ready(&id),
                    )))
                    .unwrap();
                state.resources.insert(id, saved);
            }
            assert_eq!(state.resources.len(), 30);
        }
        *installed(&mut state) = old.clone();
        // Include the durable retry checkpoint in version and serde validation.
        state
            .resources
            .get_mut("management")
            .unwrap()
            .last_failed_state = Some(old);
        let state: StackState =
            serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
        let controller = state.resources["management"]
            .get_internal_controller_typed::<AwsRemoteStackManagementController>()
            .unwrap();
        assert!(controller.permissions_boundary_arn.is_none());
        assert!(controller.permissions_boundary_document.is_none());
        assert_eq!(
            controller.management_permissions_revision.as_deref(),
            revision
        );
        assert!(controller.management_permissions_applied);
        state.resources["management"]
            .get_last_failed_controller()
            .unwrap()
            .unwrap();
        validate_installed_aws_permissions_boundary(
            &stack,
            &state,
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            &config,
        )
        .unwrap();
        let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
        for result in [
            executor.step(state.clone()).await,
            executor.refresh(state.clone()).await,
        ] {
            let next = result.unwrap().next_state;
            assert_eq!(next.resources.len(), state.resources.len());
            assert!(next
                .resources
                .values()
                .all(|saved| saved.status == ResourceStatus::Running));
            let controller = next.resources["management"]
                .get_internal_controller_typed::<AwsRemoteStackManagementController>()
                .unwrap();
            assert!(controller.permissions_boundary_arn.is_none());
            assert_eq!(
                controller.role_arn.as_deref(),
                Some("arn:aws:iam::123456789012:role/boundary-test-management")
            );
        }
    }
}

#[tokio::test]
async fn empty_legacy_automatic_install_does_not_require_a_boundary() {
    let (stack, mut state, mut config) = fixture();
    state.resources.clear();
    config.stack_settings.setup_management = SetupManagement::Automatic;
    validate_installed_aws_permissions_boundary(
        &stack,
        &state,
        &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
        &config,
    )
    .unwrap();
    let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
    // Observation before the first setup import must not invent an installed boundary.
    assert!(executor
        .refresh(state)
        .await
        .unwrap()
        .next_state
        .resources
        .is_empty());
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
async fn key_grants_require_the_installed_boundary_to_cover_the_imported_arn() {
    let (_, mut state, config) = fixture();
    let key_arn = "arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555";
    let key = Key::new("encryption".to_string()).build();
    let stack = Stack::new("boundary".to_string())
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(key.clone(), ResourceLifecycle::Frozen)
        .permission(
            "application",
            PermissionProfile::new().resource("encryption", ["key/native-storage"]),
        )
        .build();
    let mut saved = StackResourceState::new_pending(
        "key".to_string(),
        Resource::new(key),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    saved.status = ResourceStatus::Running;
    saved
        .set_internal_controller(Some(Box::new(AwsKeyController {
            key_arn: Some(key_arn.to_string()),
            ..Default::default()
        })))
        .unwrap();
    state.resources.insert("encryption".to_string(), saved);
    let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
    let original_document = installed(&mut state)["permissionsBoundaryDocument"].clone();
    // Setup approves the exact resource ceiling, not only today's identity actions.
    for grant in [
        None,
        Some(json!({
            "Effect": "Allow", "NotAction": "iam:*",
            "Resource": "arn:aws:kms:us-east-1:123456789012:key/sibling"
        })),
        Some(json!({
            "Effect": "Allow", "Action": ["kms:Decrypt", "kms:GenerateDataKey"],
            "Resource": key_arn
        })),
    ] {
        installed(&mut state)["permissionsBoundaryDocument"] = original_document.clone();
        if let Some(grant) = grant {
            installed(&mut state)["permissionsBoundaryDocument"]["Statement"]
                .as_array_mut()
                .unwrap()
                .push(grant);
        }
        assert_setup_required(&executor, &state, "exact key ceiling").await;
    }

    let approved = aws_permissions_boundary_for_stack(
        &stack,
        &state,
        &permission_context(&state.resource_prefix),
    )
    .unwrap();
    installed(&mut state)["permissionsBoundaryDocument"] = approved;
    executor.step(state.clone()).await.unwrap();
    executor.refresh(state.clone()).await.unwrap();

    // The saved setup approval cannot authorize a replacement imported key.
    state
        .resources
        .get_mut("encryption")
        .unwrap()
        .set_internal_controller(Some(Box::new(AwsKeyController {
            key_arn: Some("arn:aws:kms:us-east-1:123456789012:key/replacement".to_string()),
            ..Default::default()
        })))
        .unwrap();
    assert_setup_required(&executor, &state, "replacement key").await;
}

/// A template install made before service accounts depended on management records no such
/// edge. Its first release only records the edge: the setup-owned role is not touched.
#[tokio::test]
async fn template_installed_service_accounts_take_the_management_edge_without_iam() {
    let (mut stack, mut state, config) = fixture();
    installed(&mut state)["permissionsBoundaryArn"] = Value::Null;
    installed(&mut state)["permissionsBoundaryDocument"] = Value::Null;
    let edge = vec![ResourceRef::new(
        RemoteStackManagement::RESOURCE_TYPE,
        "management",
    )];
    for id in ["execution-sa", "build-execution-sa"] {
        let account = ServiceAccount::new(id.to_string()).build();
        let mut entry = ResourceEntry::new(account.clone(), ResourceLifecycle::Frozen);
        entry.dependencies = edge.clone();
        stack.resources.insert(id.to_string(), entry);
        let mut saved = StackResourceState::new_pending(
            "service-account".to_string(),
            Resource::new(account),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        saved.status = ResourceStatus::Running;
        saved
            .set_internal_controller(Some(Box::new(AwsServiceAccountController::mock_ready(
                &format!("boundary-test-{id}"),
            ))))
            .unwrap();
        state.resources.insert(id.to_string(), saved);
    }
    // The provider mock has no expectations, so any IAM client request fails the test.
    let executor =
        StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
            .deployment_config(&config)
            .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
            .step_running_resources(false)
            .service_provider(Arc::new(MockPlatformServiceProvider::new()))
            .build()
            .unwrap();
    assert!(!executor.is_synced(&state));
    let plan = executor.plan(&state).unwrap();
    assert!(plan.updates.is_empty(), "{:?}", plan.updates.keys());
    assert_eq!(
        plan.dependency_records
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        ["build-execution-sa", "execution-sa"].into(),
    );
    let state = executor.step(state).await.unwrap().next_state;
    assert!(
        executor.is_synced(&state),
        "{:?}",
        state
            .resources
            .iter()
            .map(|(id, r)| (id, r.status, &r.error, &r.dependencies))
            .collect::<Vec<_>>()
    );
    for id in ["execution-sa", "build-execution-sa"] {
        let account = &state.resources[id];
        assert_eq!(
            account.status,
            ResourceStatus::Running,
            "{id}: {:?}",
            account.error
        );
        assert_eq!(account.dependencies, edge, "{id}");
        assert_eq!(account.config, stack.resources[id].config, "{id}");
    }
    let replanned = executor.plan(&state).unwrap();
    assert!(replanned.updates.is_empty() && replanned.dependency_records.is_empty());
}

/// Profiles with least-privilege grants on prefixed resources: two consumers of secret
/// parameters and a Deny that protects one of them. Before admitted grants were skipped, the
/// renderer copied these into the boundary.
fn prefixed_grant_profiles(stack: &mut Stack) {
    let inline = |id: &str, effect: &str, actions: &[&str], resources: &[&str]| {
        PermissionSetReference::from_inline(
            serde_json::from_value(json!({
                "id": id, "description": "Prefixed grant",
                "platforms": {"aws": [{"effect": effect, "grant": {"actions": actions},
                    "binding": {"stack": {"resources": resources}}}]}
            }))
            .unwrap(),
        )
    };
    let parameter = |name: &str| {
        format!("arn:aws:ssm:${{awsRegion}}:${{awsAccountId}}:parameter/${{stackPrefix}}-{name}")
    };
    for (profile, names) in [
        ("first", vec!["secret-one", "secret-two"]),
        ("second", vec!["secret-two"]),
    ] {
        let resources: Vec<_> = names.iter().map(|name| parameter(name)).collect();
        let resources: Vec<_> = resources.iter().map(String::as_str).collect();
        let mut grants = vec![inline(
            "custom/secret-read",
            "Allow",
            &["ssm:GetParameter"],
            &resources,
        )];
        if profile == "first" {
            grants.push(inline(
                "custom/protect-config",
                "Deny",
                &["ssm:*"],
                &[&parameter("config")],
            ));
        }
        let mut permissions = PermissionProfile::new();
        permissions.0.insert("*".to_string(), grants);
        stack
            .permissions
            .profiles
            .insert(profile.to_string(), permissions);
    }
}

/// A setup installed before admitted grants were skipped holds a larger document with those
/// grants copied in. It still covers the stack, so runtime continues without a setup rerun.
#[tokio::test]
async fn boundaries_installed_with_copied_prefixed_grants_still_cover_the_stack() {
    let (mut stack, mut state, config) = fixture();
    prefixed_grant_profiles(&mut stack);
    let required =
        aws_permissions_boundary_for_stack(&stack, &state, &permission_context("boundary-test"))
            .unwrap();
    let mut statements = required["Statement"].as_array().unwrap().clone();
    assert!(
        !required.to_string().contains("ssm:GetParameter"),
        "admitted grants are no longer copied"
    );
    // Verbatim from the earlier renderer for this stack, in its order around the inline Deny.
    // The rest of its document matches today's.
    let protect = statements
        .iter()
        .position(|statement| statement["Action"] == "ssm:*")
        .unwrap();
    statements.insert(
        protect + 1,
        json!({"Effect": "Allow", "Action": "ssm:GetParameter",
            "Resource": "arn:aws:ssm:us-east-1:123456789012:parameter/boundary-test-secret-two"}),
    );
    statements.insert(
        protect,
        json!({"Effect": "Allow", "Action": "ssm:GetParameter", "Resource": [
            "arn:aws:ssm:us-east-1:123456789012:parameter/boundary-test-secret-one",
            "arn:aws:ssm:us-east-1:123456789012:parameter/boundary-test-secret-two"]}),
    );
    let earlier = json!({"Version": "2012-10-17", "Statement": statements});
    installed(&mut state)["permissionsBoundaryDocument"] = earlier;

    validate_installed_aws_permissions_boundary(
        &stack,
        &state,
        &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
        &config,
    )
    .unwrap();
    let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
    executor.step(state.clone()).await.unwrap();
    executor.refresh(state).await.unwrap();
}

/// A setup installed before scoped patterns were merged holds each catalog pattern on its own.
/// It does not cover today's wider merged patterns, but it covers everything the stack's
/// runtime needs, so runtime continues without a setup rerun.
#[tokio::test]
async fn boundaries_installed_before_scoped_patterns_were_merged_still_cover_the_stack() {
    let (stack, mut state, config) = fixture();
    let current =
        aws_permissions_boundary_for_stack(&stack, &state, &permission_context("boundary-test"))
            .unwrap();
    let mut earlier = current.clone();
    let scoped = earlier["Statement"][0]["Resource"].as_array_mut().unwrap();
    let region = "arn:aws:codebuild:us-east-1:123456789012";
    let logs = "arn:aws:logs:us-east-1:123456789012:log-group";
    for (merged, members) in [
        (
            format!("{region}:*/boundary-test-*"),
            vec![
                format!("{region}:project/boundary-test-*"),
                format!("{region}:report-group/boundary-test-*"),
            ],
        ),
        (
            format!("{logs}:/aws/*/boundary-test-*"),
            vec![
                format!("{logs}:/aws/codebuild/boundary-test-*:*"),
                format!("{logs}:/aws/lambda/boundary-test-*"),
            ],
        ),
    ] {
        let position = scoped
            .iter()
            .position(|pattern| pattern == &json!(merged))
            .unwrap_or_else(|| panic!("{merged} is not in {current}"));
        scoped.remove(position);
        scoped.extend(members.into_iter().map(Value::from));
    }
    scoped.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    assert!(!aws_permissions_boundary_covers(&earlier, &current).unwrap());
    installed(&mut state)["permissionsBoundaryDocument"] = earlier;

    validate_installed_aws_permissions_boundary(
        &stack,
        &state,
        &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
        &config,
    )
    .unwrap();
    let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
    executor.step(state.clone()).await.unwrap();
    executor.refresh(state).await.unwrap();
}
