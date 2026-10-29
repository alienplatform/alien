mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

#[test]
fn network_tags_require_creation_or_existing_stack_ownership() {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let policy = AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("network/provision").expect("network permission set"),
            BindingTarget::Stack,
            &context,
        )
        .expect("network policy");
    let own_tags = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ];
    for (resource_type, create_action) in [
        ("vpc", "CreateVpc"),
        ("internet-gateway", "CreateInternetGateway"),
        ("subnet", "CreateSubnet"),
        ("route-table", "CreateRouteTable"),
        ("natgateway", "CreateNatGateway"),
        ("elastic-ip", "AllocateAddress"),
        ("security-group", "CreateSecurityGroup"),
    ] {
        let arn = format!("arn:aws:ec2:us-west-2:123456789012:{resource_type}/example");
        let tagging = || Request::new("ec2:CreateTags", &arn);
        assert_eq!(
            evaluate(&policy.statement, &tagging().request_tags(&own_tags)),
            Decision::NotAllowed,
            "request tags alone must not authorize tagging {resource_type}"
        );
        assert_eq!(
            evaluate(
                &policy.statement,
                &tagging().request_tags(&own_tags).resource_tags(&[
                    (ALIEN_STACK_TAG_KEY, "stack-b"),
                    (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
                ])
            ),
            Decision::NotAllowed,
            "must not retag another stack's {resource_type}"
        );
        assert_eq!(
            evaluate(
                &policy.statement,
                &tagging()
                    .request_tags(&own_tags)
                    .with("ec2:CreateAction", create_action)
            ),
            Decision::Allowed,
            "must allow tags during {create_action}"
        );
        for invalid_action in ["", "CreateTags", "RunInstances"] {
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &tagging()
                        .request_tags(&own_tags)
                        .with("ec2:CreateAction", invalid_action)
                ),
                Decision::NotAllowed,
                "must not authorize {resource_type} tags for {invalid_action:?}"
            );
        }
        assert_eq!(
            evaluate(
                &policy.statement,
                &tagging().with("ec2:CreateAction", create_action)
            ),
            Decision::NotAllowed,
            "creation still requires ownership tags"
        );
        assert_eq!(
            evaluate(
                &policy.statement,
                &tagging()
                    .resource_tags(&own_tags)
                    .request_tags(&[("Name", "renamed")])
            ),
            Decision::Allowed,
            "must allow updating tags on our {resource_type}"
        );
        assert_eq!(
            evaluate(
                &policy.statement,
                &tagging().request_tags(&own_tags).resource_tags(&[
                    (ALIEN_STACK_TAG_KEY, "stack-a"),
                    (ALIEN_MANAGED_BY_TAG_KEY, "setup"),
                ])
            ),
            Decision::NotAllowed,
            "must preserve setup ownership"
        );
    }
}
