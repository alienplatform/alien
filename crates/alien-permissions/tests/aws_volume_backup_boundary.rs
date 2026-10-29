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
        aws_permissions_boundary, aws_permissions_boundary_covers, AwsIamStatement,
        AwsRuntimePermissionsGenerator,
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
fn management_passes_dlm_only_the_setup_role() {
    let management = management();
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
        assert_eq!(
            evaluate(
                &management,
                &Request::new("iam:DeleteRole", role_arn(&setup_role))
            ),
            Decision::ExplicitlyDenied,
            "{setup_role}"
        );
    }
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
fn workload_boundary_covers_volume_backups_without_allowing_snapshot_sharing() {
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

    for action in ["ec2:ModifySnapshotAttribute", "ec2:CopySnapshot"] {
        for grant in &required.statement {
            let mut sharing = grant.clone();
            sharing.action = vec![action.to_string()];
            let sharing = serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [sharing],
            });
            assert!(
                !aws_permissions_boundary_covers(&boundary, &sharing).unwrap(),
                "boundary must not allow {action}, even with the stack ownership conditions"
            );
        }
    }

    // Coverage must not silently drop ownership conditions or grant DLM sharing/copying.
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
