//! The boundary's Deny statements evaluated the way IAM does: a stack's explicit grant of a
//! denied action reaches only that grant's resources, and the other stack resources stay denied.

mod iam_policy;

use alien_core::{PermissionProfile, PermissionSetReference, Stack};
use alien_permissions::{
    generators::{aws_stack_permissions_boundary, AwsIamStatement},
    PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};
use serde_json::{json, Value};

const QUEUE: &str = "arn:aws:sqs:us-east-1:123456789012:app-jobs";

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("app")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
        .with_managing_account_id("210987654321")
}

fn inline(actions: &[&str], resources: &[&str]) -> PermissionSetReference {
    PermissionSetReference::Inline(
        serde_json::from_value(json!({
            "id": "custom/sharing",
            "description": "Test grant",
            "platforms": {"aws": [{
                "grant": {"actions": actions},
                "binding": {"stack": {"resources": resources}}
            }]}
        }))
        .unwrap(),
    )
}

/// The rendered Deny statements, in the shape the evaluator reads.
fn denies(boundary: &Value) -> Vec<AwsIamStatement> {
    boundary["Statement"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|statement| statement["Effect"] == "Deny")
        .map(|statement| {
            let mut statement = statement.clone();
            statement["Sid"] = json!("");
            for key in ["Action", "Resource", "NotResource"] {
                if let Some(value) = statement.get(key).filter(|value| value.is_string()) {
                    statement[key] = json!([value]);
                }
            }
            serde_json::from_value(statement).unwrap()
        })
        .collect()
}

#[test]
fn an_explicit_grant_lifts_a_deny_only_on_its_own_resources() {
    let stack = Stack::new("test".to_string())
        .permission(
            "application",
            PermissionProfile(indexmap::indexmap! {
                "*".to_string() => vec![
                    inline(&["sqs:SetQueueAttributes"], &[QUEUE]),
                    inline(&["s3:*", "*"], &["*"]),
                    PermissionSetReference::from_name("storage/provision"),
                ]
            }),
        )
        .build();
    let denies =
        denies(&aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context())).unwrap());
    let decision = |action, resource: &str| evaluate(&denies, &Request::new(action, resource));

    assert_ne!(
        decision("sqs:SetQueueAttributes", QUEUE),
        Decision::ExplicitlyDenied
    );
    for (action, resource) in [
        (
            "sqs:SetQueueAttributes",
            "arn:aws:sqs:us-east-1:123456789012:app-other",
        ),
        (
            "sqs:CreateQueue",
            "arn:aws:sqs:us-east-1:123456789012:app-new",
        ),
        ("sqs:AddPermission", QUEUE),
        (
            "sns:CreateTopic",
            "arn:aws:sns:us-east-1:123456789012:app-topic",
        ),
        ("s3:PutBucketPolicy", "arn:aws:s3:::app-data"),
        ("s3:PutBucketAcl", "arn:aws:s3:::app-data"),
        ("ram:CreateResourceShare", "*"),
        (
            "es:CreateDomain",
            "arn:aws:es:us-east-1:123456789012:domain/app-search",
        ),
        (
            "es:UpdateElasticsearchDomainConfig",
            "arn:aws:es:us-east-1:123456789012:domain/app-search",
        ),
    ] {
        assert_eq!(
            decision(action, resource),
            Decision::ExplicitlyDenied,
            "{action} on {resource}"
        );
    }
    let key = "arn:aws:kms:us-east-1:123456789012:key/app";
    assert_eq!(
        evaluate(
            &denies,
            &Request::new("kms:CreateGrant", key).with("kms:GrantIsForAWSResource", "false")
        ),
        Decision::ExplicitlyDenied
    );
    assert_ne!(
        evaluate(
            &denies,
            &Request::new("kms:CreateGrant", key).with("kms:GrantIsForAWSResource", "true")
        ),
        Decision::ExplicitlyDenied
    );
}
