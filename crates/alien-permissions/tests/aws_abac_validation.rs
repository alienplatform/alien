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
fn aws_tag_tamper_protection_denies_alien_tag_mutation() {
    let permission_set = get_permission_set("aws/tag-tamper-protection")
        .expect("aws/tag-tamper-protection permission set must exist");
    let aws_permissions = permission_set
        .platforms
        .aws
        .as_ref()
        .expect("tag tamper protection must have AWS permissions");

    assert!(!aws_permissions.is_empty());
    assert!(
        aws_permissions
            .iter()
            .all(|permission| permission.effect == AwsPermissionEffect::Deny),
        "tag tamper protection must only contain Deny statements"
    );

    let actions = aws_permissions
        .iter()
        .flat_map(|permission| {
            permission
                .grant
                .actions
                .as_ref()
                .expect("tag tamper protection must have actions")
        })
        .cloned()
        .collect::<Vec<_>>();
    for expected in [
        "acm:RemoveTagsFromCertificate",
        "autoscaling:DeleteTags",
        "dynamodb:UntagResource",
        "ec2:DeleteTags",
        "elasticloadbalancing:RemoveTags",
        "events:UntagResource",
        "lambda:UntagResource",
        "sqs:UntagQueue",
    ] {
        assert!(
            actions.contains(&expected.to_string()),
            "tag tamper protection is missing {expected}"
        );
    }

    for permission in aws_permissions {
        for binding in [
            permission.binding.stack.as_ref(),
            permission.binding.resource.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(binding.resources, vec!["*".to_string()]);
            assert!(has_condition_key(binding, "aws:TagKeys"));
        }
    }

    let s3_permission = aws_permissions
        .iter()
        .find(|permission| {
            permission
                .grant
                .actions
                .as_ref()
                .is_some_and(|actions| actions.contains(&"s3:PutBucketTagging".to_string()))
        })
        .expect("tag tamper protection must deny s3:PutBucketTagging");
    for binding in [
        s3_permission.binding.stack.as_ref(),
        s3_permission.binding.resource.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        assert!(has_condition_key(binding, "aws:ResourceTag/${stackTag}"));
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
            | "acm:DeleteCertificate"
            | "apigateway:POST"
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
            | "ec2:DeleteTags"
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
            | "lambda:CreateWorker"
    )
}
