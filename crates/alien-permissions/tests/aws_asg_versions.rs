mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

#[test]
fn bounded_asg_updates_require_numeric_versions_but_allow_capacity_changes() {
    let arn = "arn:aws:autoscaling:us-west-2:123456789012:autoScalingGroup:example:autoScalingGroupName/stack-a-compute";
    let tags = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ];
    for bounded in [false, true] {
        for binding in [BindingTarget::Stack, BindingTarget::Resource] {
            let mut context = PermissionContext::new()
                .with_stack_prefix("stack-a")
                .with_resource_name("compute")
                .with_aws_account_id("123456789012")
                .with_aws_region("us-west-2");
            if bounded {
                context = context.with_aws_permissions_boundary_arn(
                    "arn:aws:iam::123456789012:policy/stack-a-boundary",
                );
            }
            let policy = AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    get_permission_set("compute-cluster/management").unwrap(),
                    binding,
                    &context,
                )
                .unwrap();
            for action in [
                "autoscaling:CreateAutoScalingGroup",
                "autoscaling:UpdateAutoScalingGroup",
            ] {
                // AWS supplies false for floating/omitted template versions, true
                // for numeric versions, and no key for capacity-only updates.
                for version in [Some("true"), Some("false"), None] {
                    for launch_configuration in [false, true] {
                        let mut request = Request::new(action, arn)
                            .resource_tags(&tags)
                            .request_tags(&tags);
                        if let Some(version) = version {
                            request =
                                request.with("autoscaling:LaunchTemplateVersionSpecified", version);
                        }
                        if launch_configuration {
                            request =
                                request.with("autoscaling:LaunchConfigurationName", "old-config");
                        }
                        let allowed = !bounded
                            || (!launch_configuration
                                && (version == Some("true")
                                    || (version.is_none()
                                        && action == "autoscaling:UpdateAutoScalingGroup")));
                        assert_eq!(
                            evaluate(&policy.statement, &request) == Decision::Allowed,
                            allowed,
                            "{bounded}/{binding}/{action}/{version:?}/{launch_configuration}"
                        );
                    }
                }
            }
            for action in [
                "autoscaling:DeleteAutoScalingGroup",
                "autoscaling:SetDesiredCapacity",
                "autoscaling:TerminateInstanceInAutoScalingGroup",
                "autoscaling:StartInstanceRefresh",
            ] {
                let request = Request::new(action, arn).resource_tags(&tags);
                assert_eq!(
                    evaluate(&policy.statement, &request) == Decision::Allowed,
                    !bounded || action != "autoscaling:StartInstanceRefresh",
                    "{bounded}/{binding}/{action}"
                );
                assert_eq!(
                    evaluate(&policy.statement, &Request::new(action, arn)),
                    Decision::NotAllowed,
                    "ownership tags remain required"
                );
            }
        }
    }
}
