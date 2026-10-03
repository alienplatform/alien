//! A deployment's AWS grants match resources by name, `<prefix>-*`. Resource prefixes may contain
//! `-`, so `acme-*` also matches every resource of a deployment with the prefix `acme-prod`. Every
//! deployment tags its resources with its own prefix, and these tests evaluate the generated
//! policies against those tags.

use alien_core::PermissionSet;
use alien_permissions::{
    generators::{
        AwsCloudFormationIamStatement, AwsCloudFormationPermissionsGenerator, AwsIamStatement,
        AwsRuntimePermissionsGenerator,
    },
    get_permission_set, list_permission_set_ids, BindingTarget, PermissionContext,
};
use serde_json::Value;
use std::collections::BTreeSet;

const DEPLOYMENT_TAG: &str = "aws:ResourceTag/deployment";

fn context(prefix: &str) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix(prefix)
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
        .with_managing_account_id("210987654321")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager")
        .with_external_id("external-id")
}

/// `acme`'s grants on its resource `prod`, whose physical names start with `acme-prod`.
fn resource_context(prefix: &str) -> PermissionContext {
    context(prefix)
        .with_resource_id("prod")
        .with_resource_name(format!("{prefix}-prod"))
}

fn aws_sets() -> Vec<&'static PermissionSet> {
    list_permission_set_ids()
        .into_iter()
        .map(|id| get_permission_set(id).expect("listed set resolves"))
        .filter(|set| set.platforms.aws.is_some())
        .collect()
}

fn sets_declaring(target: BindingTarget) -> impl Iterator<Item = &'static PermissionSet> {
    aws_sets().into_iter().filter(move |set| {
        set.platforms
            .aws
            .as_ref()
            .unwrap()
            .iter()
            .all(|permission| match target {
                BindingTarget::Stack => permission.binding.stack.is_some(),
                BindingTarget::Resource => permission.binding.resource.is_some(),
            })
    })
}

fn statements(
    target: BindingTarget,
    context: &PermissionContext,
) -> Vec<(String, AwsIamStatement)> {
    let generator = AwsRuntimePermissionsGenerator::new();
    sets_declaring(target)
        .flat_map(|set| {
            generator
                .generate_policy(set, target, context)
                .unwrap_or_else(|error| panic!("{} renders at {target} scope: {error}", set.id))
                .statement
                .into_iter()
                .map(|statement| (set.id.clone(), statement))
        })
        .collect()
}

/// The stack grants a CloudFormation stack named `stack_name` renders, with its intrinsics
/// resolved the way CloudFormation resolves them.
fn cloudformation_statements(stack_name: &str) -> Vec<(String, AwsIamStatement)> {
    let context = PermissionContext::new()
        .with_stack_prefix("")
        .with_aws_region("${AWS::Region}")
        .with_aws_account_id("${AWS::AccountId}")
        .with_managing_role_arn("${ManagingRoleArn}");
    let generator = AwsCloudFormationPermissionsGenerator::new();
    sets_declaring(BindingTarget::Stack)
        .flat_map(|set| {
            generator
                .generate_policy(set, BindingTarget::Stack, &context)
                .unwrap_or_else(|error| panic!("{} renders for CloudFormation: {error}", set.id))
                .statement
                .into_iter()
                .map(|statement| (set.id.clone(), resolve(statement, stack_name)))
        })
        .collect()
}

fn resolve(statement: AwsCloudFormationIamStatement, stack_name: &str) -> AwsIamStatement {
    let resolve_value = |value: &Value| -> String {
        let template = match value {
            Value::String(plain) => plain.clone(),
            Value::Object(intrinsic) => match intrinsic.get("Fn::Sub") {
                Some(Value::String(template)) => template.clone(),
                Some(Value::Array(template_and_variables)) => {
                    let template = template_and_variables[0].as_str().unwrap();
                    template.replace("${ManagingAccountId}", "210987654321")
                }
                _ => panic!("resolve intrinsic {value}"),
            },
            _ => panic!("resolve value {value}"),
        };
        template
            .replace("${AWS::StackName}", stack_name)
            .replace("${AWS::Partition}", "aws")
            .replace("${AWS::Region}", "us-east-1")
            .replace("${AWS::AccountId}", "123456789012")
    };
    AwsIamStatement {
        sid: statement.sid,
        effect: statement.effect,
        action: statement
            .action
            .iter()
            .map(|action| action.as_str().unwrap().to_string())
            .collect(),
        resource: statement.resource.iter().map(resolve_value).collect(),
        not_resource: statement.not_resource.iter().map(resolve_value).collect(),
        condition: statement.condition.map(|condition| {
            condition
                .into_iter()
                .map(|(operator, entries)| {
                    let entries = entries
                        .iter()
                        .map(|(key, value)| (key.clone(), resolve_value(value)))
                        .collect();
                    (operator, entries)
                })
                .collect()
        }),
    }
}

/// `acme`'s stack grants as the runtime and Terraform render them, and as a CloudFormation stack
/// named `acme` renders them; plus its grants on its own resource `prod`.
fn acme_grants() -> Vec<(String, AwsIamStatement)> {
    [
        ("stack", statements(BindingTarget::Stack, &context("acme"))),
        (
            "resource",
            statements(BindingTarget::Resource, &resource_context("acme")),
        ),
        ("cloudformation", cloudformation_statements("acme")),
    ]
    .into_iter()
    .flat_map(|(renderer, grants)| {
        grants
            .into_iter()
            .map(move |(set, statement)| (format!("{renderer} {set}"), statement))
    })
    .collect()
}

