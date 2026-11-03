mod iam_policy;

use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{AwsIamPolicy, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("stack-a")
        .with_resource_name("worker")
        .with_resource_id("worker")
        .with_managing_account_id("123456789012")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-west-2")
}

#[test]
fn catalog_tag_grants_require_creation_or_existing_stack_ownership() {
    // Match shared tagging operations across services, including services not yet
    // used by the catalog. Wildcard grants are checked against the same operations.
    const TAG_OPERATIONS: &str = "createtags deletetags tagresource untagresource
        tagresources untagresources addtags removetags addtagstoresource removetagsfromresource
        changetagsforresource createorupdatetags tagrole untagrole taginstanceprofile
        untaginstanceprofile tagpolicy untagpolicy taguser untaguser tagservercertificate
        untagservercertificate tagqueue untagqueue tagloggroup untagloggroup
        addtagstocertificate removetagsfromcertificate putbuckettagging putobjecttagging
        putobjectversiontagging deleteobjecttagging deleteobjectversiontagging";
    let allows_tag_write = |action: &str, include_ec2_create: bool| {
        let action = action.to_ascii_lowercase();
        let (service, operation) = action.split_once(':').unwrap_or(("*", action.as_str()));
        // CodeBuild replaces project tags through UpdateProject, with no
        // separate TagResource permission check.
        TAG_OPERATIONS.split_whitespace().any(|tagging| {
            (include_ec2_create || service != "ec2" || tagging != "createtags")
                && iam_policy::wildcard_match(operation, tagging)
        }) || iam_policy::wildcard_match(&action, "codebuild:updateproject")
            || ["post", "put", "patch", "delete"]
                .iter()
                .any(|method| iam_policy::wildcard_match(&action, &format!("apigateway:{method}")))
    };
    for id in alien_permissions::list_permission_set_ids() {
        let set = get_permission_set(id).expect("registered permission set");
        for entry in set.platforms.aws.iter().flatten() {
            if entry.effect != alien_core::AwsPermissionEffect::Allow
                || !entry
                    .grant
                    .actions
                    .iter()
                    .flatten()
                    .any(|action| allows_tag_write(action, true))
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
                    .is_some_and(|value| value.as_string() == Some("${stackPrefix}"));
                let creation =
                    equals
                        .and_then(|c| c.get("ec2:CreateAction"))
                        .is_some_and(|action| {
                            matches!(
                                action.as_string().unwrap(),
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
                let other_tag_actions = entry
                    .grant
                    .actions
                    .iter()
                    .flatten()
                    .any(|action| allows_tag_write(action, false));
                // CreateAction is supplied by ELB only during resource creation.
                // Request tags alone must never authorize standalone AddTags.
                let elb_creation = entry.grant.actions.as_ref().is_some_and(|actions| {
                    actions.len() == 1 && actions[0] == "elasticloadbalancing:AddTags"
                }) && binding
                    .condition
                    .as_ref()
                    .and_then(|c| c.get("StringLike"))
                    .and_then(|c| c.get("elasticloadbalancing:CreateAction"))
                    .is_some_and(|action| action.as_string() == Some("Create*"))
                    && equals
                        .and_then(|c| c.get("aws:RequestTag/${stackTag}"))
                        .is_some_and(|value| value.as_string() == Some("${stackPrefix}"));
                // Resource bindings use the assigned or imported physical name, which
                // need not share the stack prefix (for example, CloudFormation names).
                let named_resource = binding.not_resources.is_empty()
                    && !binding.resources.is_empty()
                    && binding.resources.iter().all(|arn| {
                        [
                            "arn:aws:dynamodb:${awsRegion}:${awsAccountId}:table/",
                            "arn:aws:s3:::",
                            "arn:aws:sqs:${awsRegion}:${awsAccountId}:",
                            "arn:aws:iam::${awsAccountId}:role/",
                            "arn:aws:iam::${awsAccountId}:policy/",
                            "arn:aws:iam::${awsAccountId}:instance-profile/",
                            "arn:aws:ecr:${awsRegion}:${awsAccountId}:repository/",
                            "arn:aws:codebuild:${awsRegion}:${awsAccountId}:project/",
                            "arn:aws:lambda:${awsRegion}:${awsAccountId}:microvm-image:",
                            "arn:aws:secretsmanager:${awsRegion}:${awsAccountId}:secret:",
                            "arn:aws:elasticloadbalancing:${awsRegion}:${awsAccountId}:targetgroup/",
                            "arn:aws:events:${awsRegion}:${awsAccountId}:rule/",
                            "arn:aws:rds:${awsRegion}:${awsAccountId}:cluster:",
                            "arn:aws:rds:${awsRegion}:${awsAccountId}:db:",
                            "arn:aws:rds:${awsRegion}:${awsAccountId}:subgrp:",
                            "arn:aws:rds:${awsRegion}:${awsAccountId}:cluster-pg:",
                        ].iter().any(|prefix| {
                            arn.strip_prefix(prefix).is_some_and(|name| {
                                name.starts_with("${stackPrefix}-")
                                    || name == "${resourceName}"
                                    || name.starts_with("${resourceName}/")
                                    || name.starts_with("${resourceName}-")
                            })
                        })
                    });
                // MicroVM image creation checks TagResource on "*". Excluding every
                // Lambda ARN permits that check without permitting existing-resource tags.
                let lambda_creation = entry.grant.actions.as_ref().is_some_and(|actions| {
                    actions.len() == 1 && actions[0] == "lambda:TagResource"
                }) && binding.resources.is_empty()
                    && binding.not_resources == ["arn:*:lambda:*:*:*"]
                    && equals
                        .and_then(|c| c.get("aws:RequestTag/${stackTag}"))
                        .is_some_and(|value| value.as_string() == Some("${stackPrefix}"));
                let gateway_request_owned = equals
                    .and_then(|c| c.get("aws:RequestTag/${stackTag}"))
                    .is_some_and(|value| value.as_string() == Some("${stackPrefix}"));
                let gateway_creation = gateway_request_owned
                    && entry.grant.actions.as_ref().is_some_and(|actions| {
                        !actions.is_empty()
                            && actions.iter().all(|action| action == "apigateway:POST")
                    })
                    && binding.not_resources.is_empty()
                    && !binding.resources.is_empty()
                    && binding.resources.iter().all(|arn| {
                        matches!(
                            arn.as_str(),
                            "arn:aws:apigateway:${awsRegion}::/apis"
                                | "arn:aws:apigateway:${awsRegion}::/restapis"
                                | "arn:aws:apigateway:${awsRegion}::/domainnames"
                        )
                    });
                // The tag path does not expose ResourceTag. REST tagging also
                // requires PATCH on the endpoint, checked by this same catalog
                // walk; the compiled operation test below checks both resources.
                let gateway_tag_path = gateway_request_owned
                    && entry.grant.actions.as_ref().is_some_and(|actions| {
                        !actions.is_empty()
                            && actions.iter().all(|action| {
                                matches!(action.as_str(), "apigateway:POST" | "apigateway:PUT")
                            })
                    })
                    && binding.not_resources.is_empty()
                    && binding.resources == ["arn:aws:apigateway:${awsRegion}::/tags/*"];
                assert!(owned || (creation && !other_tag_actions) || named_resource || lambda_creation
                    || elb_creation || gateway_creation || gateway_tag_path,
                    "{id} allows tag writes without creation or existing stack ownership: {binding:?}");
            }
        }
    }
}

#[test]
fn catalog_pass_role_grants_require_the_consuming_service() {
    for id in alien_permissions::list_permission_set_ids() {
        let set = get_permission_set(id).expect("registered permission set");
        let passes_roles = |actions: &[String]| {
            actions.iter().any(|action| {
                iam_policy::wildcard_match(&action.to_ascii_lowercase(), "iam:passrole")
            })
        };
        if !set.platforms.aws.iter().flatten().any(|entry| {
            entry.effect == alien_core::AwsPermissionEffect::Allow
                && entry.grant.actions.as_deref().is_some_and(passes_roles)
        }) {
            continue;
        }
        let service = match id {
            "worker/management" | "worker/provision" | "sandbox/provision" => {
                "lambda.amazonaws.com"
            }
            "build/provision" => "codebuild.amazonaws.com",
            "compute-cluster/management" | "compute-cluster/provision" => "ec2.amazonaws.com",
            "container/provision" => "dlm.amazonaws.com",
            _ => panic!("review the consuming service for {id}"),
        };
        // Render both binding scopes with the boundary selected: legacy grants must
        // disappear, and every remaining Allow must require the consuming service.
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let generated = AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    set,
                    target,
                    &context().with_aws_permissions_boundary_arn(
                        "arn:aws:iam::123456789012:policy/stack-a-boundary",
                    ),
                )
                .unwrap();
            let grants = generated
                .statement
                .iter()
                .filter(|statement| statement.effect == "Allow" && passes_roles(&statement.action))
                .collect::<Vec<_>>();
            assert!(!grants.is_empty(), "{id} / {target}");
            for statement in grants {
                assert_eq!(
                    statement
                        .condition
                        .as_ref()
                        .and_then(|condition| condition.get("StringEquals"))
                        .and_then(|equals| equals.get("iam:PassedToService"))
                        .and_then(|value| value.as_string()),
                    Some(service),
                    "{id} / {target}"
                );
                for resource in &statement.resource {
                    let role = resource.replace('*', "example");
                    for candidate in [service, "unrelated.amazonaws.com", ""] {
                        let request = Request::new("iam:PassRole", &role);
                        let request = if candidate.is_empty() {
                            request
                        } else {
                            request.with("iam:PassedToService", candidate)
                        };
                        assert_eq!(
                            evaluate(&generated.statement, &request) == Decision::Allowed,
                            candidate == service,
                            "{id} / {target}: {candidate}"
                        );
                    }
                }
            }
        }
    }
}

