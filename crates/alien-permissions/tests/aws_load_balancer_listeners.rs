mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{
        AwsCloudFormationPermissionsGenerator, AwsIamPolicy, AwsRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};
use rstest::rstest;
use serde_json::Value;

const FIPS_POLICY: &str = "ELBSecurityPolicy-TLS13-1-2-FIPS-2023-04";
/// Both FIPS policies a listener may use; the second adds post-quantum key exchange.
const FIPS_POLICIES: [&str; 2] = [FIPS_POLICY, "ELBSecurityPolicy-TLS13-1-2-FIPS-PQ-2025-09"];
const ACCOUNT_PREFIX: &str = "arn:aws:elasticloadbalancing:us-west-2:123456789012";

fn resolve_cloudformation(value: Value, stack: &str) -> Value {
    if let Some(template) = value.get("Fn::Sub") {
        let mut text = template.as_str().expect("string substitution").to_owned();
        for (key, replacement) in [
            ("AWS::Partition", "aws"),
            ("AWS::StackName", stack),
            ("AWS::Region", "us-west-2"),
            ("AWS::AccountId", "123456789012"),
        ] {
            text = text.replace(&format!("${{{key}}}"), replacement);
        }
        assert!(!text.contains("${"), "unresolved substitution: {text}");
        return Value::String(text);
    }
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|value| resolve_cloudformation(value, stack))
            .collect(),
        Value::Object(values) => values
            .into_iter()
            .map(|(key, value)| (key, resolve_cloudformation(value, stack)))
            .collect(),
        other => other,
    }
}

fn policy(set: &str, binding: BindingTarget, cloudformation: bool) -> AwsIamPolicy {
    stack_policy(set, binding, cloudformation, "stack-a")
}

fn stack_policy(
    set: &str,
    binding: BindingTarget,
    cloudformation: bool,
    stack: &str,
) -> AwsIamPolicy {
    let context = PermissionContext::new()
        .with_stack_prefix(stack)
        .with_resource_id("api")
        .with_resource_name(format!("{stack}-api"))
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let set = get_permission_set(set).unwrap();
    if cloudformation {
        let rendered = AwsCloudFormationPermissionsGenerator::new()
            .generate_policy(set, binding, &context.with_stack_prefix(""))
            .unwrap();
        serde_json::from_value(resolve_cloudformation(
            serde_json::to_value(rendered).unwrap(),
            stack,
        ))
        .unwrap()
    } else {
        AwsRuntimePermissionsGenerator::new()
            .generate_policy(set, binding, &context)
            .unwrap()
    }
}