/// IAM resource matching: `*` matches any run of characters and `?` exactly one.
fn arn_matches(pattern: &str, arn: &str) -> bool {
    let (pattern, arn) = (pattern.as_bytes(), arn.as_bytes());
    let (mut p, mut a, mut star, mut resume) = (0, 0, None, 0);
    while a < arn.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == arn[a]) {
            p += 1;
            a += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            resume = a;
        } else if let Some(star) = star {
            p = star + 1;
            resume += 1;
            a = resume;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

/// Evaluates the statement's conditions the way IAM does for the operators permission sets use.
/// A key absent from the request fails `StringEquals` and `StringLike` and passes an `IfExists`
/// operator.
fn conditions_hold(statement: &AwsIamStatement, request: &[(&str, &str)]) -> bool {
    let Some(condition) = &statement.condition else {
        return true;
    };
    condition.iter().all(|(operator, entries)| {
        entries.iter().all(|(key, expected)| {
            let actual = request
                .iter()
                .find(|(request_key, _)| request_key == key)
                .map(|(_, value)| *value);
            match (operator.as_str(), actual) {
                ("StringEquals", Some(actual)) => actual == expected,
                ("StringLike", Some(actual)) => arn_matches(expected, actual),
                ("StringEquals" | "StringLike", None) => false,
                ("StringEqualsIfExists", Some(actual)) => actual == expected,
                ("StringEqualsIfExists", None) => true,
                (operator, _) => panic!("evaluate condition operator {operator}"),
            }
        })
    })
}

fn allows(statement: &AwsIamStatement, arn: &str, request: &[(&str, &str)]) -> bool {
    statement.effect == "Allow"
        && statement
            .resource
            .iter()
            .any(|pattern| arn_matches(pattern, arn))
        && conditions_hold(statement, request)
}

/// One concrete resource name for every name pattern of `acme-prod`'s stack grants.
fn acme_prod_resources() -> BTreeSet<String> {
    statements(BindingTarget::Stack, &context("acme-prod"))
        .into_iter()
        .flat_map(|(_, statement)| statement.resource)
        .filter(|pattern| pattern.contains("acme-prod"))
        .map(|pattern| pattern.replace('*', "x"))
        .collect()
}

/// S3 evaluates `aws:ResourceTag` only for buckets with attribute-based access control enabled.
fn request_carries_resource_tags(arn: &str) -> bool {
    !arn.starts_with("arn:aws:s3:::")
}

#[test]
fn grants_named_after_a_prefix_skip_resources_tagged_for_a_deployment_that_extends_it() {
    let others = acme_prod_resources();
    assert!(
        others.iter().any(|arn| arn.contains(":role/acme-prod-")),
        "the sample covers roles: {others:?}"
    );

    let mut reached = Vec::new();
    for (grant, statement) in acme_grants() {
        for arn in others
            .iter()
            .filter(|arn| request_carries_resource_tags(arn))
        {
            let named_for_acme = statement
                .resource
                .iter()
                .any(|pattern| pattern.contains("acme") && arn_matches(pattern, arn));
            if named_for_acme && allows(&statement, arn, &[(DEPLOYMENT_TAG, "acme-prod")]) {
                reached.push(format!("{grant} {}: {arn}", statement.sid));
            }
        }
    }

    assert!(
        reached.is_empty(),
        "grants of `acme` reach resources tagged for `acme-prod`:\n{}",
        reached.join("\n")
    );
}

#[test]
fn grants_keep_reaching_their_own_tagged_and_untagged_resources() {
    let mut lost = Vec::new();
    for (grant, statement) in acme_grants() {
        for arn in statement
            .resource
            .iter()
            .filter(|pattern| pattern.contains("acme"))
            .map(|pattern| pattern.replace('*', "x"))
        {
            if allows(&statement, &arn, &[])
                && !allows(&statement, &arn, &[(DEPLOYMENT_TAG, "acme")])
            {
                lost.push(format!("{grant} {}: {arn}", statement.sid));
            }
        }
    }
    assert!(
        lost.is_empty(),
        "grants allow an untagged resource but not the same resource tagged for its own deployment:\n{}",
        lost.join("\n")
    );

    let queue = "arn:aws:sqs:us-east-1:123456789012:acme-jobs";
    let publish = statements(BindingTarget::Stack, &context("acme"))
        .into_iter()
        .find(|(set, _)| set == "queue/publish")
        .map(|(_, statement)| statement)
        .expect("queue/publish has a stack grant");
    assert!(
        allows(&publish, queue, &[]),
        "an untagged queue stays reachable"
    );
    assert!(allows(&publish, queue, &[(DEPLOYMENT_TAG, "acme")]));
    assert!(!allows(
        &publish,
        "arn:aws:sqs:us-east-1:123456789012:acme-prod-jobs",
        &[(DEPLOYMENT_TAG, "acme-prod")]
    ));
}

#[test]
fn arn_matching_follows_iam_wildcards() {
    assert!(arn_matches(
        "arn:aws:s3:::acme-*",
        "arn:aws:s3:::acme-prod-x"
    ));
    assert!(arn_matches("role/acme-*-sa", "role/acme-prod-x-sa"));
    assert!(arn_matches("a?c", "abc"));
    assert!(!arn_matches("role/acme-*-sa", "role/acme-x-pull"));
    assert!(!arn_matches("arn:aws:s3:::acme-*", "arn:aws:s3:::acmeprod"));
}
