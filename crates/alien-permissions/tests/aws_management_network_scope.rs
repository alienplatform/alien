//! The management identity creates instances, security groups and load balancers only inside
//! the stack's own network: its VPC, its subnets and, for a network management creates itself,
//! the VPC and subnets that carry the stack's tags.

mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{
        AwsCloudFormationPermissionsGenerator, AwsIamStatement, AwsRuntimePermissionsGenerator,
    },
    get_permission_set, AwsNetworkScope, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};
use rstest::rstest;
use serde_json::Value;

const ACCOUNT: &str = "123456789012";
const REGION: &str = "us-west-2";
const OWN_VPC: &str = "vpc-0aaaaaaaaaaaaaaa1";
const OTHER_VPC: &str = "vpc-0bbbbbbbbbbbbbbb2";
const OWN_SUBNETS: [&str; 2] = ["subnet-0aaaaaaaaaaaaaaa1", "subnet-0aaaaaaaaaaaaaaa2"];
const OTHER_SUBNET: &str = "subnet-0bbbbbbbbbbbbbbb1";

#[derive(Clone, Copy, Debug)]
enum Network {
    /// Setup knows the VPC: it created it, or the installer named it.
    Pinned,
    /// Management creates the network itself.
    Tagged,
    /// The account's default VPC, which setup does not resolve.
    Unknown,
}

fn context(network: Network, boundary: bool) -> PermissionContext {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("stack-a-api")
        .with_resource_id("api")
        .with_aws_account_id(ACCOUNT)
        .with_aws_region(REGION);
    let context = if boundary {
        context.with_aws_permissions_boundary_arn(format!(
            "arn:aws:iam::{ACCOUNT}:policy/stack-a-deployment-management-boundary"
        ))
    } else {
        context
    };
    match network {
        Network::Pinned => context.with_aws_network(AwsNetworkScope::Vpc {
            vpc_id: OWN_VPC.to_string(),
            subnet_ids: OWN_SUBNETS.iter().map(|id| id.to_string()).collect(),
        }),
        Network::Tagged => context.with_aws_network(AwsNetworkScope::StackTagged),
        Network::Unknown => context,
    }
}