fn tags(stack: &'static str) -> [(&'static str, &'static str); 3] {
    [
        (ALIEN_STACK_TAG_KEY, stack),
        (ALIEN_RESOURCE_TAG_KEY, "api"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ]
}

/// A listener request as the controllers send it, on a load balancer or listener of `owner`.
fn listener_request(action: &'static str, owner: &'static str, load_balancer: &str) -> Request {
    let resource = match action {
        "elasticloadbalancing:CreateListener" => format!("{ACCOUNT_PREFIX}:{load_balancer}"),
        _ => format!(
            "{ACCOUNT_PREFIX}:{}/0123456789abcdef",
            load_balancer.replacen("loadbalancer/", "listener/", 1)
        ),
    };
    let request = Request::new(action, resource).resource_tags(&tags(owner));
    match action {
        "elasticloadbalancing:CreateListener" => request.request_tags(&tags("stack-a")),
        _ => request,
    }
}

// AWS sets elasticloadbalancing:SecurityPolicy and ListenerProtocol only from the request, so
// the requests below name them exactly where the controllers do: an HTTPS CreateListener names
// both, a TCP or HTTP one only the protocol, and ModifyListener only the TLS policy.
#[rstest]
#[case::container_provision("container/provision", true)]
#[case::daemon_provision("daemon/provision", true)]
#[case::container_management("container/management", false)]
fn listener_writes_stay_on_this_stack_with_the_fips_policy(
    #[case] set: &str,
    #[case] creates_listeners: bool,
) {
    let alb = "loadbalancer/app/stack-a-api-alb/50dc6c495c0c9188";
    let other_stack_alb = "loadbalancer/app/stack-b-api-alb/6d0ecf831eec9f09";
    for cloudformation in [false, true] {
        for binding in [BindingTarget::Stack, BindingTarget::Resource] {
            let statements = policy(set, binding, cloudformation).statement;
            let decide = |request: Request| evaluate(&statements, &request);
            let label = format!("{set} {binding:?} cloudformation={cloudformation}");

            let create = || listener_request("elasticloadbalancing:CreateListener", "stack-a", alb);
            let created = if creates_listeners {
                Decision::Allowed
            } else {
                Decision::NotAllowed
            };
            for fips_policy in FIPS_POLICIES {
                assert_eq!(
                    decide(
                        create()
                            .with("elasticloadbalancing:ListenerProtocol", "HTTPS")
                            .with("elasticloadbalancing:SecurityPolicy", fips_policy)
                    ),
                    created,
                    "{label}: HTTPS listener with {fips_policy} on this stack's ALB"
                );
            }
            assert_eq!(
                decide(
                    listener_request(
                        "elasticloadbalancing:CreateListener",
                        "stack-b",
                        other_stack_alb
                    )
                    .with("elasticloadbalancing:ListenerProtocol", "HTTPS")
                    .with("elasticloadbalancing:SecurityPolicy", FIPS_POLICY)
                ),
                Decision::NotAllowed,
                "{label}: listener on another stack's ALB"
            );
            assert_eq!(
                decide(
                    create()
                        .with("elasticloadbalancing:ListenerProtocol", "HTTPS")
                        .with(
                            "elasticloadbalancing:SecurityPolicy",
                            "ELBSecurityPolicy-2016-08"
                        )
                ),
                Decision::NotAllowed,
                "{label}: HTTPS listener with a TLS 1.0 policy"
            );
            assert_eq!(
                decide(create().with("elasticloadbalancing:ListenerProtocol", "HTTPS")),
                Decision::NotAllowed,
                "{label}: HTTPS listener without a TLS policy gets ELBSecurityPolicy-2016-08"
            );
            // The port 80 listener that redirects to HTTPS. IAM cannot see its actions.
            assert_eq!(
                decide(create().with("elasticloadbalancing:ListenerProtocol", "HTTP")),
                created,
                "{label}: HTTP listener on this stack's ALB"
            );
            assert_eq!(
                decide(
                    listener_request(
                        "elasticloadbalancing:CreateListener",
                        "stack-b",
                        other_stack_alb
                    )
                    .with("elasticloadbalancing:ListenerProtocol", "HTTP")
                ),
                Decision::NotAllowed,
                "{label}: HTTP listener on another stack's ALB"
            );

            let modify = || listener_request("elasticloadbalancing:ModifyListener", "stack-a", alb);
            for fips_policy in FIPS_POLICIES {
                assert_eq!(
                    decide(modify().with("elasticloadbalancing:SecurityPolicy", fips_policy)),
                    Decision::Allowed,
                    "{label}: certificate rotation or TLS policy move to {fips_policy}"
                );
            }
            assert_eq!(
                decide(
                    listener_request(
                        "elasticloadbalancing:ModifyListener",
                        "stack-b",
                        other_stack_alb
                    )
                    .with("elasticloadbalancing:SecurityPolicy", FIPS_POLICY)
                ),
                Decision::NotAllowed,
                "{label}: another stack's listener"
            );
            assert_eq!(
                decide(modify().with(
                    "elasticloadbalancing:SecurityPolicy",
                    "ELBSecurityPolicy-2016-08"
                )),
                Decision::NotAllowed,
                "{label}: TLS downgrade"
            );
            assert_eq!(
                decide(modify().with("elasticloadbalancing:ListenerProtocol", "HTTP")),
                Decision::NotAllowed,
                "{label}: switch to plain HTTP"
            );
        }
    }
}

#[test]
fn container_nlb_listeners_are_plain_tcp() {
    let nlb = "loadbalancer/net/stack-a-api-nlb/50dc6c495c0c9188";
    for binding in [BindingTarget::Stack, BindingTarget::Resource] {
        let statements = policy("container/provision", binding, false).statement;
        let create = || listener_request("elasticloadbalancing:CreateListener", "stack-a", nlb);
        assert_eq!(
            evaluate(
                &statements,
                &create().with("elasticloadbalancing:ListenerProtocol", "TCP")
            ),
            Decision::Allowed,
            "{binding:?}: TCP listener"
        );
        assert_eq!(
            evaluate(
                &statements,
                &create().with("elasticloadbalancing:ListenerProtocol", "TLS")
            ),
            Decision::NotAllowed,
            "{binding:?}: TLS listener without a TLS policy"
        );
        assert_eq!(
            evaluate(
                &statements,
                &listener_request("elasticloadbalancing:CreateListener", "stack-b", nlb)
                    .with("elasticloadbalancing:ListenerProtocol", "TCP")
            ),
            Decision::NotAllowed,
            "{binding:?}: TCP listener on another stack's NLB"
        );
    }
}

// Long stack prefixes are compacted into a hashed load balancer name that no longer starts with
// the prefix, so container/provision authorizes its load balancer writes by tag alone.
#[test]
fn container_provision_reaches_compacted_load_balancer_names_by_tag() {
    let context = PermissionContext::new()
        .with_stack_prefix("e2e-03-aws-terraform-tcp-f938d0b8fa")
        .with_resource_id("tcp-service")
        .with_resource_name("e2e-03-aws-terraform-tcp-f938d0b8fa-tcp-service")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let statements = AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("container/provision").unwrap(),
            BindingTarget::Resource,
            &context,
        )
        .unwrap()
        .statement;
    let name = "e2e-03-aws-terrafo-1a2b3c4d-nlb";
    for (stack, decision) in [
        ("e2e-03-aws-terraform-tcp-f938d0b8fa", Decision::Allowed),
        ("other-stack", Decision::NotAllowed),
    ] {
        let tags = [
            (ALIEN_STACK_TAG_KEY, stack),
            (ALIEN_RESOURCE_TAG_KEY, "tcp-service"),
            (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        ];
        for (action, resource) in [
            (
                "elasticloadbalancing:ModifyLoadBalancerAttributes",
                format!("loadbalancer/net/{name}/50dc6c495c0c9188"),
            ),
            (
                "elasticloadbalancing:ModifyTargetGroupAttributes",
                format!("targetgroup/{name}/50dc6c495c0c9188"),
            ),
            (
                "elasticloadbalancing:ModifyListener",
                format!("listener/net/{name}/50dc6c495c0c9188/0123456789abcdef"),
            ),
        ] {
            let request =
                Request::new(action, format!("{ACCOUNT_PREFIX}:{resource}")).resource_tags(&tags);
            assert_eq!(
                evaluate(&statements, &request),
                decision,
                "{stack}: {action}"
            );
        }
        let create = Request::new(
            "elasticloadbalancing:CreateListener",
            format!("{ACCOUNT_PREFIX}:loadbalancer/net/{name}/50dc6c495c0c9188"),
        )
        .resource_tags(&tags)
        .request_tags(&[
            (ALIEN_STACK_TAG_KEY, "e2e-03-aws-terraform-tcp-f938d0b8fa"),
            (ALIEN_RESOURCE_TAG_KEY, "tcp-service"),
            (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        ])
        .with("elasticloadbalancing:ListenerProtocol", "TCP");
        assert_eq!(
            evaluate(&statements, &create),
            decision,
            "{stack}: CreateListener"
        );
    }
}

// Stack names and resource prefixes can be dash-prefixes of each other: "a-prod-api-alb" matches
// "a-*". Stack "a" must reach its own load balancer, listener and security group, and none of
// stack "a-prod"'s.
#[rstest]
#[case::container_provision("container/provision", true)]
#[case::daemon_provision("daemon/provision", true)]
#[case::container_management("container/management", false)]
fn a_stack_cannot_reach_a_stack_whose_name_it_prefixes(
    #[case] set: &str,
    #[case] provisions: bool,
) {
    let tags = |stack: &'static str| {
        [
            (ALIEN_STACK_TAG_KEY, stack),
            (ALIEN_RESOURCE_TAG_KEY, "api"),
            (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        ]
    };
    let requests = |owner: &'static str| {
        let alb = format!("{ACCOUNT_PREFIX}:loadbalancer/app/{owner}-api-alb/50dc6c495c0c9188");
        let listener = format!(
            "{ACCOUNT_PREFIX}:listener/app/{owner}-api-alb/50dc6c495c0c9188/0123456789abcdef"
        );
        let create = || {
            Request::new("elasticloadbalancing:CreateListener", alb.clone())
                .resource_tags(&tags(owner))
                .request_tags(&tags("a"))
        };
        [
            (
                "HTTPS CreateListener",
                provisions,
                create()
                    .with("elasticloadbalancing:ListenerProtocol", "HTTPS")
                    .with("elasticloadbalancing:SecurityPolicy", FIPS_POLICY),
            ),
            (
                "HTTP CreateListener",
                provisions,
                create().with("elasticloadbalancing:ListenerProtocol", "HTTP"),
            ),
            (
                "ModifyListener",
                true,
                Request::new("elasticloadbalancing:ModifyListener", listener)
                    .resource_tags(&tags(owner))
                    .with("elasticloadbalancing:SecurityPolicy", FIPS_POLICY),
            ),
            (
                "AuthorizeSecurityGroupIngress",
                provisions,
                Request::new(
                    "ec2:AuthorizeSecurityGroupIngress",
                    "arn:aws:ec2:us-west-2:123456789012:security-group/sg-0123456789abcdef0",
                )
                .resource_tags(&tags(owner)),
            ),
        ]
    };
    for cloudformation in [false, true] {
        for binding in [BindingTarget::Stack, BindingTarget::Resource] {
            let statements = stack_policy(set, binding, cloudformation, "a").statement;
            let label = format!("{set} {binding:?} cloudformation={cloudformation}");
            for (call, granted, request) in requests("a") {
                let expected = if granted {
                    Decision::Allowed
                } else {
                    Decision::NotAllowed
                };
                assert_eq!(
                    evaluate(&statements, &request),
                    expected,
                    "{label}: {call} on stack a's own resource"
                );
            }
            for (call, _, request) in requests("a-prod") {
                assert_eq!(
                    evaluate(&statements, &request),
                    Decision::NotAllowed,
                    "{label}: {call} on stack a-prod's resource"
                );
            }
        }
    }
}
