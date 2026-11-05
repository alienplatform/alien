//! What the management identity and the volume-backup execution role can reach, checked by
//! evaluating concrete requests against the rendered policies the way IAM does: an explicit Deny
//! wins, otherwise any matching Allow allows, otherwise the request is denied.
//!
//! The requests are the ones that matter for EBS data: creating or rewriting a role DLM may run
//! as, passing a role to DLM, stamping a volume with the stack's tags, and snapshotting, sharing
//! or copying a volume or snapshot. The live check of the same requests against AWS is described
//! in the pull request that introduced this file.

mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{
        aws_permissions_boundary, aws_permissions_boundary_covers, aws_stack_permissions_boundary,
        AwsCloudFormationPermissionsGenerator, AwsIamStatement, AwsRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, wildcard_match, Decision, Request};

const ACCOUNT: &str = "123456789012";
const REGION: &str = "us-east-1";
const PREFIX: &str = "app";

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix(PREFIX)
        .with_aws_account_id(ACCOUNT)
        .with_aws_region(REGION)
}

fn rendered(set_ids: &[&str]) -> Vec<AwsIamStatement> {
    let generator = AwsRuntimePermissionsGenerator::new();
    set_ids
        .iter()
        .flat_map(|id| {
            let set = get_permission_set(id).unwrap_or_else(|| panic!("missing set {id}"));
            generator
                .generate_policy(set, BindingTarget::Stack, &context())
                .unwrap_or_else(|error| panic!("{id} should render: {error}"))
                .statement
        })
        .collect()
}

/// The container and compute-cluster grants the management identity holds together. IAM unions
/// them, so a narrow grant in one set is only as narrow as the widest overlapping grant elsewhere.
fn management() -> Vec<AwsIamStatement> {
    rendered(&[
        "container/provision",
        "compute-cluster/provision",
        "compute-cluster/management",
    ])
}

fn role_arn(name: &str) -> String {
    format!("arn:aws:iam::{ACCOUNT}:role/{name}")
}

fn volume_arn(id: &str) -> String {
    format!("arn:aws:ec2:{REGION}:{ACCOUNT}:volume/{id}")
}

fn snapshot_arn(id: &str) -> String {
    format!("arn:aws:ec2:{REGION}::snapshot/{id}")
}

fn stack_volume_tags() -> [(&'static str, &'static str); 3] {
    [
        (ALIEN_STACK_TAG_KEY, PREFIX),
        (ALIEN_RESOURCE_TAG_KEY, "db"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ]
}

const DLM_SERVICE_POLICY: &str =
    "arn:aws:iam::aws:policy/service-role/AWSDataLifecycleManagerServiceRole";

#[test]
fn management_cannot_make_or_rewrite_a_role_dlm_runs_as() {
    let management = management();
    let legacy = role_arn("app-dlm-0123456789abcdef");
    let setup_role = role_arn("app-volume-backups-compute");

    for target in [&legacy, &setup_role] {
        let create =
            Request::new("iam:CreateRole", target.clone()).request_tags(&stack_volume_tags());
        assert_ne!(
            evaluate(&management, &create),
            Decision::Allowed,
            "management could create {target} with any trust policy"
        );
        let attach = Request::new("iam:AttachRolePolicy", target.clone())
            .with("iam:PolicyARN", DLM_SERVICE_POLICY);
        assert_ne!(
            evaluate(&management, &attach),
            Decision::Allowed,
            "management could attach the DLM service policy to {target}"
        );
    }
    for action in [
        "iam:PutRolePolicy",
        "iam:UpdateAssumeRolePolicy",
        "iam:AttachRolePolicy",
        "iam:DeleteRole",
        "iam:TagRole",
        "iam:UntagRole",
        "iam:DeleteRolePermissionsBoundary",
    ] {
        assert_eq!(
            evaluate(&management, &Request::new(action, setup_role.clone())),
            Decision::ExplicitlyDenied,
            "{action} on the setup-created execution role must be denied"
        );
    }
}

#[test]
fn bounded_container_grants_do_not_write_setup_backup_roles() {
    let boundary = format!("arn:aws:iam::{ACCOUNT}:policy/{PREFIX}-deployment-management-boundary");
    let context = legacy_context().with_aws_permissions_boundary_arn(&boundary);
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        let set = get_permission_set("container/provision").unwrap();
        let runtime = AwsRuntimePermissionsGenerator::new()
            .generate_policy(set, target, &context)
            .unwrap();
        let cfn = AwsCloudFormationPermissionsGenerator::new()
            .generate_policy(set, target, &context)
            .unwrap();
        assert_eq!(
            runtime.statement.iter().map(|s| &s.sid).collect::<Vec<_>>(),
            cfn.statement.iter().map(|s| &s.sid).collect::<Vec<_>>()
        );
        for owner in ["setup", "runtime"] {
            for action in [
                "iam:CreateRole",
                "iam:PutRolePolicy",
                "iam:DeleteRolePolicy",
                "iam:UpdateAssumeRolePolicy",
                "iam:PutRolePermissionsBoundary",
                "iam:TagRole",
                "iam:DeleteRole",
            ] {
                let request = Request::new(action, role_arn("app-volume-backups-compute"))
                    .with("iam:PermissionsBoundary", &boundary)
                    .resource_tags(&[("managed-by", owner), ("deployment", PREFIX)])
                    .request_tags(&[("managed-by", owner), ("deployment", PREFIX)]);
                assert_eq!(
                    evaluate(&runtime.statement, &request),
                    Decision::NotAllowed,
                    "{owner}: {action}"
                );
            }
        }
    }
}

