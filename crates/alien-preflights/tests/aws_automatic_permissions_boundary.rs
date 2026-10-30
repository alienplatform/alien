//! Exercise the automatic profile produced by preflight, not just individual catalog sets.
#[path = "../../alien-permissions/tests/iam_policy/mod.rs"]
mod iam_policy;

use alien_core::{
    permissions::ManagementPermissions, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, Platform, ResourceLifecycle, SetupManagement, Stack, StackSettings,
    StackState, Storage, Vault, Worker, WorkerCode, ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY,
};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, management_identity_global_refs, BindingTarget, PermissionContext,
};
use alien_preflights::{mutations::ManagementPermissionProfileMutation, StackMutation};
use iam_policy::{evaluate, wildcard_match, Decision, Request};
use serde_json::json;
use std::collections::BTreeSet;

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/sample-deployment-management-boundary";
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

async fn automatic_stack() -> Stack {
    stack_with_management(SetupManagement::Automatic, ManagementPermissions::Auto).await
}

async fn stack_with_management(
    setup_management: SetupManagement,
    management: ManagementPermissions,
) -> Stack {
    let mut stack = Stack::new("sample".to_string())
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
    stack.permissions.management = management;
    let settings = StackSettings {
        setup_management,
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
    ManagementPermissionProfileMutation
        .mutate(stack, &StackState::new(Platform::Aws), &config)
        .await
        .unwrap()
}

#[tokio::test]
async fn automatic_profile_bounds_every_role_writer_and_cannot_change_the_boundary() {
    let stack = automatic_stack().await;
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
    assert!(!ids.contains("remote-stack-management/provision"));
    let bounded = compile(&stack, true);
    for action in [
        "iam:CreatePolicyVersion",
        "iam:SetDefaultPolicyVersion",
        "iam:DeletePolicyVersion",
        "iam:DeletePolicy",
        "iam:TagPolicy",
        "iam:UntagPolicy",
    ] {
        assert_eq!(
            evaluate(&bounded, &Request::new(action, BOUNDARY)),
            Decision::ExplicitlyDenied,
            "{action}"
        );
        assert_eq!(
            evaluate(
                &bounded,
                &Request::new(
                    action,
                    "arn:aws:iam::123456789012:policy/sample-deployment-management-1"
                )
            ),
            Decision::NotAllowed,
            "management must not rewrite its own policies: {action}"
        );
    }
    assert_eq!(
        evaluate(
            &bounded,
            &Request::new("iam:DeleteRolePermissionsBoundary", ROLE)
        ),
        Decision::ExplicitlyDenied
    );
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
                assert_eq!(
                    statement
                        .condition
                        .as_ref()
                        .and_then(|c| c.get("StringEquals"))
                        .and_then(|c| c.get("iam:PermissionsBoundary"))
                        .and_then(|value| value.as_string()),
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
            let mut request = Request::new(action, ROLE)
                .resource_tags(&[("managed-by", "runtime"), ("deployment", "sample")])
                .request_tags(&[("managed-by", "runtime"), ("deployment", "sample")]);
            if let Some(boundary) = boundary {
                request = request.with("iam:PermissionsBoundary", boundary);
            }
            assert_ne!(evaluate(&bounded, &request), Decision::Allowed, "{action}");
        }
    }
    // Obsolete-policy cleanup also requires runtime ownership and the boundary.
    for policy in [
        "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore",
        "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy",
        "arn:aws:iam::aws:policy/AdministratorAccess",
    ] {
        for action in ["iam:DetachRolePolicy", "iam:AttachRolePolicy"] {
            let expected = if action == "iam:DetachRolePolicy" {
                Decision::Allowed
            } else {
                Decision::NotAllowed
            };
            assert_eq!(
                evaluate(
                    &bounded,
                    &Request::new(action, "arn:aws:iam::123456789012:role/sample-compute-role")
                        .with("iam:PolicyARN", policy)
                        .with("iam:PermissionsBoundary", BOUNDARY)
                        .resource_tags(&[("managed-by", "runtime"), ("deployment", "sample")])
                ),
                expected,
                "{action}: {policy}"
            );
        }
    }
    let legacy = compile(&stack, false);
    assert_role_ownership_matrix(&bounded);
    for action in [
        "iam:CreateRole",
        "iam:PutRolePolicy",
        "iam:UpdateAssumeRolePolicy",
    ] {
        assert_eq!(
            evaluate(
                &bounded,
                &Request::new(action, ROLE)
                    .with("iam:PermissionsBoundary", BOUNDARY)
                    .resource_tags(&[("managed-by", "runtime"), ("deployment", "sample")])
                    .request_tags(&[("managed-by", "runtime"), ("deployment", "sample")])
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

fn assert_role_ownership_matrix(policy: &[AwsIamStatement]) {
    for role in [
        ROLE,
        "arn:aws:iam::123456789012:role/sample-volume-backups-compute",
        "arn:aws:iam::123456789012:role/sample-compute-role",
        "arn:aws:iam::123456789012:role/sample-dlm-old",
    ] {
        for owner in [None, Some("runtime"), Some("setup")] {
            for deployment in [None, Some("sample"), Some("foreign")] {
                for boundary in [None, Some(BOUNDARY), Some("foreign-boundary")] {
                    for action in [
                        "iam:PutRolePolicy",
                        "iam:DeleteRolePolicy",
                        "iam:UpdateAssumeRolePolicy",
                        "iam:PutRolePermissionsBoundary",
                        "iam:DetachRolePolicy",
                        "iam:DeleteRole",
                        "iam:TagRole",
                    ] {
                        let mut request = Request::new(action, role).request_tags(&[
                            ("managed-by", "runtime"),
                            ("deployment", "sample"),
                            ("resource", "jobs"),
                            ("resource-type", "worker"),
                            ("Name", "jobs"),
                        ]);
                        if let Some(owner) = owner {
                            request = request.with("aws:ResourceTag/managed-by", owner);
                        }
                        if let Some(deployment) = deployment {
                            request = request.with("aws:ResourceTag/deployment", deployment);
                        }
                        if let Some(boundary) = boundary {
                            request = request.with("iam:PermissionsBoundary", boundary);
                        }
                        let owned = owner == Some("runtime") && deployment == Some("sample");
                        let needs_boundary = !matches!(action, "iam:TagRole" | "iam:DeleteRole");
                        assert_eq!(evaluate(policy, &request) == Decision::Allowed, owned && (!needs_boundary || boundary == Some(BOUNDARY)), "{role} {action} owner={owner:?} deployment={deployment:?} boundary={boundary:?}");
                    }
                }
            }
        }
        for action in ["iam:CreateRole", "iam:TagRole"] {
            for owner in [None, Some("runtime"), Some("setup")] {
                for deployment in [None, Some("sample"), Some("foreign")] {
                    for owner_key in ["managed-by", "Managed-By"] {
                        for deployment_key in ["deployment", "Deployment"] {
                            let mut tags = vec![("resource", "jobs")];
                            if let Some(owner) = owner {
                                tags.push((owner_key, owner));
                            }
                            if let Some(deployment) = deployment {
                                tags.push((deployment_key, deployment));
                            }
                            let request = Request::new(action, role)
                                .with("iam:PermissionsBoundary", BOUNDARY)
                                .resource_tags(&[
                                    ("managed-by", "runtime"),
                                    ("deployment", "sample"),
                                ])
                                .request_tags(&tags);
                            assert_eq!(
                                evaluate(policy, &request) == Decision::Allowed,
                                owner == Some("runtime")
                                    && deployment == Some("sample")
                                    && owner_key == "managed-by"
                                    && deployment_key == "deployment",
                                "{role} {action}: {tags:?}"
                            );
                        }
                    }
                }
            }
        }
        // No catalog grant currently needs tag removal; ownership cannot be removed.
        for keys in [
            vec!["managed-by"],
            vec!["deployment"],
            vec!["resource", "Managed-By"],
        ] {
            let request = Request::new("iam:UntagRole", role)
                .resource_tags(&[("managed-by", "runtime"), ("deployment", "sample")])
                .with_values("aws:TagKeys", &keys);
            assert_ne!(evaluate(policy, &request), Decision::Allowed);
        }
    }
}

#[tokio::test]
async fn legacy_automatic_iam_permissions_match_the_pre_boundary_policy() {
    let stack = automatic_stack().await;
    let legacy = compile(&stack, false);
    let baseline: Vec<AwsIamStatement> =
        serde_json::from_str(include_str!("fixtures/legacy_automatic_iam_policy.json")).unwrap();
    let iam = |statements: &[AwsIamStatement]| {
        let statements: Vec<_> = statements
            .iter()
            .filter_map(|statement| {
                let mut statement = statement.clone();
                statement.action.retain(|action| {
                    action == "*"
                        || action.split_once(':').is_some_and(|(service, _)| {
                            wildcard_match(&service.to_ascii_lowercase(), "iam")
                        })
                });
                (!statement.action.is_empty()).then_some(statement)
            })
            .collect();
        json!({"Version": "2012-10-17", "Statement": statements})
    };
    let baseline_policy = iam(&baseline);
    let legacy_policy = iam(&legacy);
    assert!(
        alien_permissions::generators::aws_permissions_boundary_covers(
            &baseline_policy,
            &legacy_policy
        )
        .unwrap(),
        "legacy compilation must not add IAM grants or remove existing denies"
    );
    assert!(
        alien_permissions::generators::aws_permissions_boundary_covers(
            &legacy_policy,
            &baseline_policy
        )
        .unwrap(),
        "legacy compilation must preserve existing IAM grants and denies"
    );
}

#[tokio::test]
async fn automatic_profile_cannot_retag_foreign_security_groups() {
    let stack = automatic_stack().await;
    let group = "arn:aws:ec2:us-east-1:123456789012:security-group/sg-example";
    let runtime_tags = [
        (ALIEN_STACK_TAG_KEY, "sample"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ];
    for bounded in [false, true] {
        // Evaluate the union: another provisioning set must not bypass network ownership.
        let policy = compile(&stack, bounded);
        for (owner, manager, expected) in [
            ("sample", "runtime", Decision::Allowed),
            ("other-stack", "runtime", Decision::NotAllowed),
            ("sample", "setup", Decision::NotAllowed),
            ("", "", Decision::NotAllowed),
        ] {
            let existing = [
                (ALIEN_STACK_TAG_KEY, owner),
                (ALIEN_MANAGED_BY_TAG_KEY, manager),
            ];
            let request = Request::new("ec2:CreateTags", group)
                .resource_tags(&existing)
                .request_tags(&runtime_tags);
            assert_eq!(
                evaluate(&policy, &request),
                expected,
                "bounded={bounded}, existing ownership={owner}/{manager}"
            );
        }

        // The service-supplied creation context permits the initial tags only.
        for (creation, tags, expected) in [
            (
                "CreateSecurityGroup",
                runtime_tags.as_slice(),
                Decision::Allowed,
            ),
            ("CreateSecurityGroup", &[][..], Decision::NotAllowed),
            ("CreateVpc", runtime_tags.as_slice(), Decision::NotAllowed),
            ("", runtime_tags.as_slice(), Decision::NotAllowed),
        ] {
            let request = Request::new("ec2:CreateTags", group)
                .request_tags(tags)
                .with("ec2:CreateAction", creation);
            assert_eq!(
                evaluate(&policy, &request),
                expected,
                "bounded={bounded}, creation={creation}, tags={tags:?}"
            );
        }
    }
}

#[tokio::test]
async fn boundary_guard_is_installed_in_every_management_mode() {
    for setup in [SetupManagement::Manual, SetupManagement::Automatic] {
        for management in [
            ManagementPermissions::Auto,
            ManagementPermissions::Extend(Default::default()),
            ManagementPermissions::Override(Default::default()),
        ] {
            if setup == SetupManagement::Automatic
                && matches!(management, ManagementPermissions::Override(_))
            {
                continue; // Automatic setup rejects override profiles.
            }
            let stack = stack_with_management(setup, management).await;
            let context = PermissionContext::new()
                .with_stack_prefix("sample")
                .with_aws_account_id("123456789012")
                .with_aws_permissions_boundary_arn(BOUNDARY);
            let guard = stack.management().profile().unwrap().0["*"]
                .iter()
                .find(|reference| reference.id() == alien_permissions::MANAGEMENT_ROLE_GUARD)
                .expect("management guard in every mode")
                .resolve(|name| get_permission_set(name).cloned())
                .unwrap();
            let policy = AwsRuntimePermissionsGenerator::new()
                .generate_policy(&guard, BindingTarget::Stack, &context)
                .unwrap();
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &Request::new("iam:CreatePolicyVersion", BOUNDARY)
                ),
                Decision::ExplicitlyDenied
            );
        }
    }
}

#[tokio::test]
async fn automatic_profile_passes_roles_only_to_their_intended_services() {
    let stack = automatic_stack().await;
    let policy = compile(&stack, true);
    // Evaluate every automatic grant together: a compute grant must not bypass backup scope.
    for (role, service, expected) in [
        (
            "sample-volume-backups-compute",
            "dlm.amazonaws.com",
            Decision::Allowed,
        ),
        (
            "sample-compute-node",
            "ec2.amazonaws.com",
            Decision::Allowed,
        ),
        (
            "sample-execution-sa",
            "lambda.amazonaws.com",
            Decision::Allowed,
        ),
        (
            "sample-volume-backups-compute",
            "lambda.amazonaws.com",
            Decision::NotAllowed,
        ),
        (
            "sample-volume-backups-compute",
            "codebuild.amazonaws.com",
            Decision::NotAllowed,
        ),
        (
            "foreign-volume-backups-compute",
            "dlm.amazonaws.com",
            Decision::NotAllowed,
        ),
        (
            "foreign-compute-node",
            "ec2.amazonaws.com",
            Decision::NotAllowed,
        ),
    ] {
        // PassRole does not expose the role's resource tags to IAM conditions.
        let request = Request::new(
            "iam:PassRole",
            format!("arn:aws:iam::123456789012:role/{role}"),
        )
        .with("iam:PassedToService", service);
        assert_eq!(evaluate(&policy, &request), expected, "{role} to {service}");
    }
}
