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
    Ai, AwsManagementConfig, ClientConfig, DeploymentConfig, InitialSetupAuthority, Key,
    ManagementConfig, PermissionProfile, PermissionSet, PermissionSetReference, Platform,
    RemoteStackManagement, Resource, ResourceEntry, ResourceLifecycle, ResourceRef, ResourceStatus,
    ServiceAccount, SetupManagement, Stack, StackResourceState, StackState, Storage,
};
use alien_permissions::PermissionContext;
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

/// A boundary installed before a catalog set gained an action still covers stacks that do not
/// use that set. A stack that uses it, through a resource of its type or a profile that names
/// it, still needs setup.
#[tokio::test]
async fn a_catalog_action_added_after_setup_requires_setup_only_where_used() {
    const ADDED: &str = "bedrock-mantle:ListModels";
    let (stack, current, config) = fixture();
    let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
    let management = || RemoteStackManagement::new("management".to_string()).build();
    // Each way a workload role can carry ai/invoke, without any of the others.
    let uses_ai = [
        (
            "an AI resource with remote access",
            Stack::new("boundary".to_string())
                .add(management(), ResourceLifecycle::Frozen)
                .add_with_remote_access(
                    Ai::new("models".to_string()).build(),
                    ResourceLifecycle::Live,
                )
                .build(),
        ),
        (
            "a stack-wide ai/invoke grant",
            Stack::new("boundary".to_string())
                .add(management(), ResourceLifecycle::Frozen)
                .permission(
                    "application",
                    PermissionProfile::new().global(["ai/invoke"]),
                )
                .build(),
        ),
        (
            "a service account that captured ai/invoke",
            Stack::new("boundary".to_string())
                .add(management(), ResourceLifecycle::Frozen)
                .add(
                    ServiceAccount::from_permission_profile(
                        "application-sa".to_string(),
                        &PermissionProfile::new().global(["ai/invoke"]),
                        |id| alien_permissions::get_permission_set(id).cloned(),
                    )
                    .unwrap(),
                    ResourceLifecycle::Frozen,
                )
                .build(),
        ),
    ];
    // A boundary installed with the current catalog covers every one of them.
    for (scenario, stack) in &uses_ai {
        validate_installed_aws_permissions_boundary(stack, &current, &client, &config)
            .unwrap_or_else(|error| panic!("{scenario} with the current boundary: {error}"));
    }

    let mut older = current.clone();
    let statements = installed(&mut older)["permissionsBoundaryDocument"]["Statement"]
        .as_array_mut()
        .unwrap();
    let mut removed = 0;
    for statement in statements.iter_mut() {
        if let Some(actions) = statement.get_mut("Action").and_then(Value::as_array_mut) {
            let before = actions.len();
            actions.retain(|action| action != ADDED);
            removed += before - actions.len();
        }
    }
    assert_eq!(removed, 1, "the full catalog boundary grants {ADDED} once");

    validate_installed_aws_permissions_boundary(&stack, &older, &client, &config)
        .expect("a stack without ai/invoke does not need the added action");
    let executor_without_ai = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
    executor_without_ai.step(older.clone()).await.unwrap();
    executor_without_ai.refresh(older.clone()).await.unwrap();

    // A service account stored with the older ai/invoke definition asks only for what that
    // definition grants, which the older boundary covers.
    let mut stored = ServiceAccount::from_permission_profile(
        "application-sa".to_string(),
        &PermissionProfile::new().global(["ai/invoke"]),
        |id| alien_permissions::get_permission_set(id).cloned(),
    )
    .unwrap();
    let mut definition = serde_json::to_value(&stored.stack_permission_sets[0]).unwrap();
    let mut stored_removed = 0;
    for entry in definition["platforms"]["aws"].as_array_mut().unwrap() {
        let actions = entry["grant"]["actions"].as_array_mut().unwrap();
        let before = actions.len();
        actions.retain(|action| action != ADDED);
        stored_removed += before - actions.len();
    }
    assert_eq!(stored_removed, 1, "today's ai/invoke grants {ADDED} once");
    stored.stack_permission_sets[0] = serde_json::from_value(definition).unwrap();
    let with_older_service_account = Stack::new("boundary".to_string())
        .add(management(), ResourceLifecycle::Frozen)
        .add(stored, ResourceLifecycle::Frozen)
        .build();
    validate_installed_aws_permissions_boundary(
        &with_older_service_account,
        &older,
        &client,
        &config,
    )
    .expect("a service account stored with the older definition needs only that definition");

    for (scenario, stack) in &uses_ai {
        let error = validate_installed_aws_permissions_boundary(stack, &older, &client, &config)
            .expect_err(scenario);
        assert_eq!(error.code, "SETUP_RERUN_REQUIRED", "{scenario}");
        assert!(
            error
                .to_string()
                .contains("does not cover this stack's runtime permissions"),
            "{scenario} must be refused for the missing action, got: {error}"
        );
        let executor = executor(stack, &config, InitialSetupAuthority::ImportedHandoff);
        assert_setup_required(&executor, &older, scenario).await;
    }
}

