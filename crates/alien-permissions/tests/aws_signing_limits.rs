//! What the verifier may do to Auto Scaling groups on a protected AWS install, evaluated the way
//! IAM does: the group's name prefix and its tags both have to be the install's.

mod iam_policy;

use alien_permissions::generators::{
    signing_limits::{aws_management_limits, aws_verifier_policy},
    AwsIamStatement,
};
use iam_policy::{evaluate, Decision, Request};
use serde_json::{json, Value};

fn statements(policy: &Value) -> Vec<AwsIamStatement> {
    let listed = |value: &Value| match value {
        Value::Null => Vec::new(),
        Value::String(text) => vec![text.clone()],
        other => serde_json::from_value(other.clone()).expect("list of strings"),
    };
    policy["Statement"]
        .as_array()
        .expect("statements")
        .iter()
        .map(|statement| {
            let mut statement = statement.clone();
            for key in ["Action", "Resource", "NotResource"] {
                if statement.get(key).is_some() {
                    statement[key] = listed(&statement[key]).into();
                }
            }
            serde_json::from_value(statement).expect("statement")
        })
        .collect()
}

fn group(name: &str) -> String {
    format!(
        "arn:aws:autoscaling:us-east-1:111122223333:autoScalingGroup:0f1e2d3c-aaaa-bbbb-cccc-111122223333:autoScalingGroupName/{name}"
    )
}

fn verifier() -> Vec<AwsIamStatement> {
    statements(&aws_verifier_policy(
        "acme",
        "111122223333",
        "us-east-1",
        "444455556666",
    ))
}

#[test]
fn the_verifier_changes_and_deletes_only_groups_tagged_for_its_install() {
    let policy = verifier();
    for action in [
        "autoscaling:UpdateAutoScalingGroup",
        "autoscaling:DeleteAutoScalingGroup",
    ] {
        let own = Request::new(action, group("acme-compute-general"))
            .resource_tags(&[("deployment", "acme")]);
        assert_eq!(evaluate(&policy, &own), Decision::Allowed, "{action}");

        // The name starts with `acme-`, but the group belongs to the install `acme-prod`.
        let other = Request::new(action, group("acme-prod-compute-general"))
            .resource_tags(&[("deployment", "acme-prod")]);
        assert_eq!(evaluate(&policy, &other), Decision::NotAllowed, "{action}");

        let untagged = Request::new(action, group("acme-compute-general"));
        assert_eq!(
            evaluate(&policy, &untagged),
            Decision::NotAllowed,
            "{action}"
        );
    }
}

#[test]
fn the_verifier_creates_groups_only_with_its_installs_tag() {
    let policy = verifier();
    for action in [
        "autoscaling:CreateAutoScalingGroup",
        "autoscaling:CreateOrUpdateTags",
    ] {
        let own = Request::new(action, group("acme-compute-general"))
            .request_tags(&[("deployment", "acme"), ("managed-by", "runtime")]);
        assert_eq!(evaluate(&policy, &own), Decision::Allowed, "{action}");

        let other = Request::new(action, group("acme-prod-compute-general"))
            .request_tags(&[("deployment", "acme-prod"), ("managed-by", "runtime")]);
        assert_eq!(evaluate(&policy, &other), Decision::NotAllowed, "{action}");
    }
}

#[test]
fn the_verifier_cannot_retag_another_installs_group_to_reach_it() {
    let policy = verifier();
    // The name starts with `acme-` and the request tag is the install's own, but the
    // group already carries another install's tag.
    let retag = Request::new(
        "autoscaling:CreateOrUpdateTags",
        group("acme-prod-compute-general"),
    )
    .request_tags(&[("deployment", "acme")])
    .resource_tags(&[("deployment", "acme-prod")]);
    assert_eq!(evaluate(&policy, &retag), Decision::NotAllowed);

    // Tagging a group that has no tag yet, or already has the install's, still works.
    for resource_tags in [&[][..], &[("deployment", "acme")][..]] {
        let own = Request::new(
            "autoscaling:CreateOrUpdateTags",
            group("acme-compute-general"),
        )
        .request_tags(&[("deployment", "acme")])
        .resource_tags(resource_tags);
        assert_eq!(evaluate(&policy, &own), Decision::Allowed);
    }
}

#[test]
fn management_cannot_read_signing_inputs_or_state_however_a_stack_grants_ssm() {
    let limits = aws_management_limits("acme", "111122223333", "us-east-1");
    let mut policy = statements(&limits);
    policy.extend(statements(&json!({ "Statement": [{
        "Sid": "StackGrant",
        "Effect": "Allow",
        "Action": ["ssm:GetParameter*", "ssm:DescribeParameters"],
        "Resource": "*",
    }] })));
    let parameter = |name: &str| format!("arn:aws:ssm:us-east-1:111122223333:parameter/{name}");

    for action in [
        "ssm:GetParameter",
        "ssm:GetParameters",
        "ssm:GetParameterHistory",
    ] {
        for name in [
            "signed/acme/inputs/databaseUrl",
            "signed/acme/inputs/nested/key",
            "signed/acme/state",
            // Another install in the same account.
            "signed/acme-prod/inputs/databaseUrl",
            "signed/acme-prod/state",
        ] {
            let request = Request::new(action, parameter(name));
            assert_eq!(
                evaluate(&policy, &request),
                Decision::ExplicitlyDenied,
                "{action} {name}"
            );
        }
        // Parameters the app and the install's own vaults keep are not signing inputs.
        let vault = Request::new(action, parameter("acme-secrets-DATABASE_URL"));
        assert_eq!(evaluate(&policy, &vault), Decision::Allowed, "{action}");
    }

    // The fingerprint read the limits grant, and the one a broad stack grant also reaches.
    let fingerprint = Request::new("ssm:GetParameter", parameter("signed/acme/fingerprint"));
    assert_eq!(evaluate(&policy, &fingerprint), Decision::Allowed);

    // The path read is authorised on the path, so a grant on `*` can't be allowed to return a
    // whole hierarchy.
    for path in ["signed/acme", "signed/acme/inputs", "signed", ""] {
        let request = Request::new("ssm:GetParametersByPath", parameter(path));
        assert_eq!(
            evaluate(&policy, &request),
            Decision::ExplicitlyDenied,
            "{path}"
        );
    }
}

#[test]
fn the_verifier_sets_bucket_policies_only_on_its_installs_buckets() {
    let policy = verifier();
    let own = Request::new("s3:PutBucketPolicy", "arn:aws:s3:::acme-assets");
    assert_eq!(evaluate(&policy, &own), Decision::Allowed);
    let other = Request::new("s3:PutBucketPolicy", "arn:aws:s3:::someone-elses-assets");
    assert_eq!(evaluate(&policy, &other), Decision::NotAllowed);
    // It can read nothing a bucket holds: the grant is for the policy alone.
    let object = Request::new("s3:GetObject", "arn:aws:s3:::acme-assets/key");
    assert_eq!(evaluate(&policy, &object), Decision::NotAllowed);
}
