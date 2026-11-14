//! Moving an Auto Scaling group to a new launch template version, or creating one for a new
//! pool, makes AWS authorize `ec2:RunInstances` for the caller against every resource the
//! launch would use. The management identity must pass that check for the compute security
//! group whichever setup created it, and only for this stack's group.

mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{
        AwsCloudFormationPermissionsGenerator, AwsIamPolicy, AwsRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};
use rstest::rstest;
use serde_json::Value;

const SECURITY_GROUP: &str =
    "arn:aws:ec2:us-west-2:123456789012:security-group/sg-0123456789abcdef0";

fn resolve_cloudformation(value: Value) -> Value {
    if let Some(template) = value.get("Fn::Sub") {
        let mut text = template.as_str().expect("string substitution").to_owned();
        for (key, replacement) in [
            ("AWS::Partition", "aws"),
            ("AWS::StackName", "stack-a"),
            ("AWS::Region", "us-west-2"),
            ("AWS::AccountId", "123456789012"),
        ] {
            text = text.replace(&format!("${{{key}}}"), replacement);
        }
        assert!(!text.contains("${"), "unresolved substitution: {text}");
        return Value::String(text);
    }
    match value {
        Value::Array(values) => values.into_iter().map(resolve_cloudformation).collect(),
        Value::Object(values) => values
            .into_iter()
            .map(|(key, value)| (key, resolve_cloudformation(value)))
            .collect(),
        other => other,
    }
}

fn management_policy(cloudformation: bool, binding: BindingTarget) -> AwsIamPolicy {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("compute")
        .with_resource_id("compute")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let set = get_permission_set("compute-cluster/management").expect("permission set exists");
    if cloudformation {
        let rendered = AwsCloudFormationPermissionsGenerator::new()
            .generate_policy(set, binding, &context.with_stack_prefix(""))
            .expect("CloudFormation policy renders");
        serde_json::from_value(resolve_cloudformation(
            serde_json::to_value(rendered).expect("policy serializes"),
        ))
        .expect("resolved policy parses")
    } else {
        AwsRuntimePermissionsGenerator::new()
            .generate_policy(set, binding, &context)
            .expect("runtime policy renders")
    }
}

fn allowed(policy: &AwsIamPolicy, request: &Request) -> bool {
    evaluate(&policy.statement, request) == Decision::Allowed
}

#[rstest]
#[case::runtime_stack(false, BindingTarget::Stack)]
#[case::runtime_resource(false, BindingTarget::Resource)]
#[case::cloudformation_stack(true, BindingTarget::Stack)]
#[case::cloudformation_resource(true, BindingTarget::Resource)]
fn management_can_launch_with_this_stacks_compute_security_group_only(
    #[case] cloudformation: bool,
    #[case] binding: BindingTarget,
) {
    let policy = management_policy(cloudformation, binding);
    let runtime = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ];

    // The rest of the launch passes, so the security group decides the outcome.
    for (resource, resource_tags, request_tags) in [
        (
            "arn:aws:ec2:us-west-2:123456789012:launch-template/lt-0123456789abcdef0",
            &runtime[..],
            &[][..],
        ),
        (
            "arn:aws:ec2:us-west-2::image/ami-0123456789abcdef0",
            &[][..],
            &[][..],
        ),
        (
            "arn:aws:ec2:us-west-2:123456789012:subnet/subnet-0123456789abcdef0",
            &[][..],
            &[][..],
        ),
        (
            "arn:aws:ec2:us-west-2:123456789012:network-interface/*",
            &[][..],
            &[][..],
        ),
        (
            "arn:aws:ec2:us-west-2:123456789012:volume/*",
            &[][..],
            &[][..],
        ),
        (
            "arn:aws:ec2:us-west-2:123456789012:instance/*",
            &[][..],
            &runtime[..],
        ),
    ] {
        let mut request = Request::new("ec2:RunInstances", resource).resource_tags(resource_tags);
        // AWS supplies the VPC of every subnet and network interface a launch uses.
        if resource.contains(":subnet/") || resource.contains(":network-interface/") {
            request = request.with("ec2:Vpc", "arn:aws:ec2:us-west-2:123456789012:vpc/vpc-0123");
        }
        if !request_tags.is_empty() {
            request = request.request_tags(request_tags);
        }
        assert!(allowed(&policy, &request), "launch resource {resource}");
    }

    for (case, tags, expected) in [
        // Created by a direct setup.
        ("runtime-owned", &runtime[..], true),
        // Created by a CloudFormation or Terraform setup.
        (
            "setup-owned",
            &[
                (ALIEN_STACK_TAG_KEY, "stack-a"),
                (ALIEN_MANAGED_BY_TAG_KEY, "setup"),
                ("resource-type", "compute-cluster"),
            ][..],
            true,
        ),
        (
            "another stack's runtime group",
            &[
                (ALIEN_STACK_TAG_KEY, "stack-b"),
                (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
            ][..],
            false,
        ),
        (
            "another stack's setup group",
            &[
                (ALIEN_STACK_TAG_KEY, "stack-b"),
                (ALIEN_MANAGED_BY_TAG_KEY, "setup"),
                ("resource-type", "compute-cluster"),
            ][..],
            false,
        ),
        (
            "a setup group of another resource type",
            &[
                (ALIEN_STACK_TAG_KEY, "stack-a"),
                (ALIEN_MANAGED_BY_TAG_KEY, "setup"),
                ("resource-type", "network"),
            ][..],
            false,
        ),
        (
            "this stack's tag without an owner",
            &[(ALIEN_STACK_TAG_KEY, "stack-a")][..],
            false,
        ),
        ("untagged", &[][..], false),
    ] {
        let request = Request::new("ec2:RunInstances", SECURITY_GROUP).resource_tags(tags);
        assert_eq!(allowed(&policy, &request), expected, "{case}");
    }

    // Using the group in a launch does not let management change or delete it.
    for action in [
        "ec2:AuthorizeSecurityGroupIngress",
        "ec2:RevokeSecurityGroupIngress",
        "ec2:AuthorizeSecurityGroupEgress",
        "ec2:RevokeSecurityGroupEgress",
        "ec2:DeleteSecurityGroup",
        "ec2:CreateTags",
        "ec2:DeleteTags",
    ] {
        let request = Request::new(action, SECURITY_GROUP).resource_tags(&runtime);
        assert!(!allowed(&policy, &request), "{action}");
    }
}