/// A prepared stack carries a stack-wide grant twice: in the profile, and in the service
/// account the preflight mutation rebuilds from it with today's catalog. The account is frozen,
/// so its role keeps what setup installed. A boundary installed with an older ai/invoke covers
/// that role; it does not cover one whose installed account already holds today's definition.
#[tokio::test]
async fn a_stack_wide_grant_is_judged_by_what_the_installed_service_account_carries() {
    const ADDED: &str = "bedrock-mantle:ListModels";
    let (_, current, config) = fixture();
    let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
    let profile = PermissionProfile::new().global(["ai/invoke"]);
    let account = |definition: fn(&mut Value)| {
        let mut account =
            ServiceAccount::from_permission_profile("application-sa".to_string(), &profile, |id| {
                alien_permissions::get_permission_set(id).cloned()
            })
            .unwrap();
        let mut stored = serde_json::to_value(&account.stack_permission_sets[0]).unwrap();
        definition(&mut stored);
        account.stack_permission_sets[0] = serde_json::from_value(stored).unwrap();
        account
    };
    fn today(_: &mut Value) {}
    fn before_the_action(definition: &mut Value) {
        let mut removed = 0;
        for entry in definition["platforms"]["aws"].as_array_mut().unwrap() {
            let actions = entry["grant"]["actions"].as_array_mut().unwrap();
            let before = actions.len();
            actions.retain(|action| action != "bedrock-mantle:ListModels");
            removed += before - actions.len();
        }
        assert_eq!(removed, 1, "today's ai/invoke grants the action once");
    }
    // The prepared stack, as the service account mutation leaves it.
    let prepared = Stack::new("boundary".to_string())
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(account(today), ResourceLifecycle::Frozen)
        .permission("application", profile.clone())
        .build();

    let mut older = current.clone();
    for statement in installed(&mut older)["permissionsBoundaryDocument"]["Statement"]
        .as_array_mut()
        .unwrap()
    {
        if let Some(actions) = statement.get_mut("Action").and_then(Value::as_array_mut) {
            actions.retain(|action| action != ADDED);
        }
    }
    let with_installed_account = |state: &StackState, definition: fn(&mut Value)| {
        let mut state = state.clone();
        let mut saved = StackResourceState::new_pending(
            "service-account".to_string(),
            Resource::new(account(definition)),
            Some(ResourceLifecycle::Frozen),
            vec![],
        );
        saved.status = ResourceStatus::Running;
        state.resources.insert("application-sa".to_string(), saved);
        state
    };

    let installed_before = with_installed_account(&older, before_the_action);
    validate_installed_aws_permissions_boundary(&prepared, &installed_before, &client, &config)
        .expect("the installed account carries the older ai/invoke, which the boundary covers");
    let executor_for_prepared =
        executor(&prepared, &config, InitialSetupAuthority::ImportedHandoff);
    executor_for_prepared
        .refresh(installed_before.clone())
        .await
        .unwrap();

    // A failed heartbeat does not change what the installed account carries.
    let mut heartbeat_failed = installed_before.clone();
    heartbeat_failed
        .resources
        .get_mut("application-sa")
        .unwrap()
        .status = ResourceStatus::RefreshFailed;
    validate_installed_aws_permissions_boundary(&prepared, &heartbeat_failed, &client, &config)
        .expect("a failed heartbeat keeps the installed account's grants");

    let installed_today = with_installed_account(&older, today);
    let error =
        validate_installed_aws_permissions_boundary(&prepared, &installed_today, &client, &config)
            .expect_err("an installed account with today's ai/invoke needs the action");
    assert_eq!(error.code, "SETUP_RERUN_REQUIRED");
    assert!(error
        .to_string()
        .contains("does not cover this stack's runtime permissions"));
    assert_setup_required(&executor_for_prepared, &installed_today, "today's account").await;

    // An update that changes the stack-wide grants is judged by the account it asks for, not
    // the installed one, so the older boundary still refuses it.
    let changed_profile = PermissionProfile::new().global(["ai/invoke", "storage/data-read"]);
    let changed = Stack::new("boundary".to_string())
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::from_permission_profile(
                "application-sa".to_string(),
                &changed_profile,
                |id| alien_permissions::get_permission_set(id).cloned(),
            )
            .unwrap(),
            ResourceLifecycle::Frozen,
        )
        .permission("application", changed_profile.clone())
        .build();
    let error =
        validate_installed_aws_permissions_boundary(&changed, &installed_before, &client, &config)
            .expect_err("changed stack-wide grants are judged as desired");
    assert!(error
        .to_string()
        .contains("does not cover this stack's runtime permissions"));
    assert_setup_required(
        &executor(&changed, &config, InitialSetupAuthority::ImportedHandoff),
        &installed_before,
        "changed stack-wide grants",
    )
    .await;

    // With the current boundary, both installed accounts are covered.
    for definition in [today as fn(&mut Value), before_the_action] {
        validate_installed_aws_permissions_boundary(
            &prepared,
            &with_installed_account(&current, definition),
            &client,
            &config,
        )
        .unwrap();
    }
}

