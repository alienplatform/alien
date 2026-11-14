//! Load balancer and target group names are limited to 32 characters, so the runtime cuts a long
//! stack prefix and adds a hash: with a 32-character prefix, a container's target group is
//! `<first 20 characters>-<hash>-tg`, a sandbox gateway's `<first 15 characters>-<hash>-tg-3128`.
//! Neither starts with `<prefix>-`. Every call management makes on them is held by the stack's
//! tags, never by the name, so it works for any prefix length.

mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, AwsNetworkScope, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

const ACCOUNT: &str = "123456789012";
const REGION: &str = "us-west-2";
/// The longest prefix the runtime cuts: 32 characters.
const PREFIX: &str = "vpcscope-workload-prod-live-8790";
const SUBNETS: [&str; 2] = ["subnet-0aaaaaaaaaaaaaaa1", "subnet-0aaaaaaaaaaaaaaa2"];
const CONTAINER_TARGET_GROUP: &str = "vpcscope-workload-pr-78338147-tg";
const GATEWAY_TARGET_GROUP: &str = "vpcscope-worklo-1a2b3c4d-tg-3128";
const LOAD_BALANCER: &str = "vpcscope-workload-pr-5e6f7a8b-lb";

fn statements() -> Vec<AwsIamStatement> {
    assert_eq!(PREFIX.len(), 32);
    for name in [CONTAINER_TARGET_GROUP, GATEWAY_TARGET_GROUP, LOAD_BALANCER] {
        assert!(name.len() <= 32 && !name.starts_with(&format!("{PREFIX}-")));
    }
    let context = PermissionContext::new()
        .with_stack_prefix(PREFIX)
        .with_aws_account_id(ACCOUNT)
        .with_aws_region(REGION)
        .with_aws_network(AwsNetworkScope::Vpc {
            vpc_id: "vpc-0aaaaaaaaaaaaaaa1".to_string(),
            subnet_ids: SUBNETS.iter().map(|id| id.to_string()).collect(),
        });
    [
        "compute-cluster/management",
        "container/provision",
        "container/management",
        "daemon/provision",
    ]
    .into_iter()
    .flat_map(|name| {
        let set = get_permission_set(name).expect("permission set exists");
        AwsRuntimePermissionsGenerator::new()
            .generate_policy(set, BindingTarget::Stack, &context)
            .expect("runtime policy renders")
            .statement
    })
    .collect()
}

const STACK_TAGS: [(&str, &str); 2] = [
    (ALIEN_STACK_TAG_KEY, PREFIX),
    (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
];

fn target_group(name: &str) -> String {
    format!("arn:aws:elasticloadbalancing:{REGION}:{ACCOUNT}:targetgroup/{name}/0123456789abcdef")
}

fn load_balancer() -> String {
    format!("arn:aws:elasticloadbalancing:{REGION}:{ACCOUNT}:loadbalancer/app/{LOAD_BALANCER}/0123456789abcdef")
}

fn listener() -> String {
    format!("arn:aws:elasticloadbalancing:{REGION}:{ACCOUNT}:listener/app/{LOAD_BALANCER}/0123456789abcdef/fedcba9876543210")
}

#[test]
fn management_reaches_target_groups_and_load_balancers_with_cut_names() {
    let statements = statements();
    let allowed = |request: Request| {
        let label = format!("{} on {}", request.action, request.resource);
        assert_eq!(
            evaluate(&statements, &request),
            Decision::Allowed,
            "{label}"
        )
    };
    for name in [CONTAINER_TARGET_GROUP, GATEWAY_TARGET_GROUP] {
        allowed(
            Request::new("elasticloadbalancing:CreateTargetGroup", target_group(name))
                .request_tags(&STACK_TAGS),
        );
        allowed(
            Request::new("elasticloadbalancing:AddTags", target_group(name))
                .request_tags(&STACK_TAGS)
                .with("elasticloadbalancing:CreateAction", "CreateTargetGroup"),
        );
        for action in [
            "elasticloadbalancing:ModifyTargetGroupAttributes",
            "elasticloadbalancing:DeleteTargetGroup",
        ] {
            allowed(Request::new(action, target_group(name)).resource_tags(&STACK_TAGS));
        }
    }
    allowed(
        Request::new("elasticloadbalancing:CreateLoadBalancer", load_balancer())
            .request_tags(&STACK_TAGS)
            .with_values("elasticloadbalancing:Subnet", &SUBNETS),
    );
    allowed(
        Request::new("elasticloadbalancing:AddTags", load_balancer())
            .request_tags(&STACK_TAGS)
            .with("elasticloadbalancing:CreateAction", "CreateLoadBalancer"),
    );
    allowed(
        Request::new("elasticloadbalancing:CreateListener", load_balancer())
            .resource_tags(&STACK_TAGS)
            .request_tags(&STACK_TAGS)
            .with("elasticloadbalancing:ListenerProtocol", "HTTPS")
            .with(
                "elasticloadbalancing:SecurityPolicy",
                "ELBSecurityPolicy-TLS13-1-2-FIPS-2023-04",
            ),
    );
    for action in [
        "elasticloadbalancing:ModifyLoadBalancerAttributes",
        "elasticloadbalancing:DeleteLoadBalancer",
    ] {
        allowed(Request::new(action, load_balancer()).resource_tags(&STACK_TAGS));
    }
    for action in [
        "elasticloadbalancing:ModifyListener",
        "elasticloadbalancing:DeleteListener",
    ] {
        allowed(Request::new(action, listener()).resource_tags(&STACK_TAGS));
    }
}

/// The one name pattern this change adds: an Auto Scaling group may join only target groups
/// named `<prefix>-*`. The compute controller creates its groups without target groups (the
/// legacy health-check group, `<prefix>-<cluster>-hc-tg`, is no longer created); container and
/// gateway target groups are registered by their own controllers and never joined to a group.
#[test]
fn auto_scaling_groups_are_created_without_target_groups() {
    let statements = statements();
    let create = || {
        Request::new(
            "autoscaling:CreateAutoScalingGroup",
            format!(
                "arn:aws:autoscaling:{REGION}:{ACCOUNT}:autoScalingGroup:1:autoScalingGroupName/{PREFIX}-compute-general"
            ),
        )
        .with("autoscaling:LaunchTemplateVersionSpecified", "true")
        .with_values("autoscaling:VPCZoneIdentifiers", &SUBNETS)
        .request_tags(&STACK_TAGS)
    };
    assert_eq!(evaluate(&statements, &create()), Decision::Allowed);
    assert_ne!(
        evaluate(
            &statements,
            &create().with_values(
                "autoscaling:TargetGroupARNs",
                &[&target_group(CONTAINER_TARGET_GROUP)]
            )
        ),
        Decision::Allowed
    );
}