fn policy(id: &str, binding: BindingTarget) -> AwsIamPolicy {
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(get_permission_set(id).unwrap(), binding, &context())
        .unwrap()
}

#[test]
fn ec2_creation_context_does_not_authorize_standalone_tagging() {
    let policy = policy("network/provision", BindingTarget::Stack);
    let tags = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
    ];
    let request = || {
        Request::new(
            "ec2:CreateTags",
            "arn:aws:ec2:us-west-2:123456789012:vpc/example",
        )
    };
    for (creation, allowed) in [("CreateVpc", true), ("RunInstances", false), ("", false)] {
        let result = evaluate(
            &policy.statement,
            &request()
                .request_tags(&tags)
                .with("ec2:CreateAction", creation),
        );
        assert_eq!(result == Decision::Allowed, allowed, "{creation}");
    }
    assert_eq!(
        evaluate(
            &policy.statement,
            &request().with("ec2:CreateAction", "CreateVpc")
        ),
        Decision::NotAllowed
    );
    assert_eq!(
        evaluate(&policy.statement, &request().resource_tags(&tags)),
        Decision::Allowed
    );
}

#[test]
fn certificate_reimport_and_gateway_tagging_require_existing_ownership() {
    let tags = [
        (ALIEN_STACK_TAG_KEY, "stack-a"),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        (alien_core::ALIEN_RESOURCE_TAG_KEY, "worker"),
    ];
    let certificate = "arn:aws:acm:us-west-2:123456789012:certificate/example";
    let domain = "arn:aws:apigateway:us-west-2::/domainnames/example.com";
    let tag_path = "arn:aws:apigateway:us-west-2::/tags/arn%3Aaws%3Aapigateway%3Aus-west-2%3A%3A%2Fdomainnames%2Fexample.com";
    for binding in [BindingTarget::Stack, BindingTarget::Resource] {
        let policy = policy("worker/provision", binding);
        for (owner, manager, allowed) in [
            ("stack-a", "runtime", true),
            ("stack-b", "runtime", false),
            ("stack-a", "setup", false),
            ("", "", false),
        ] {
            let existing = [
                (ALIEN_STACK_TAG_KEY, owner),
                (ALIEN_MANAGED_BY_TAG_KEY, manager),
                (alien_core::ALIEN_RESOURCE_TAG_KEY, "worker"),
            ];
            for (action, arn) in [
                ("acm:ImportCertificate", certificate),
                ("apigateway:PATCH", domain),
            ] {
                // ACM reimport does not accept request tags. API Gateway's endpoint
                // permission must hold even when the caller supplies our tags.
                let mut request = Request::new(action, arn).resource_tags(&existing);
                if action == "apigateway:PATCH" {
                    request = request.request_tags(&tags);
                }
                assert_eq!(
                    evaluate(&policy.statement, &request) == Decision::Allowed,
                    allowed,
                    "{binding:?} {action} {owner}/{manager}"
                );
            }
        }
        // REST tagging requires BOTH PUT on the tag path and PATCH on the endpoint.
        // HTTP API tagging uses the same endpoint check above.
        for (requested, allowed) in [(tags.as_slice(), true), (&[][..], false)] {
            let request = Request::new("apigateway:PUT", tag_path).request_tags(requested);
            assert_eq!(
                evaluate(&policy.statement, &request) == Decision::Allowed,
                allowed
            );
        }
    }
}