// A stack can define its own grant and give it to a service account, like a role the account
// assumes elsewhere. Setup's boundary renders only the catalog, so it has never carried such a
// grant, and the check must not require it: every later release of the stack would otherwise
// need setup again.
#[tokio::test]
async fn a_stack_defined_account_grant_is_not_required_of_the_boundary() {
    let (_, current, config) = fixture();
    let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
    let defined: PermissionSet = serde_json::from_value(json!({
        "id": "custom/assume-target-role",
        "description": "Assumes the management role in target accounts",
        "platforms": {
            "aws": [{
                "grant": { "actions": ["sts:AssumeRole"] },
                "binding": { "stack": { "resources": ["arn:aws:iam::*:role/*-management"] } }
            }]
        }
    }))
    .unwrap();
    assert!(alien_permissions::get_permission_set(&defined.id).is_none());
    let account = ServiceAccount::new("target-sa".to_string())
        .stack_permission_set(defined)
        .build();
    let profile = PermissionProfile::new().resource("target-sa", ["service-account/impersonate"]);
    let stack = Stack::new("boundary".to_string())
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(account.clone(), ResourceLifecycle::Frozen)
        .permission("manager", profile)
        .build();

    // Setup installed the account and rendered the boundary for this stack.
    let mut state = current.clone();
    let mut saved = StackResourceState::new_pending(
        "service-account".to_string(),
        Resource::new(account),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    saved.status = ResourceStatus::Running;
    state.resources.insert("target-sa".to_string(), saved);
    let context = permission_context(&state.resource_prefix);
    installed(&mut state)["permissionsBoundaryDocument"] =
        aws_permissions_boundary_for_stack(&stack, &state, &context).unwrap();
    assert!(
        !installed(&mut state)["permissionsBoundaryDocument"]
            .to_string()
            .contains("sts:AssumeRole"),
        "setup's boundary renders only catalog grants"
    );

    validate_installed_aws_permissions_boundary(&stack, &state, &client, &config)
        .expect("a stack-defined account grant is not required of the boundary");
    let executor = executor(&stack, &config, InitialSetupAuthority::ImportedHandoff);
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

/// An update that has finished leaves the account it replaced in `previous_config`. The role
/// carries what the update applied, so the check judges `config`; the replaced definition,
/// here a stack-wide `postgres/data-access` stored before its read gained the stack tag
/// condition, must not be asked of the boundary. While an update is in flight or after it
/// failed, the role may still carry the replaced definition, and that one is judged.
#[tokio::test]
async fn a_running_account_is_judged_by_its_config_and_an_unfinished_update_by_the_one_it_replaced()
{
    let (_, current, config) = fixture();
    let client = ClientConfig::Aws(Box::new(AwsClientConfig::mock()));
    let profile = PermissionProfile::new().global(["postgres/data-access"]);
    let account = |definition: fn(&mut Value)| {
        let mut account =
            ServiceAccount::from_permission_profile("application-sa".to_string(), &profile, |id| {
                alien_permissions::get_permission_set(id).cloned()
            })
            .unwrap();
        let mut stored = serde_json::to_value(&account.stack_permission_sets[0]).unwrap();
        definition(&mut stored);
        account.stack_permission_sets[0] = serde_json::from_value(stored).unwrap();
        account
    };
    fn today(_: &mut Value) {}
    fn before_the_tag_condition(definition: &mut Value) {
        let mut removed = 0;
        for entry in definition["platforms"]["aws"].as_array_mut().unwrap() {
            for scope in ["stack", "resource"] {
                let binding = entry["binding"][scope].as_object_mut().unwrap();
                removed += usize::from(binding.remove("condition").is_some());
            }
        }
        assert_eq!(removed, 2, "today's read is conditioned in both scopes");
    }
    let prepared = Stack::new("boundary".to_string())
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(account(today), ResourceLifecycle::Frozen)
        .permission("application", profile.clone())
        .build();
    let with_account =
        |status: ResourceStatus, config: fn(&mut Value), previous: Option<fn(&mut Value)>| {
            let mut state = current.clone();
            let mut saved = StackResourceState::new_pending(
                "service-account".to_string(),
                Resource::new(account(config)),
                Some(ResourceLifecycle::Frozen),
                vec![],
            );
            saved.status = status;
            saved.previous_config = previous.map(|previous| Resource::new(account(previous)));
            state.resources.insert("application-sa".to_string(), saved);
            state
        };
    let executor = executor(&prepared, &config, InitialSetupAuthority::ImportedHandoff);

    // The replaced definition is outside the installed boundary.
    let applied_before = with_account(ResourceStatus::Running, before_the_tag_condition, None);
    let error =
        validate_installed_aws_permissions_boundary(&prepared, &applied_before, &client, &config)
            .expect_err("the boundary allows the secret read only with the stack tag condition");
    assert!(error
        .to_string()
        .contains("does not cover this stack's runtime permissions"));

    // The update to today's definition finished, and its replaced account stayed behind.
    for status in [ResourceStatus::Running, ResourceStatus::RefreshFailed] {
        let finished = with_account(status, today, Some(before_the_tag_condition));
        validate_installed_aws_permissions_boundary(&prepared, &finished, &client, &config)
            .unwrap_or_else(|error| {
                panic!("a {status:?} account is judged by what it applied: {error}")
            });
    }
    let finished = with_account(
        ResourceStatus::Running,
        today,
        Some(before_the_tag_condition),
    );
    executor.step(finished.clone()).await.unwrap();
    executor.refresh(finished).await.unwrap();

    // An update in flight or failed may not have replaced the role yet.
    for status in [ResourceStatus::Updating, ResourceStatus::UpdateFailed] {
        let unfinished = with_account(status, today, Some(before_the_tag_condition));
        let error =
            validate_installed_aws_permissions_boundary(&prepared, &unfinished, &client, &config)
                .expect_err("an unfinished update is judged by the account it replaced");
        assert_eq!(error.code, "SETUP_RERUN_REQUIRED", "{status:?}");
        assert!(
            error
                .to_string()
                .contains("does not cover this stack's runtime permissions"),
            "{status:?}: {error}"
        );
    }
}