fn legacy_context() -> PermissionContext {
    context().with_resource_name("db").with_resource_id("db")
}

#[test]
fn management_passes_dlm_only_the_setup_role() {
    for bounded in [false, true] {
        let mut permission_context = context();
        if bounded {
            permission_context = permission_context.with_aws_permissions_boundary_arn(
                "arn:aws:iam::123456789012:policy/app-deployment-management-boundary",
            );
        }
        let management: Vec<_> = [
            "container/provision",
            "compute-cluster/provision",
            "compute-cluster/management",
        ]
        .into_iter()
        .flat_map(|id| {
            AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    get_permission_set(id).unwrap(),
                    BindingTarget::Stack,
                    &permission_context,
                )
                .unwrap()
                .statement
        })
        .collect();
        // IAM doesn't evaluate a role's tags for PassRole, so the request carries none: a grant
        // that relied on them would deny here as it does in AWS.
        let pass = |role: &str, service: &str| {
            evaluate(
                &management,
                &Request::new("iam:PassRole", role_arn(role)).with("iam:PassedToService", service),
            )
        };

        // Whatever the cluster is called, its setup role stays clear of the legacy-role Deny,
        // `app-dlm-*`, and of the legacy cleanup grants on the same names.
        for cluster in ["compute", "dlm", "dlm-a", "volume-backups"] {
            let setup_role = format!("app-volume-backups-{cluster}");
            assert_eq!(
                pass(&setup_role, "dlm.amazonaws.com"),
                Decision::Allowed,
                "{setup_role}"
            );
            for service in ["lambda.amazonaws.com", "codebuild.amazonaws.com"] {
                assert_eq!(
                    pass(&setup_role, service),
                    if bounded {
                        Decision::NotAllowed
                    } else {
                        Decision::Allowed
                    },
                    "bounded={bounded}: {setup_role} to {service}"
                );
            }
            assert_eq!(
                evaluate(
                    &management,
                    &Request::new("iam:DeleteRole", role_arn(&setup_role))
                ),
                if bounded {
                    Decision::NotAllowed
                } else {
                    Decision::ExplicitlyDenied
                },
                "{setup_role}"
            );
        }
        assert_eq!(
            pass("app-compute-node", "ec2.amazonaws.com"),
            Decision::Allowed
        );
        // A role earlier releases made at runtime still carries the AWS managed DLM policy, which
        // may share or copy any snapshot. The compute cluster's prefix-wide PassRole names it too.
        assert_eq!(
            pass("app-dlm-0123456789abcdef", "dlm.amazonaws.com"),
            Decision::ExplicitlyDenied
        );
        for statement in &management {
            let passes_role = statement
                .action
                .iter()
                .any(|action| wildcard_match(&action.to_lowercase(), "iam:passrole"));
            let uses_role_tags = statement.condition.iter().flatten().any(|(_, entries)| {
                entries.keys().any(|key| {
                    key.starts_with("aws:ResourceTag/") || key.starts_with("iam:ResourceTag/")
                })
            });
            assert!(
                !(passes_role && uses_role_tags),
                "{}: IAM doesn't evaluate role tags for PassRole",
                statement.sid
            );
        }
    }
}

