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
                        "aoss:tagresource",
                        "aoss:untagresource",
                        "apigateway:tagresource",
                        "lambda:tagresource",
                        "lambda:untagresource",
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
                        "aoss:tagresource",
                        "aoss:untagresource",
                        "apigateway:tagresource",
                        "lambda:tagresource",
                        "lambda:untagresource",
                    ]
                    .iter()
                    .any(|tagging| {
                        iam_policy::wildcard_match(&action.to_ascii_lowercase(), tagging)
                    })
                });
                let lambda_tags_only = entry.grant.actions.iter().flatten().all(|action| {
                    matches!(action.as_str(), "lambda:TagResource" | "lambda:ListTags")
                });
                let lambda_prefix = lambda_tags_only
                    && binding.not_resources.is_empty()
                    && !binding.resources.is_empty()
                    && binding.resources.iter().all(|arn| {
                        arn.starts_with("arn:aws:lambda:${awsRegion}:${awsAccountId}:microvm-image:${stackPrefix}-")
                    });
                // MicroVM image creation checks TagResource on "*". Excluding every
                // Lambda ARN permits that check without permitting existing-resource tags.
                let lambda_creation = entry.grant.actions.as_ref().is_some_and(|actions| {
                    actions.len() == 1 && actions[0] == "lambda:TagResource"
                }) && binding.resources.is_empty()
                    && binding.not_resources == ["arn:*:lambda:*:*:*"]
                    && equals
                        .and_then(|c| c.get("aws:RequestTag/${stackTag}"))
                        .is_some_and(|value| value == "${stackPrefix}");
                assert!(owned || (creation && !non_ec2) || lambda_prefix || lambda_creation,
                    "{id} allows tag writes without creation or existing stack ownership: {binding:?}");
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