/// Resolve the pseudo parameters of a CloudFormation policy the way CloudFormation would.
fn resolve_cloudformation(value: Value) -> Value {
    if let Some(template) = value.get("Fn::Sub") {
        let mut text = template.as_str().expect("string substitution").to_owned();
        for (key, replacement) in [
            ("AWS::Partition", "aws"),
            ("AWS::StackName", "stack-a"),
            ("AWS::Region", REGION),
            ("AWS::AccountId", ACCOUNT),
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

/// The management statements that create network-facing resources, as either generator renders
/// them.
fn statements(
    cloudformation: bool,
    binding: BindingTarget,
    network: Network,
    boundary: bool,
) -> Vec<AwsIamStatement> {
    let context = context(network, boundary);
    ["compute-cluster/management", "container/provision"]
        .into_iter()
        .flat_map(|id| {
            let set = get_permission_set(id).expect("permission set exists");
            if cloudformation {
                let rendered = AwsCloudFormationPermissionsGenerator::new()
                    .generate_policy(set, binding, &context.clone().with_stack_prefix(""))
                    .expect("CloudFormation policy renders");
                let policy: alien_permissions::generators::AwsIamPolicy =
                    serde_json::from_value(resolve_cloudformation(
                        serde_json::to_value(rendered).expect("policy serializes"),
                    ))
                    .expect("resolved policy parses");
                policy.statement
            } else {
                AwsRuntimePermissionsGenerator::new()
                    .generate_policy(set, binding, &context)
                    .expect("runtime policy renders")
                    .statement
            }
        })
        .collect()
}

fn ec2(kind: &str, id: &str) -> String {
    format!("arn:aws:ec2:{REGION}:{ACCOUNT}:{kind}/{id}")
}

fn vpc_arn(id: &str) -> String {
    ec2("vpc", id)
}

fn allowed(statements: &[AwsIamStatement], request: &Request) -> bool {
    evaluate(statements, request) == Decision::Allowed
}

fn launch_subnet(subnet: &str, vpc: &str) -> Request {
    Request::new("ec2:RunInstances", ec2("subnet", subnet)).with("ec2:Vpc", vpc_arn(vpc))
}

fn launch_interface(vpc: &str) -> Request {
    Request::new("ec2:RunInstances", ec2("network-interface", "*")).with("ec2:Vpc", vpc_arn(vpc))
}

fn security_group_in(vpc: &str) -> Request {
    Request::new("ec2:CreateSecurityGroup", ec2("vpc", vpc)).with("ec2:VpcID", vpc)
}

fn new_security_group_in(vpc: &str) -> Request {
    Request::new("ec2:CreateSecurityGroup", ec2("security-group", "*"))
        .with("ec2:Vpc", vpc_arn(vpc))
        .request_tags(&[
            (ALIEN_STACK_TAG_KEY, "stack-a"),
            (ALIEN_RESOURCE_TAG_KEY, "api"),
            (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        ])
}

fn load_balancer_in(subnets: &[&str]) -> Request {
    let request = Request::new(
        "elasticloadbalancing:CreateLoadBalancer",
        format!("arn:aws:elasticloadbalancing:{REGION}:{ACCOUNT}:loadbalancer/app/stack-a-api/1"),
    )
    .request_tags(&[
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_RESOURCE_TAG_KEY, "api"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ]);
    if subnets.is_empty() {
        request
    } else {
        request.with_values("elasticloadbalancing:Subnet", subnets)
    }
}

fn auto_scaling_group(action: &'static str, subnets: &[&str]) -> Request {
    let request = Request::new(
        action,
        format!(
            "arn:aws:autoscaling:{REGION}:{ACCOUNT}:autoScalingGroup:1:autoScalingGroupName/stack-a-compute-general"
        ),
    )
    .with("autoscaling:LaunchTemplateVersionSpecified", "true");
    let request = if subnets.is_empty() {
        request
    } else {
        request.with_values("autoscaling:VPCZoneIdentifiers", subnets)
    };
    if action == "autoscaling:CreateAutoScalingGroup" {
        request.request_tags(&[
            (ALIEN_STACK_TAG_KEY, "stack-a"),
            (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        ])
    } else {
        request.resource_tags(&[
            (ALIEN_STACK_TAG_KEY, "stack-a"),
            (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        ])
    }
}

#[rstest]
fn launches_stay_in_the_stacks_vpc(
    #[values(false, true)] cloudformation: bool,
    #[values(BindingTarget::Stack, BindingTarget::Resource)] binding: BindingTarget,
) {
    let statements = statements(cloudformation, binding, Network::Pinned, true);
    assert!(allowed(
        &statements,
        &launch_subnet(OWN_SUBNETS[0], OWN_VPC)
    ));
    assert!(allowed(&statements, &launch_interface(OWN_VPC)));
    assert!(!allowed(
        &statements,
        &launch_subnet(OTHER_SUBNET, OTHER_VPC)
    ));
    assert!(!allowed(&statements, &launch_interface(OTHER_VPC)));
}

#[rstest]
fn security_groups_are_created_only_in_the_stacks_vpc(
    #[values(false, true)] cloudformation: bool,
    #[values(BindingTarget::Stack, BindingTarget::Resource)] binding: BindingTarget,
) {
    let statements = statements(cloudformation, binding, Network::Pinned, true);
    assert!(allowed(&statements, &security_group_in(OWN_VPC)));
    assert!(allowed(&statements, &new_security_group_in(OWN_VPC)));
    assert!(!allowed(&statements, &security_group_in(OTHER_VPC)));
    assert!(!allowed(&statements, &new_security_group_in(OTHER_VPC)));
}

#[rstest]
fn load_balancers_are_created_only_in_the_stacks_subnets(
    #[values(false, true)] cloudformation: bool,
    #[values(BindingTarget::Stack, BindingTarget::Resource)] binding: BindingTarget,
) {
    let statements = statements(cloudformation, binding, Network::Pinned, true);
    assert!(allowed(&statements, &load_balancer_in(&OWN_SUBNETS)));
    assert!(allowed(&statements, &load_balancer_in(&OWN_SUBNETS[..1])));
    assert!(!allowed(
        &statements,
        &load_balancer_in(&[OWN_SUBNETS[0], OTHER_SUBNET])
    ));
    assert!(!allowed(&statements, &load_balancer_in(&[OTHER_SUBNET])));
    assert!(!allowed(&statements, &load_balancer_in(&[])));
}

#[rstest]
fn auto_scaling_groups_are_created_only_in_the_stacks_subnets(
    #[values(false, true)] cloudformation: bool,
    #[values(false, true)] boundary: bool,
) {
    let statements = statements(
        cloudformation,
        BindingTarget::Stack,
        Network::Pinned,
        boundary,
    );
    let create =
        |subnets: &[&str]| auto_scaling_group("autoscaling:CreateAutoScalingGroup", subnets);
    assert!(allowed(&statements, &create(&OWN_SUBNETS)));
    assert!(!allowed(
        &statements,
        &create(&[OWN_SUBNETS[0], OTHER_SUBNET])
    ));
    assert!(!allowed(&statements, &create(&[OTHER_SUBNET])));
    // A group created with only availability zones lands in the account's default VPC.
    assert!(!allowed(&statements, &create(&[])));
}

/// AWS checks a group's subnets against the caller's launch grant when the group is created,
/// not when an update moves it, so an update may not name subnets in any network mode.
#[rstest]
fn auto_scaling_group_updates_keep_the_groups_subnets(
    #[values(false, true)] cloudformation: bool,
    #[values(false, true)] boundary: bool,
    #[values(Network::Pinned, Network::Tagged, Network::Unknown)] network: Network,
) {
    let statements = statements(cloudformation, BindingTarget::Stack, network, boundary);
    let update =
        |subnets: &[&str]| auto_scaling_group("autoscaling:UpdateAutoScalingGroup", subnets);
    assert!(allowed(&statements, &update(&[])));
    assert!(!allowed(&statements, &update(&[OTHER_SUBNET])));
    assert!(!allowed(&statements, &update(&OWN_SUBNETS)));
}

#[rstest]
fn a_network_management_creates_is_scoped_by_its_tags(
    #[values(BindingTarget::Stack, BindingTarget::Resource)] binding: BindingTarget,
) {
    let statements = statements(false, binding, Network::Tagged, true);
    let own = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ];
    let other = [
        (ALIEN_STACK_TAG_KEY, "stack-b"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ];
    let subnet = |tags: &[(&str, &str)]| {
        Request::new("ec2:RunInstances", ec2("subnet", OTHER_SUBNET))
            .with("ec2:Vpc", vpc_arn(OTHER_VPC))
            .resource_tags(tags)
    };
    assert!(allowed(&statements, &subnet(&own)));
    assert!(!allowed(&statements, &subnet(&other)));
    assert!(!allowed(&statements, &subnet(&[])));

    let vpc = |tags: &[(&str, &str)]| {
        Request::new("ec2:CreateSecurityGroup", ec2("vpc", OTHER_VPC))
            .with("ec2:VpcID", OTHER_VPC)
            .resource_tags(tags)
    };
    assert!(allowed(&statements, &vpc(&own)));
    assert!(!allowed(&statements, &vpc(&other)));
    assert!(!allowed(&statements, &vpc(&[])));

    // A launch creates its interface untagged; an existing interface must be the stack's.
    assert!(allowed(&statements, &launch_interface(OTHER_VPC)));
    assert!(!allowed(
        &statements,
        &launch_interface(OTHER_VPC).resource_tags(&other)
    ));
}

/// Without a VPC setup can name (the account's default VPC), the conditions render as
/// wildcards: the same reach as before network scoping.
#[rstest]
fn an_unknown_network_keeps_the_account_wide_reach(#[values(false, true)] cloudformation: bool) {
    let statements = statements(cloudformation, BindingTarget::Stack, Network::Unknown, true);
    assert!(allowed(
        &statements,
        &launch_subnet(OTHER_SUBNET, OTHER_VPC)
    ));
    assert!(allowed(&statements, &security_group_in(OTHER_VPC)));
    assert!(allowed(&statements, &new_security_group_in(OTHER_VPC)));
    assert!(allowed(&statements, &load_balancer_in(&[OTHER_SUBNET])));
}

#[test]
fn the_runtime_policy_names_the_stacks_vpc_and_subnets() {
    let statements = statements(false, BindingTarget::Stack, Network::Pinned, true);
    let rendered = serde_json::to_string(&statements).expect("statements serialize");
    assert!(rendered.contains(&format!(
        "\"ec2:Vpc\":\"arn:aws:ec2:{REGION}:{ACCOUNT}:vpc/{OWN_VPC}\""
    )));
    assert!(rendered.contains(&format!("\"ec2:VpcID\":\"{OWN_VPC}\"")));
    assert!(rendered.contains(&format!(
        "\"elasticloadbalancing:Subnet\":[\"{}\",\"{}\"]",
        OWN_SUBNETS[0], OWN_SUBNETS[1]
    )));
}
