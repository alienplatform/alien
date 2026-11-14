//! Target group names share the stack prefix with every stack whose name extends this one's
//! (`stack-a` and `stack-a-prod`), so the name alone must never let management tag a group.
//! Tags are written only while management creates the group.

mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};
use rstest::rstest;

fn add_tags(group: &str, create_action: Option<&str>) -> Request {
    let request = Request::new(
        "elasticloadbalancing:AddTags",
        format!("arn:aws:elasticloadbalancing:us-west-2:123456789012:targetgroup/{group}/0123456789abcdef"),
    )
    .request_tags(&[
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ]);
    match create_action {
        Some(action) => request.with("elasticloadbalancing:CreateAction", action),
        None => request,
    }
}

#[rstest]
fn compute_management_tags_target_groups_only_when_creating_them(
    #[values(BindingTarget::Stack, BindingTarget::Resource)] binding: BindingTarget,
) {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("stack-a-compute")
        .with_resource_id("compute")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let set = get_permission_set("compute-cluster/management").expect("permission set exists");
    let policy = AwsRuntimePermissionsGenerator::new()
        .generate_policy(set, binding, &context)
        .expect("runtime policy renders");

    // Another stack's existing group whose name starts with this stack's prefix.
    let other = add_tags("stack-a-prod-compute", None);
    assert_ne!(evaluate(&policy.statement, &other), Decision::Allowed);

    let created = add_tags("stack-a-compute", Some("CreateTargetGroup"));
    assert_eq!(evaluate(&policy.statement, &created), Decision::Allowed);
}