#[test]
fn management_can_tag_only_volumes_the_stack_owns() {
    let management = management();
    let ownership = stack_volume_tags();

    // An existing volume of someone else's, stamped with this stack's tags, would then fall
    // under every stack-scoped snapshot grant.
    let stamp_foreign = Request::new("ec2:CreateTags", volume_arn("vol-foreign"))
        .request_tags(&ownership)
        .resource_tags(&[("Name", "prod-db")]);
    assert_ne!(evaluate(&management, &stamp_foreign), Decision::Allowed);

    // The runtime's tag writes on existing volumes, on a volume the stack owns: the
    // backup-policy tag, and the tags a restored volume gets once it replaces the old one.
    for extra in [("alien-backup-policy", "app/db"), ("Ordinal", "0")] {
        let mut tags = ownership.to_vec();
        tags.push(extra);
        let retag = Request::new("ec2:CreateTags", volume_arn("vol-owned"))
            .request_tags(&tags)
            .resource_tags(&ownership);
        assert_eq!(
            evaluate(&management, &retag),
            Decision::Allowed,
            "the runtime must keep tagging its own volume with {extra:?}"
        );
    }

    // Tags written while creating a volume (a new replica's volume, or a restore's).
    let create = Request::new("ec2:CreateVolume", volume_arn("*")).request_tags(&ownership);
    assert_eq!(evaluate(&management, &create), Decision::Allowed);
    let tag_on_create = Request::new("ec2:CreateTags", volume_arn("vol-new"))
        .request_tags(&ownership)
        .with("ec2:CreateAction", "CreateVolume");
    assert_eq!(evaluate(&management, &tag_on_create), Decision::Allowed);

    let snapshot_foreign = Request::new("ec2:CreateSnapshot", volume_arn("vol-foreign"))
        .resource_tags(&[("Name", "prod-db")]);
    assert_ne!(evaluate(&management, &snapshot_foreign), Decision::Allowed);
}

#[test]
fn the_execution_role_snapshots_only_stack_volumes_and_never_shares() {
    let role = rendered(&["container/volume-backups"]);
    let ownership = stack_volume_tags();

    let own_volume =
        Request::new("ec2:CreateSnapshot", volume_arn("vol-owned")).resource_tags(&ownership);
    assert_eq!(evaluate(&role, &own_volume), Decision::Allowed);
    let new_snapshot =
        Request::new("ec2:CreateSnapshot", snapshot_arn("snap-new")).request_tags(&ownership);
    assert_eq!(evaluate(&role, &new_snapshot), Decision::Allowed);
    let tag_new = Request::new("ec2:CreateTags", snapshot_arn("snap-new"))
        .request_tags(&ownership)
        .with("ec2:CreateAction", "CreateSnapshot");
    assert_eq!(evaluate(&role, &tag_new), Decision::Allowed);
    let expire =
        Request::new("ec2:DeleteSnapshot", snapshot_arn("snap-old")).resource_tags(&ownership);
    assert_eq!(evaluate(&role, &expire), Decision::Allowed);

    // A DLM policy can target any tag. The role must refuse the volumes it reaches that way.
    let foreign_volume = Request::new("ec2:CreateSnapshot", volume_arn("vol-foreign"))
        .resource_tags(&[("Name", "prod-db")]);
    assert_eq!(evaluate(&role, &foreign_volume), Decision::NotAllowed);
    let foreign_snapshot = Request::new("ec2:DeleteSnapshot", snapshot_arn("snap-foreign"));
    assert_eq!(evaluate(&role, &foreign_snapshot), Decision::NotAllowed);
    let retag_foreign =
        Request::new("ec2:CreateTags", snapshot_arn("snap-foreign")).request_tags(&ownership);
    assert_eq!(evaluate(&role, &retag_foreign), Decision::NotAllowed);

    // Share and copy rules in a DLM policy run as this role.
    for action in [
        "ec2:ModifySnapshotAttribute",
        "ec2:CopySnapshot",
        "ec2:ModifySnapshotTier",
        "ec2:EnableFastSnapshotRestores",
        "ec2:CreateImage",
    ] {
        let request = Request::new(action, snapshot_arn("snap-owned")).resource_tags(&ownership);
        assert_eq!(
            evaluate(&role, &request),
            Decision::NotAllowed,
            "the execution role must not be granted {action}"
        );
    }
}

#[test]
fn workload_boundary_covers_volume_backups_only_with_ownership_conditions() {
    let boundary =
        aws_permissions_boundary(&context().with_managing_account_id("210987654321"), &[]).unwrap();
    let required = AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("container/volume-backups").unwrap(),
            BindingTarget::Stack,
            &context(),
        )
        .unwrap();
    assert!(
        aws_permissions_boundary_covers(&boundary, &serde_json::to_value(&required).unwrap())
            .unwrap()
    );

    // Check the complete snapshot lifecycle, including conditions on both the source
    // volume and the new snapshot. Describe-only coverage would not be sufficient.
    for action in [
        "ec2:CreateSnapshot",
        "ec2:CreateTags",
        "ec2:DeleteSnapshot",
        "ec2:DescribeVolumes",
        "ec2:DescribeSnapshots",
        "ec2:DescribeInstances",
    ] {
        assert!(required
            .statement
            .iter()
            .any(|s| s.action.iter().any(|a| a == action)));
    }

    // The identity policy excludes sharing above; the boundary supplies stack isolation.
    // Removing ownership conditions must still fail coverage.
    for action in [
        "ec2:CreateSnapshot",
        "ec2:CreateSnapshots",
        "ec2:CreateTags",
        "ec2:DeleteSnapshot",
        "ec2:ModifySnapshotAttribute",
        "ec2:CopySnapshot",
    ] {
        for resource in [volume_arn("vol-foreign"), snapshot_arn("snap-foreign")] {
            let unconditional = serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [{
                    "Effect": "Allow",
                    "Action": action,
                    "Resource": resource,
                }],
            });
            assert!(
                !aws_permissions_boundary_covers(&boundary, &unconditional).unwrap(),
                "boundary must not allow unconditional {action} on {resource}"
            );
        }
    }
}

