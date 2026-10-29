mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{AwsIamPolicy, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

#[test]
fn ec2_tags_require_creation_or_existing_stack_ownership() {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("worker")
        .with_resource_id("worker")
        .with_managing_account_id("123456789012")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    for (set_id, resources) in [
        (
            "network/provision",
            &[
                ("vpc", "CreateVpc"),
                ("internet-gateway", "CreateInternetGateway"),
                ("subnet", "CreateSubnet"),
                ("route-table", "CreateRouteTable"),
                ("natgateway", "CreateNatGateway"),
                ("elastic-ip", "AllocateAddress"),
                ("security-group", "CreateSecurityGroup"),
            ][..],
        ),
        (
            "compute-cluster/provision",
            &[("launch-template", "CreateLaunchTemplate")][..],
        ),
        (
            "compute-cluster/management",
            &[("launch-template", "CreateLaunchTemplate")][..],
        ),
    ] {
        let set = get_permission_set(set_id).expect("permission set");
        for binding in [BindingTarget::Stack, BindingTarget::Resource] {
            // Network permissions only have a stack binding.
            if binding == BindingTarget::Resource && set_id == "network/provision" {
                continue;
            }
            let policy = AwsRuntimePermissionsGenerator::new()
                .generate_policy(set, binding, &context)
                .expect("EC2 policy");
            for (resource_type, create_action) in resources {
                assert_tag_ownership(&policy, resource_type, create_action);
                if *resource_type != "launch-template" {
                    continue;
                }
                let arn = format!("arn:aws:ec2:us-west-2:123456789012:{resource_type}/example");
                assert_eq!(
                    evaluate(
                        &policy.statement,
                        &Request::new("ec2:CreateLaunchTemplate", arn).request_tags(&[
                            (ALIEN_STACK_TAG_KEY, "stack-a"),
                            (ALIEN_MANAGED_BY_TAG_KEY, "runtime")
                        ])
                    ),
                    Decision::Allowed,
                    "{set_id} must still allow {create_action}"
                );
            }
        }
    }
}

fn assert_tag_ownership(policy: &AwsIamPolicy, resource_type: &str, create_action: &str) {
    let own_tags = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ];

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

#[test]
fn catalog_tag_grants_require_creation_or_existing_stack_ownership() {
    for id in alien_permissions::list_permission_set_ids() {
        let set = get_permission_set(id).expect("registered permission set");
        for entry in set.platforms.aws.iter().flatten() {
            if entry.effect != alien_core::AwsPermissionEffect::Allow
                || !entry.grant.actions.iter().flatten().any(|action| {
                    [
                        "ec2:createtags",
                        "acm:addtagstocertificate",
                        "dlm:tagresource",
                        "ses:tagresource",
                        "ses:untagresource",
                    ]
                    .iter()
                    .any(|tagging| {
                        iam_policy::wildcard_match(&action.to_ascii_lowercase(), tagging)
                    })
                })
            {
                continue;
            }
            for binding in [&entry.binding.stack, &entry.binding.resource]
                .into_iter()
                .flatten()
            {
                let equals = binding
                    .condition
                    .as_ref()
                    .and_then(|c| c.get("StringEquals"));
                let owned = equals
                    .and_then(|c| c.get("aws:ResourceTag/${stackTag}"))
                    .is_some_and(|value| value == "${stackPrefix}");
                let creation =
                    equals
                        .and_then(|c| c.get("ec2:CreateAction"))
                        .is_some_and(|action| {
                            matches!(
                                action.as_str(),
                                "CreateVpc"
                                    | "CreateInternetGateway"
                                    | "CreateSubnet"
                                    | "CreateRouteTable"
                                    | "CreateNatGateway"
                                    | "AllocateAddress"
                                    | "CreateSecurityGroup"
                                    | "CreateLaunchTemplate"
                                    | "RunInstances"
                                    | "CreateVolume"
                                    | "CreateSnapshot"
                            )
                        });
                let non_ec2 = entry.grant.actions.iter().flatten().any(|action| {
                    [
                        "acm:addtagstocertificate",
                        "dlm:tagresource",
                        "ses:tagresource",
                        "ses:untagresource",
                    ]
                    .iter()
                    .any(|tagging| {
                        iam_policy::wildcard_match(&action.to_ascii_lowercase(), tagging)
                    })
                });
                assert!(owned || (creation && !non_ec2), "{id} allows tag writes without creation or existing stack ownership: {binding:?}");
            }
        }
    }
}

#[test]
fn certificate_tags_require_existing_stack_ownership() {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("worker")
        .with_resource_id("worker")
        .with_managing_account_id("123456789012")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let arn = "arn:aws:acm:us-west-2:123456789012:certificate/example";
    let own_tags = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        (alien_core::ALIEN_RESOURCE_TAG_KEY, "worker"),
    ];
    for id in [
        "container/provision",
        "container/management",
        "worker/provision",
        "worker/management",
        "daemon/provision",
        "kubernetes-public-endpoint/management",
    ] {
        let set = get_permission_set(id).unwrap();
        for binding in [BindingTarget::Stack, BindingTarget::Resource] {
            if id == "kubernetes-public-endpoint/management" && binding == BindingTarget::Stack {
                continue;
            }
            let policy = AwsRuntimePermissionsGenerator::new()
                .generate_policy(set, binding, &context)
                .unwrap();
            let tagging = || Request::new("acm:AddTagsToCertificate", arn).request_tags(&own_tags);
            for (owner, manager) in [("stack-b", "runtime"), ("stack-a", "setup"), ("", "")] {
                assert_eq!(
                    evaluate(
                        &policy.statement,
                        &tagging().resource_tags(&[
                            (ALIEN_STACK_TAG_KEY, owner),
                            (ALIEN_MANAGED_BY_TAG_KEY, manager),
                            (alien_core::ALIEN_RESOURCE_TAG_KEY, "worker"),
                        ])
                    ),
                    Decision::NotAllowed,
                    "{id}: cannot retag foreign, setup-owned or unowned certificates"
                );
            }
            assert_eq!(
                evaluate(&policy.statement, &tagging().resource_tags(&own_tags)),
                Decision::Allowed,
                "{id}: retag own certificate"
            );
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &Request::new("acm:ImportCertificate", arn).request_tags(&own_tags)
                ),
                Decision::Allowed,
                "{id}: import tagged certificate"
            );
        }
    }
}

