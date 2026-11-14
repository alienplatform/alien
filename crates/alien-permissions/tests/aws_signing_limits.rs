//! What the verifier may do to Auto Scaling groups on a protected AWS install, evaluated the way
//! IAM does: the group's name prefix and its tags both have to be the install's.

mod iam_policy;

use alien_permissions::generators::{
    signing_limits::aws_verifier_policy,
    AwsIamStatement,
};
use iam_policy::{evaluate, Decision, Request};
use serde_json::Value;

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
fn the_verifier_sets_bucket_policies_only_on_its_installs_buckets() {
    let policy = verifier();
    let own = Request::new("s3:PutBucketPolicy", "arn:aws:s3:::acme-assets");
    assert_eq!(evaluate(&policy, &own), Decision::Allowed);
    let other = Request::new("s3:PutBucketPolicy", "arn:aws:s3:::someone-elses-assets");
    assert_eq!(evaluate(&policy, &other), Decision::NotAllowed);
    let block = Request::new("s3:PutBucketPublicAccessBlock", "arn:aws:s3:::acme-assets");
    assert_eq!(evaluate(&policy, &block), Decision::Allowed);
    for action in ["s3:PutBucketNotification", "s3:GetBucketNotification"] {
        let own = Request::new(action, "arn:aws:s3:::acme-assets");
        assert_eq!(evaluate(&policy, &own), Decision::Allowed, "{action}");
        let other = Request::new(action, "arn:aws:s3:::someone-elses-assets");
        assert_eq!(evaluate(&policy, &other), Decision::NotAllowed, "{action}");
    }
    // It can read nothing a bucket holds: the grant is for the policy alone.
    let object = Request::new("s3:GetObject", "arn:aws:s3:::acme-assets/key");
    assert_eq!(evaluate(&policy, &object), Decision::NotAllowed);
}

#[test]
fn the_verifier_creates_and_configures_only_its_installs_queues() {
    let policy = verifier();
    for action in ["sqs:CreateQueue", "sqs:SetQueueAttributes", "sqs:TagQueue"] {
        let own = Request::new(action, "arn:aws:sqs:us-east-1:111122223333:acme-jobs");
        assert_eq!(evaluate(&policy, &own), Decision::Allowed, "{action}");
        let other = Request::new(action, "arn:aws:sqs:us-east-1:111122223333:someone-elses");
        assert_eq!(evaluate(&policy, &other), Decision::NotAllowed, "{action}");
    }
    let send = Request::new(
        "sqs:SendMessage",
        "arn:aws:sqs:us-east-1:111122223333:acme-jobs",
    );
    assert_eq!(evaluate(&policy, &send), Decision::NotAllowed);
}

#[test]
fn the_verifier_maps_event_sources_only_to_its_installs_functions() {
    let policy = verifier();
    for action in [
        "lambda:CreateEventSourceMapping",
        "lambda:UpdateEventSourceMapping",
    ] {
        let own = Request::new(action, "*").with(
            "lambda:FunctionArn",
            "arn:aws:lambda:us-east-1:111122223333:function:acme-worker",
        );
        assert_eq!(evaluate(&policy, &own), Decision::Allowed, "{action}");
        let other = Request::new(action, "*").with(
            "lambda:FunctionArn",
            "arn:aws:lambda:us-east-1:111122223333:function:someone-elses",
        );
        assert_eq!(evaluate(&policy, &other), Decision::NotAllowed, "{action}");
    }
}

#[test]
fn the_verifier_reads_event_source_mappings_before_it_changes_them() {
    let policy = verifier();
    let read = Request::new(
        "lambda:GetEventSourceMapping",
        "arn:aws:lambda:us-east-1:111122223333:event-source-mapping:abc",
    );
    assert_eq!(evaluate(&policy, &read), Decision::Allowed);
}