#[test]
fn api_gateway_tagging_requires_an_owned_endpoint() {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("worker")
        .with_resource_id("worker")
        .with_managing_account_id("123456789012")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let own_tags = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        (alien_core::ALIEN_RESOURCE_TAG_KEY, "worker"),
    ];
    for id in ["worker/provision", "worker/management"] {
        for binding in [BindingTarget::Stack, BindingTarget::Resource] {
            let policy = AwsRuntimePermissionsGenerator::new()
                .generate_policy(get_permission_set(id).unwrap(), binding, &context)
                .unwrap();
            for path in ["apis", "restapis", "domainnames"] {
                let collection = format!("arn:aws:apigateway:us-west-2::/{path}");
                assert_eq!(
                    evaluate(
                        &policy.statement,
                        &Request::new("apigateway:POST", &collection).request_tags(&own_tags)
                    ),
                    Decision::Allowed
                );
                assert_eq!(
                    evaluate(
                        &policy.statement,
                        &Request::new("apigateway:POST", &collection)
                    ),
                    Decision::NotAllowed
                );
                let endpoint = format!("{collection}/example");
                // REST TagResource checks PUT on the tag path and PATCH on the API.
                // HTTP APIs check PATCH directly on the API. ResourceTag is absent
                // on the REST tag path, so the endpoint check enforces ownership.
                let tag_path = format!("arn:aws:apigateway:us-west-2::/tags/arn%3Aaws%3Aapigateway%3Aus-west-2%3A%3A%2F{path}%2Fexample");
                assert_eq!(
                    evaluate(
                        &policy.statement,
                        &Request::new("apigateway:PUT", &tag_path).request_tags(&own_tags)
                    ),
                    Decision::Allowed
                );
                for action in [
                    "apigateway:PATCH",
                    "apigateway:PUT",
                    "apigateway:TagResource",
                ] {
                    for (owner, manager, resource) in [
                        ("stack-b", "runtime", "worker"),
                        ("stack-a", "setup", "worker"),
                        ("", "", ""),
                        ("stack-a", "runtime", "worker"),
                        ("stack-a", "runtime", "sibling"),
                    ] {
                        let allowed = owner == "stack-a"
                            && manager == "runtime"
                            && (binding == BindingTarget::Stack || resource == "worker");
                        let request = Request::new(action, &endpoint)
                            .request_tags(&own_tags)
                            .resource_tags(&[
                                (ALIEN_STACK_TAG_KEY, owner),
                                (ALIEN_MANAGED_BY_TAG_KEY, manager),
                                (alien_core::ALIEN_RESOURCE_TAG_KEY, resource),
                            ]);
                        assert_eq!(
                            evaluate(&policy.statement, &request),
                            if allowed {
                                Decision::Allowed
                            } else {
                                Decision::NotAllowed
                            },
                            "{id} {binding:?} {action} {path} {owner}/{manager}/{resource}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn sandbox_creation_tag_grants_exclude_existing_lambda_resources() {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("sandbox")
        .with_resource_id("sandbox")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    for binding in [BindingTarget::Stack, BindingTarget::Resource] {
        let policy = AwsRuntimePermissionsGenerator::new()
            .generate_policy(
                get_permission_set("sandbox/provision").unwrap(),
                binding,
                &context,
            )
            .unwrap();
        for manager in ["runtime", "setup"] {
            let tagging = |resource: &str| {
                Request::new("lambda:TagResource", resource).request_tags(&[
                    (ALIEN_STACK_TAG_KEY, "stack-a"),
                    (ALIEN_MANAGED_BY_TAG_KEY, manager),
                    (alien_core::ALIEN_RESOURCE_TAG_KEY, "sandbox"),
                ])
            };
            assert_eq!(
                evaluate(&policy.statement, &tagging("*")),
                Decision::Allowed
            );
            assert_eq!(
                evaluate(&policy.statement, &Request::new("lambda:TagResource", "*")),
                Decision::NotAllowed
            );
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &tagging("*").with("aws:RequestTag/deployment", "stack-b")
                ),
                Decision::NotAllowed
            );
            for partition in ["aws", "aws-us-gov", "aws-cn"] {
                for resource in [
                    "function:stack-b-worker",
                    "microvm-image:stack-b-sandbox",
                    "microvm-image:stack-a-sandbox-2",
                    "event-source-mapping:example",
                    "code-signing-config:example",
                    "capacity-provider:example",
                    "network-connector:example",
                    "web-function:example",
                ] {
                    let arn = format!("arn:{partition}:lambda:us-west-2:123456789012:{resource}");
                    let allowed = partition == "aws"
                        && binding == BindingTarget::Stack
                        && resource == "microvm-image:stack-a-sandbox-2";
                    assert_eq!(
                        evaluate(&policy.statement, &tagging(&arn)),
                        if allowed {
                            Decision::Allowed
                        } else {
                            Decision::NotAllowed
                        },
                        "{binding:?} {manager} {arn}"
                    );
                }
            }
            let own = "arn:aws:lambda:us-west-2:123456789012:microvm-image:stack-a-sandbox";
            assert_eq!(
                evaluate(&policy.statement, &tagging(own)),
                Decision::Allowed
            );
            for arn in [
                "arn:aws:lambda:us-east-1:123456789012:microvm-image:stack-a-sandbox",
                "arn:aws:lambda:us-west-2:999999999999:microvm-image:stack-a-sandbox",
            ] {
                assert_eq!(
                    evaluate(&policy.statement, &tagging(arn)),
                    Decision::NotAllowed
                );
            }
        }
    }
}

#[test]
fn opensearch_tag_changes_require_existing_deployment_ownership() {
    let context = PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2");
    let set = get_permission_set("experimental/aws-opensearch/provision").unwrap();
    for binding in [BindingTarget::Stack, BindingTarget::Resource] {
        let policy = AwsRuntimePermissionsGenerator::new()
            .generate_policy(set, binding, &context)
            .unwrap();
        for resource_type in ["collection", "collection-group"] {
            let arn = format!("arn:aws:aoss:us-west-2:123456789012:{resource_type}/assigned-id");
            for action in ["aoss:TagResource", "aoss:UntagResource"] {
                for owner in [None, Some("stack-b"), Some("stack-a")] {
                    for manager in ["setup", "runtime"] {
                        let mut request = Request::new(action, &arn)
                            .request_tags(&[(ALIEN_STACK_TAG_KEY, "stack-a")]);
                        if let Some(owner) = owner {
                            request = request.resource_tags(&[
                                (ALIEN_STACK_TAG_KEY, owner),
                                (ALIEN_MANAGED_BY_TAG_KEY, manager),
                            ]);
                        }
                        assert_eq!(
                            evaluate(&policy.statement, &request),
                            if owner == Some("stack-a") {
                                Decision::Allowed
                            } else {
                                Decision::NotAllowed
                            },
                            "{binding:?} {action} {resource_type} {owner:?}/{manager}"
                        );
                    }
                }
            }
        }
        for action in ["aoss:CreateCollection", "aoss:CreateCollectionGroup"] {
            assert_eq!(
                evaluate(
                    &policy.statement,
                    &Request::new(action, "*").request_tags(&[(ALIEN_STACK_TAG_KEY, "stack-a")])
                ),
                Decision::Allowed
            );
        }
    }
}
