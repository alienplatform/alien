//! What the verifier may do to Auto Scaling groups on a protected AWS install, evaluated the way
//! IAM does: the group's name prefix and its tags both have to be the install's.

mod iam_policy;

use alien_permissions::generators::{signing_limits::aws_verifier_policy, AwsIamStatement};
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