#[test]
fn lifecycle_policy_tags_require_existing_stack_ownership() {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("worker")
        .with_resource_id("worker")
        .with_managing_account_id("123456789012")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let arn = "arn:aws:dlm:us-west-2:123456789012:policy/example";
    let own_tags = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        (alien_core::ALIEN_RESOURCE_TAG_KEY, "worker"),
    ];
    for binding in [BindingTarget::Stack, BindingTarget::Resource] {
        let policy = AwsRuntimePermissionsGenerator::new()
            .generate_policy(
                get_permission_set("container/provision").unwrap(),
                binding,
                &context,
            )
            .unwrap();
        let tagging = || Request::new("dlm:TagResource", arn).request_tags(&own_tags);
        for tags in [
            vec![],
            vec![
                (ALIEN_STACK_TAG_KEY, "stack-b"),
                (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
                (alien_core::ALIEN_RESOURCE_TAG_KEY, "worker"),
            ],
            vec![
                (ALIEN_STACK_TAG_KEY, "stack-a"),
                (ALIEN_MANAGED_BY_TAG_KEY, "setup"),
                (alien_core::ALIEN_RESOURCE_TAG_KEY, "worker"),
            ],
        ] {
            assert_eq!(
                evaluate(&policy.statement, &tagging().resource_tags(&tags)),
                Decision::NotAllowed
            );
        }
        assert_eq!(
            evaluate(
                &policy.statement,
                &Request::new("dlm:TagResource", arn)
                    .resource_tags(&own_tags)
                    .request_tags(&[("Name", "renamed")])
            ),
            Decision::Allowed
        );
        assert_eq!(
            evaluate(
                &policy.statement,
                &Request::new("dlm:CreateLifecyclePolicy", "*").request_tags(&own_tags)
            ),
            Decision::Allowed
        );
        assert_eq!(
            evaluate(
                &policy.statement,
                &Request::new("dlm:CreateLifecyclePolicy", "*")
            ),
            Decision::NotAllowed
        );
        if binding == BindingTarget::Resource {
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &tagging().resource_tags(&[
                        (ALIEN_STACK_TAG_KEY, "stack-a"),
                        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
                        (alien_core::ALIEN_RESOURCE_TAG_KEY, "sibling"),
                    ])
                ),
                Decision::NotAllowed,
                "resource binding must preserve resource ownership"
            );
        }
    }
}

#[test]
fn email_tag_changes_require_existing_stack_ownership() {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("stack-a-email")
        .with_resource_id("email")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let set = get_permission_set("email/provision").expect("email provision");
    for binding in [BindingTarget::Stack, BindingTarget::Resource] {
        let policy = AwsRuntimePermissionsGenerator::new()
            .generate_policy(set, binding, &context)
            .expect("email policy");
        for (resource, create_action) in [
            ("identity/mail.example.com", "ses:CreateEmailIdentity"),
            (
                "configuration-set/stack-a-email",
                "ses:CreateConfigurationSet",
            ),
        ] {
            let arn = format!("arn:aws:ses:us-west-2:123456789012:{resource}");
            for action in ["ses:TagResource", "ses:UntagResource"] {
                for existing_stack in [None, Some("stack-b"), Some("stack-a")] {
                    let mut request = Request::new(action, &arn)
                        .request_tags(&[(ALIEN_STACK_TAG_KEY, "stack-a")]);
                    if let Some(stack) = existing_stack {
                        request = request.resource_tags(&[(ALIEN_STACK_TAG_KEY, stack)]);
                    }
                    assert_eq!(
                        evaluate(&policy.statement, &request),
                        if existing_stack == Some("stack-a") {
                            Decision::Allowed
                        } else {
                            Decision::NotAllowed
                        },
                        "{binding:?} {action} on {resource}, existing stack {existing_stack:?}"
                    );
                }
            }
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &Request::new(create_action, &arn)
                        .request_tags(&[(ALIEN_STACK_TAG_KEY, "stack-a")])
                ),
                Decision::Allowed,
                "tagged {create_action} remains available"
            );
        }
    }
}