#[test]
fn wildcard_matching_follows_iam() {
    assert!(wildcard_match(
        "arn:aws:iam::1:role/app-*",
        "arn:aws:iam::1:role/app-x/y"
    ));
    assert!(wildcard_match("ec2:*", "ec2:createtags"));
    assert!(wildcard_match("a?c", "abc"));
    assert!(!wildcard_match(
        "arn:aws:iam::1:role/app-volume-backups-*",
        "arn:aws:iam::1:role/app-dlm-1"
    ));
    assert!(!wildcard_match("app", "app-x"));
}

#[test]
fn encrypted_volume_backup_grants_are_covered_without_an_application_profile() {
    let key_arn = "arn:aws:kms:us-east-1:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab";
    let container = alien_core::Container::new("db".to_string())
        .cluster("compute".to_string())
        .permissions("execution".to_string())
        .code(alien_core::ContainerCode::Image {
            image: "postgres:17".to_string(),
        })
        .cpu(alien_core::ResourceSpec {
            min: "1".to_string(),
            desired: "1".to_string(),
        })
        .memory(alien_core::ResourceSpec {
            min: "1Gi".to_string(),
            desired: "1Gi".to_string(),
        })
        .persistent_storage(alien_core::PersistentStorage {
            size: "10Gi".to_string(),
            mount_path: "/data".to_string(),
            encryption_key: Some(alien_core::ResourceRef::new("key".into(), "encryption")),
            backups: alien_core::VolumeBackups::default(),
        })
        .build();
    let mut second = container.clone();
    second.id = "db-replica".to_string();
    second.persistent_storage.as_mut().unwrap().backups.enabled = false;
    let stack = alien_core::Stack::new("sample".to_string())
        .add(container, alien_core::ResourceLifecycle::Live)
        .add(second, alien_core::ResourceLifecycle::Live)
        .add(
            alien_core::Key::new("encryption".to_string()).build(),
            alien_core::ResourceLifecycle::Frozen,
        )
        .build();
    assert!(stack.permission_profiles().is_empty());
    let context = context().with_managing_account_id("210987654321");
    let mut resolutions = 0;
    let boundary = aws_stack_permissions_boundary(&stack, &context, |owner, scope| {
        resolutions += 1;
        assert_eq!(owner, "execution");
        assert_eq!(scope, "encryption");
        Ok(context.clone().with_resource_name(key_arn))
    })
    .unwrap();
    let key_policy = AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("key/native-volume").unwrap(),
            BindingTarget::Resource,
            &context.clone().with_resource_name(key_arn),
        )
        .unwrap();
    let mut backup_policy = rendered(&["container/volume-backups"]);
    backup_policy.extend(key_policy.statement.clone());
    assert!(
        aws_permissions_boundary_covers(
            &boundary,
            &serde_json::json!({
                "Version": "2012-10-17", "Statement": backup_policy,
            })
        )
        .unwrap(),
        "the complete encrypted backup policy must fit the generated ceiling"
    );
    assert_eq!(resolutions, 1, "shared volume keys should resolve once");
    for mut grant in key_policy.statement {
        let original = grant.clone();
        grant.resource = vec![key_arn.replace("1234abcd", "9876abcd")];
        assert!(
            !aws_permissions_boundary_covers(
                &boundary,
                &serde_json::json!({
                    "Version": "2012-10-17", "Statement": [grant],
                })
            )
            .unwrap(),
            "a sibling key must remain outside the ceiling"
        );
        let mut grant = original;
        grant.condition = None;
        // Other service restrictions stay in identity policies; key grants must be for AWS resources.
        let creates_grants = grant
            .action
            .iter()
            .any(|action| action == "kms:CreateGrant");
        assert_eq!(
            aws_permissions_boundary_covers(
                &boundary,
                &serde_json::json!({
                    "Version": "2012-10-17", "Statement": [grant],
                })
            )
            .unwrap(),
            !creates_grants,
            "the exact-key ceiling leaves service restrictions to identity policies"
        );
    }
    assert!(
        aws_stack_permissions_boundary(&stack, &context, |_, _| Ok(context.clone())).is_err(),
        "a missing physical key ARN must not silently omit encryption grants"
    );
}
