use alien_core::{AwsBindingSpec, AwsPermissionEffect};
use alien_permissions::{get_permission_set, list_permission_set_ids};

const RUNTIME_AWS_PERMISSION_SETS: &[&str] = &[
    "artifact-registry/provision",
    "container/provision",
    "container/management",
    "compute-cluster/provision",
    "compute-cluster/management",
    "compute-cluster/execute",
    "worker/provision",
    "worker/management",
    "kv/provision",
    "network/provision",
    "queue/provision",
    "storage/provision",
    "vault/provision",
];

#[test]
fn runtime_aws_system_generated_resources_are_abac_guarded() {
    let mut failures = Vec::new();

    for permission_set_id in RUNTIME_AWS_PERMISSION_SETS {
        let permission_set = get_permission_set(permission_set_id)
            .unwrap_or_else(|| panic!("missing permission set {permission_set_id}"));
        let Some(aws_permissions) = permission_set.platforms.aws.as_ref() else {
            continue;
        };

        for (statement_index, permission) in aws_permissions.iter().enumerate() {
            if permission.effect == AwsPermissionEffect::Deny {
                continue;
            }

            let actions = permission
                .grant
                .actions
                .as_ref()
                .unwrap_or_else(|| panic!("{permission_set_id} AWS permission has no actions"));

            for binding in [
                permission.binding.stack.as_ref(),
                permission.binding.resource.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                for action in actions {
                    if binding.resources.iter().any(|resource| resource == "*") {
                        if wildcard_action_allowed(action, binding) {
                            continue;
                        }

                        failures.push(format!(
                            "{permission_set_id}[{statement_index}] action {action} uses Resource \"*\" without an allowed read/cross-account/request-tag exception"
                        ));
                        continue;
                    }

                    if !binding.resources.iter().any(|resource| {
                        resource_requires_abac(resource) || lambda_resource_requires_abac(resource)
                    }) {
                        continue;
                    }

                    if documented_create_security_group_vpc_authorization(action, binding) {
                        continue;
                    }

                    let has_request_tag = has_condition_key(binding, "aws:RequestTag/${stackTag}");
                    let has_resource_tag =
                        has_condition_key(binding, "aws:ResourceTag/${stackTag}");

                    if action_requires_tag_condition(action)
                        && !(has_request_tag || has_resource_tag)
                    {
                        failures.push(format!(
                            "{permission_set_id}[{statement_index}] action {action} on {:?} is missing a stack tag condition",
                            binding.resources
                        ));
                    }
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "AWS ABAC validation failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn aws_tag_tamper_protection_is_not_a_builtin_boundary_layer() {
    assert!(
        get_permission_set("aws/tag-tamper-protection").is_none(),
        "AWS isolation should be enforced by scoped permission sets, not a compensating tag-tamper Deny"
    );
}

#[test]
fn kv_and_queue_management_do_not_mutate_tags() {
    let cases: [(&str, &[&str]); 2] = [
        (
            "kv/management",
            &["dynamodb:TagResource", "dynamodb:UntagResource"],
        ),
        ("queue/management", &["sqs:TagQueue", "sqs:UntagQueue"]),
    ];

    for (permission_set_id, forbidden_actions) in cases {
        let permission_set = get_permission_set(permission_set_id)
            .unwrap_or_else(|| panic!("missing permission set {permission_set_id}"));
        let aws_permissions = permission_set
            .platforms
            .aws
            .as_ref()
            .unwrap_or_else(|| panic!("{permission_set_id} must have AWS permissions"));
        let actions = aws_permissions
            .iter()
            .flat_map(|permission| permission.grant.actions.as_deref().unwrap_or_default())
            .collect::<Vec<_>>();

        for forbidden_action in forbidden_actions {
            assert!(
                !actions
                    .iter()
                    .any(|action| action.as_str() == *forbidden_action),
                "{permission_set_id} should not include tag mutation action {forbidden_action}"
            );
        }
    }

    let queue_management =
        get_permission_set("queue/management").expect("queue/management permission set must exist");
    let queue_actions = queue_management
        .platforms
        .aws
        .as_ref()
        .expect("queue/management must have AWS permissions")
        .iter()
        .flat_map(|permission| permission.grant.actions.as_deref().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(
        queue_actions
            .iter()
            .any(|action| action.as_str() == "sqs:PurgeQueue"),
        "queue/management should retain PurgeQueue"
    );
}

#[test]
fn provision_sets_do_not_grant_tag_removal() {
    let cases: [(&str, &[&str]); 6] = [
        ("artifact-registry/provision", &["ecr:UntagResource"]),
        ("kv/provision", &["dynamodb:UntagResource"]),
        ("network/provision", &["ec2:DeleteTags"]),
        ("queue/provision", &["sqs:UntagQueue"]),
        ("service-account/provision", &["iam:UntagRole"]),
        ("worker/provision", &["lambda:UntagResource"]),
    ];

    for (permission_set_id, forbidden_actions) in cases {
        let permission_set = get_permission_set(permission_set_id)
            .unwrap_or_else(|| panic!("missing permission set {permission_set_id}"));
        let aws_permissions = permission_set
            .platforms
            .aws
            .as_ref()
            .unwrap_or_else(|| panic!("{permission_set_id} must have AWS permissions"));
        let actions = aws_permissions
            .iter()
            .flat_map(|permission| permission.grant.actions.as_deref().unwrap_or_default())
            .collect::<Vec<_>>();

        for forbidden_action in forbidden_actions {
            assert!(
                !actions
                    .iter()
                    .any(|action| action.as_str() == *forbidden_action),
                "{permission_set_id} should not include tag removal action {forbidden_action}"
            );
        }
    }
}

#[test]
fn kubernetes_public_endpoint_acm_permissions_are_resource_scoped() {
    let permission_set = get_permission_set("kubernetes-public-endpoint/management")
        .expect("permission set must exist");
    let aws_permissions = permission_set
        .platforms
        .aws
        .as_ref()
        .expect("permission set must have AWS permissions");

    assert_eq!(aws_permissions.len(), 2);
    for permission in aws_permissions {
        assert!(
            permission.binding.stack.is_none(),
            "Kubernetes endpoint ACM permissions must be attached to concrete public resources"
        );
        let binding = permission
            .binding
            .resource
            .as_ref()
            .expect("resource binding required");
        assert_eq!(
            binding.resources,
            ["arn:aws:acm:${awsRegion}:${awsAccountId}:certificate/*"]
        );

        let actions = permission.grant.actions.as_ref().expect("actions required");
        if actions
            .iter()
            .any(|action| action == "acm:DeleteCertificate")
        {
            assert!(has_condition_key(binding, "aws:ResourceTag/${stackTag}"));
            assert!(has_condition_key(binding, "aws:ResourceTag/${resourceTag}"));
            assert!(has_condition_key(
                binding,
                "aws:ResourceTag/${managedByTag}"
            ));
        } else {
            assert!(has_condition_key(binding, "aws:RequestTag/${stackTag}"));
            assert!(has_condition_key(binding, "aws:RequestTag/${resourceTag}"));
            assert!(has_condition_key(binding, "aws:RequestTag/${managedByTag}"));
        }
    }
}

#[test]
fn aws_resource_arns_are_stack_or_resource_scoped_unless_documented_external() {
    let mut failures = Vec::new();

    for permission_set_id in list_permission_set_ids() {
        let permission_set = get_permission_set(permission_set_id)
            .unwrap_or_else(|| panic!("missing permission set {permission_set_id}"));
        let Some(aws_permissions) = permission_set.platforms.aws.as_ref() else {
            continue;
        };

        for (statement_index, permission) in aws_permissions.iter().enumerate() {
            if permission.effect == AwsPermissionEffect::Deny {
                continue;
            }

            let actions = permission
                .grant
                .actions
                .as_deref()
                .unwrap_or_else(|| panic!("{permission_set_id} AWS permission has no actions"));

            for binding in [
                permission.binding.stack.as_ref(),
                permission.binding.resource.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                for resource in &binding.resources {
                    if resource == "*"
                        || resource.contains("${stackPrefix}")
                        || resource.contains("${resourceName}")
                        || binding.condition.is_some()
                        || documented_external_resource_scope(resource)
                        || documented_run_instances_companion_resource(actions, resource)
                        || documented_create_security_group_vpc_resource(actions, resource)
                    {
                        continue;
                    }

                    failures.push(format!(
                        "{permission_set_id}[{statement_index}] uses unscoped AWS resource ARN {resource}"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "AWS resource ARN scoping validation failed:\n{}",
        failures.join("\n")
    );
}

fn documented_external_resource_scope(resource: &str) -> bool {
    // Runtime compute pulls images from the manager-owned artifact registry. Target-account
    // isolation is enforced by the repository resource policy that grants this role access.
    resource == "arn:aws:ecr:*:${managingAccountId}:repository/*"
}

fn documented_run_instances_companion_resource(actions: &[String], resource: &str) -> bool {
    if actions.iter().any(|action| action != "ec2:RunInstances") {
        return false;
    }

    // EC2 Auto Scaling validates launch template use with an ec2:RunInstances
    // dry run. IAM evaluates that action against the public AMI, selected subnet,
    // and create-side EC2 resources as well as the tagged launch template.
    resource == "arn:aws:ec2:${awsRegion}::image/*"
        || resource == "arn:aws:ec2:${awsRegion}:${awsAccountId}:subnet/*"
        || resource == "arn:aws:ec2:${awsRegion}:${awsAccountId}:network-interface/*"
        || resource == "arn:aws:ec2:${awsRegion}:${awsAccountId}:volume/*"
}

fn documented_create_security_group_vpc_resource(actions: &[String], resource: &str) -> bool {
    actions
        .iter()
        .all(|action| action == "ec2:CreateSecurityGroup")
        && documented_create_security_group_authorization_resource(resource)
}

fn documented_create_security_group_vpc_authorization(
    action: &str,
    binding: &AwsBindingSpec,
) -> bool {
    action == "ec2:CreateSecurityGroup"
        && binding
            .resources
            .iter()
            .any(|resource| documented_create_security_group_authorization_resource(resource))
}

fn documented_create_security_group_authorization_resource(resource: &str) -> bool {
    matches!(
        resource,
        "arn:aws:ec2:${awsRegion}:${awsAccountId}:security-group/*"
            | "arn:aws:ec2:${awsRegion}:${awsAccountId}:vpc/*"
    )
}

#[test]
fn aws_wildcard_resources_are_read_only_or_conditioned_unless_documented() {
    let mut failures = Vec::new();

    for permission_set_id in list_permission_set_ids() {
        let permission_set = get_permission_set(permission_set_id)
            .unwrap_or_else(|| panic!("missing permission set {permission_set_id}"));
        let Some(aws_permissions) = permission_set.platforms.aws.as_ref() else {
            continue;
        };

        for (statement_index, permission) in aws_permissions.iter().enumerate() {
            if permission.effect == AwsPermissionEffect::Deny {
                continue;
            }

            let actions = permission
                .grant
                .actions
                .as_ref()
                .unwrap_or_else(|| panic!("{permission_set_id} AWS permission has no actions"));

            for binding in [
                permission.binding.stack.as_ref(),
                permission.binding.resource.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if !binding.resources.iter().any(|resource| resource == "*") {
                    continue;
                }

                for action in actions {
                    if wildcard_action_allowed(action, binding) {
                        continue;
                    }

                    failures.push(format!(
                        "{permission_set_id}[{statement_index}] action {action} uses Resource \"*\" without an allowed read/cross-account/request-tag exception"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "AWS wildcard resource validation failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn dynamodb_permissions_do_not_depend_on_abac() {
    let dynamodb_sets = [
        "kv/heartbeat",
        "kv/management",
        "kv/provision",
        "kv/data-read",
        "kv/data-write",
    ];

    let mut failures = Vec::new();
    for permission_set_id in dynamodb_sets {
        let permission_set = get_permission_set(permission_set_id)
            .unwrap_or_else(|| panic!("missing permission set {permission_set_id}"));
        let Some(aws_permissions) = permission_set.platforms.aws.as_ref() else {
            continue;
        };

        for permission in aws_permissions {
            for binding in [
                permission.binding.stack.as_ref(),
                permission.binding.resource.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if has_condition_key(binding, "aws:ResourceTag/${stackTag}")
                    || has_condition_key(binding, "aws:RequestTag/${stackTag}")
                {
                    failures.push(format!(
                        "{permission_set_id} uses DynamoDB tag conditions; add an AWS account preflight before relying on DynamoDB ABAC"
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "DynamoDB ABAC guard failed:\n{}",
        failures.join("\n")
    );
}

fn resource_requires_abac(resource: &str) -> bool {
    (resource.contains(":ec2:")
        && (resource.contains(":volume/")
            || resource.contains(":instance/")
            || resource.contains(":launch-template/")
            || resource.contains(":security-group/")
            || resource.contains(":vpc/")
            || resource.contains(":internet-gateway/")
            || resource.contains(":subnet/")
            || resource.contains(":route-table/")
            || resource.contains(":natgateway/")
            || resource.contains(":elastic-ip/")))
        || resource.contains(":acm:")
        || resource.contains(":apigateway:")
        || resource.contains(":elasticloadbalancing:")
        || resource.contains(":autoscaling:")
        || resource.contains(":events:")
}

fn lambda_resource_requires_abac(resource: &str) -> bool {
    resource.contains(":lambda:") && resource.contains(":function:")
}

fn has_condition_key(binding: &AwsBindingSpec, expected_key: &str) -> bool {
    binding.condition.as_ref().is_some_and(|conditions| {
        conditions
            .values()
            .any(|condition_values| condition_values.keys().any(|key| key == expected_key))
    })
}

fn wildcard_action_allowed(action: &str, binding: &AwsBindingSpec) -> bool {
    action_is_forced_wildcard_read(action)
        || action_is_documented_cross_account_exception(action)
        || (action_requires_tag_condition(action)
            && has_condition_key(binding, "aws:RequestTag/${stackTag}"))
}

fn action_is_forced_wildcard_read(action: &str) -> bool {
    if matches!(action, "ec2:DescribeVpcAttribute" | "ec2:GetConsoleOutput") {
        return false;
    }

    let (_, name) = action
        .split_once(':')
        .unwrap_or_else(|| panic!("invalid AWS action {action}"));
    name.starts_with("Describe")
        || name.starts_with("List")
        || name.starts_with("Get")
        || name == "LookupEvents"
}

fn action_is_documented_cross_account_exception(action: &str) -> bool {
    matches!(action, "ecr:GetAuthorizationToken")
}

fn action_requires_tag_condition(action: &str) -> bool {
    matches!(
        action,
        "acm:ImportCertificate"
            | "acm:AddTagsToCertificate"
            | "acm:DeleteCertificate"
            | "apigateway:POST"
            | "apigateway:PUT"
            | "apigateway:TagResource"
            | "autoscaling:CreateAutoScalingGroup"
            | "autoscaling:DeleteAutoScalingGroup"
            | "autoscaling:SetDesiredCapacity"
            | "autoscaling:StartInstanceRefresh"
            | "autoscaling:UpdateAutoScalingGroup"
            | "ec2:AllocateAddress"
            | "ec2:AssociateRouteTable"
            | "ec2:AttachInternetGateway"
            | "ec2:AuthorizeSecurityGroupEgress"
            | "ec2:AuthorizeSecurityGroupIngress"
            | "ec2:CreateInternetGateway"
            | "ec2:CreateLaunchTemplate"
            | "ec2:CreateLaunchTemplateVersion"
            | "ec2:CreateNatGateway"
            | "ec2:CreateRoute"
            | "ec2:CreateRouteTable"
            | "ec2:CreateSecurityGroup"
            | "ec2:CreateSubnet"
            | "ec2:CreateTags"
            | "ec2:CreateVolume"
            | "ec2:CreateVpc"
            | "ec2:DeleteInternetGateway"
            | "ec2:DeleteLaunchTemplate"
            | "ec2:DeleteNatGateway"
            | "ec2:DeleteRoute"
            | "ec2:DeleteRouteTable"
            | "ec2:DeleteSecurityGroup"
            | "ec2:DeleteSubnet"
            | "ec2:DeleteVolume"
            | "ec2:DeleteVpc"
            | "ec2:DescribeVpcAttribute"
            | "ec2:DetachInternetGateway"
            | "ec2:DisassociateRouteTable"
            | "ec2:GetConsoleOutput"
            | "ec2:ModifyVpcAttribute"
            | "ec2:ReleaseAddress"
            | "ec2:RevokeSecurityGroupEgress"
            | "ec2:RevokeSecurityGroupIngress"
            | "elasticloadbalancing:AddTags"
            | "elasticloadbalancing:CreateListener"
            | "elasticloadbalancing:CreateLoadBalancer"
            | "elasticloadbalancing:CreateTargetGroup"
            | "elasticloadbalancing:DeleteListener"
            | "elasticloadbalancing:DeleteLoadBalancer"
            | "elasticloadbalancing:DeleteTargetGroup"
            | "events:PutRule"
            | "lambda:CreateFunction"
    )
}
