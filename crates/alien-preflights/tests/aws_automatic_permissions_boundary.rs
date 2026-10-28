//! Exercise the automatic profile produced by preflight, not just individual catalog sets.
#[path = "../../alien-permissions/tests/iam_policy/mod.rs"]
mod iam_policy;

use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform, ResourceLifecycle,
    SetupManagement, Stack, StackSettings, StackState, Storage, Vault, Worker, WorkerCode,
};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, management_identity_global_refs, BindingTarget, PermissionContext,
};
use alien_preflights::{mutations::ManagementPermissionProfileMutation, StackMutation};
use iam_policy::{evaluate, wildcard_match, Decision, Request};
use serde_json::json;
use std::collections::BTreeSet;

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/sample-permissions-boundary";
const ROLE: &str = "arn:aws:iam::123456789012:role/sample-jobs";
const WRITES: &[&str] = &[
    "iam:CreateRole",
    "iam:PutRolePolicy",
    "iam:AttachRolePolicy",
    "iam:UpdateAssumeRolePolicy",
    "iam:PutRolePermissionsBoundary",
    "iam:DeleteRolePolicy",
    "iam:DetachRolePolicy",
];

fn compile(stack: &Stack, bounded: bool) -> Vec<AwsIamStatement> {
    let mut context = PermissionContext::new()
        .with_stack_prefix("sample")
        .with_aws_region("us-east-1")
        .with_aws_account_id("123456789012")
        .with_managing_account_id("210987654321")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager");
    if bounded {
        context = context.with_aws_permissions_boundary_arn(BOUNDARY);
    }
    let profile = stack.management().profile().unwrap();
    // Automatic AWS setup must grant everything up front; it cannot write its own role later.
    assert!(profile.0.keys().all(|scope| scope == "*"));
    management_identity_global_refs(stack.resources.values(), profile)
        .into_iter()
        .flat_map(|reference| {
            let set = reference
                .resolve(|name| get_permission_set(name).cloned())
                .unwrap();
            AwsRuntimePermissionsGenerator::new()
                .generate_policy(&set, BindingTarget::Stack, &context)
                .unwrap()
                .statement
        })
        .collect()
}

#[tokio::test]
async fn automatic_profile_bounds_every_role_writer_and_cannot_change_the_boundary() {
    let stack = Stack::new("sample".to_string())
        .add(
            Storage::new("uploads".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(
            Vault::new(alien_core::DEPLOYER_SECRETS_VAULT_ID.to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Worker::new("jobs".to_string())
                .code(WorkerCode::Image {
                    image: "test:latest".to_string(),
                })
                .permissions("default".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let settings = StackSettings {
        setup_management: SetupManagement::Automatic,
        ..StackSettings::default()
    };
    let config = DeploymentConfig::builder()
        .stack_settings(settings)
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let stack = ManagementPermissionProfileMutation
        .mutate(stack, &StackState::new(Platform::Aws), &config)
        .await
        .unwrap();
    let ids: BTreeSet<_> = stack.management().profile().unwrap().0["*"]
        .iter()
        .map(|reference| reference.id())
        .collect();
    // Automatic mode includes writers even for resource types absent from this release.
    for id in [
        "service-account/provision",
        "artifact-registry/provision",
        "artifact-registry/management",
        "worker/provision",
        "ai/provision",
        "container/provision",
    ] {
        assert!(ids.contains(id), "missing automatic grant: {id}");
    }
    let bounded = compile(&stack, true);
    let mut observed_writes = BTreeSet::new();
    for statement in bounded
        .iter()
        .filter(|statement| statement.effect == "Allow")
    {
        for action in &statement.action {
            let matches_action = |write: &str| {
                wildcard_match(&action.to_ascii_lowercase(), &write.to_ascii_lowercase())
            };
            assert!(
                !matches_action("iam:DeleteRolePermissionsBoundary"),
                "{statement:?}"
            );
            for write in WRITES.iter().filter(|write| matches_action(write)) {
                observed_writes.insert(*write);
                if statement.action == ["iam:DetachRolePolicy"]
                    && statement.resource == ["arn:aws:iam::123456789012:role/sample-dlm-*"]
                {
                    assert_eq!(
                        json!(statement.condition),
                        json!({"StringEquals": {"iam:PolicyARN": "arn:aws:iam::aws:policy/service-role/AWSDataLifecycleManagerServiceRole"}})
                    );
                    continue;
                }
                assert_eq!(
                    statement
                        .condition
                        .as_ref()
                        .and_then(|c| c.get("StringEquals"))
                        .and_then(|c| c.get("iam:PermissionsBoundary"))
                        .map(String::as_str),
                    Some(BOUNDARY),
                    "{statement:?}"
                );
            }
            for write in [
                "iam:CreatePolicy",
                "iam:CreatePolicyVersion",
                "iam:SetDefaultPolicyVersion",
                "iam:DeletePolicyVersion",
                "iam:DeletePolicy",
            ] {
                let covers_boundary = statement
                    .resource
                    .iter()
                    .any(|r| wildcard_match(r, BOUNDARY))
                    || (!statement.not_resource.is_empty()
                        && !statement
                            .not_resource
                            .iter()
                            .any(|r| wildcard_match(r, BOUNDARY)));
                assert!(!matches_action(write) || !covers_boundary, "{statement:?}");
            }
        }
    }
    // Current automatic grants use inline policies; attaching managed policies is not granted.
    assert_eq!(
        observed_writes,
        WRITES
            .iter()
            .copied()
            .filter(|action| *action != "iam:AttachRolePolicy")
            .collect()
    );
    for action in WRITES {
        for boundary in [None, Some("arn:aws:iam::123456789012:policy/other")] {
            let mut request = Request::new(action, ROLE);
            if let Some(boundary) = boundary {
                request = request.with("iam:PermissionsBoundary", boundary);
            }
            assert_ne!(evaluate(&bounded, &request), Decision::Allowed, "{action}");
        }
    }
    let legacy = compile(&stack, false);
    for action in [
        "iam:CreateRole",
        "iam:PutRolePolicy",
        "iam:UpdateAssumeRolePolicy",
    ] {
        assert_eq!(
            evaluate(
                &bounded,
                &Request::new(action, ROLE).with("iam:PermissionsBoundary", BOUNDARY)
            ),
            Decision::Allowed,
            "{action}"
        );
        assert_eq!(
            evaluate(&legacy, &Request::new(action, ROLE)),
            Decision::Allowed,
            "legacy {action}"
        );
    }
    assert_ne!(
        evaluate(
            &legacy,
            &Request::new("iam:PutRolePermissionsBoundary", ROLE)
                .with("iam:PermissionsBoundary", BOUNDARY)
        ),
        Decision::Allowed
    );
}
