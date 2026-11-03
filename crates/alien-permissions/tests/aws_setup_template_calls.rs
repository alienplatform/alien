//! The setup policy has to cover what the CloudFormation and Terraform setups call on AWS, not
//! only what the runtime controllers call. Each request below was denied to a role holding exactly
//! the rendered setup policy while it created, updated (cluster parameter change) or deleted a
//! Postgres + storage setup stack. The second half checks that a stack whose name is a dash-prefix
//! of another stack's cannot reach that stack's database objects.
mod common;
mod iam_policy;

use alien_permissions::{
    generators::{
        AwsCloudFormationPermissionsGenerator, AwsIamStatement, AwsRuntimePermissionsGenerator,
    },
    get_permission_set,
    initial_setup::generate_aws_initial_setup_policy,
    BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};
use serde_json::Value;

const RDS: &str = "arn:aws:rds:us-east-1:123456789012";
const IAM: &str = "arn:aws:iam::123456789012";
const EC2: &str = "arn:aws:ec2:us-east-1:123456789012";
const SECRETS: &str = "arn:aws:secretsmanager:us-east-1:123456789012:secret";
const KMS_KEY: &str = "arn:aws:kms:us-east-1:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab";

/// Tags the setup templates put on what they create.
const SETUP_TAGS: &[(&str, &str)] = &[
    ("deployment", "my-stack"),
    ("managed-by", "setup"),
    ("resource", "db"),
];
const OTHER_STACK_TAGS: &[(&str, &str)] = &[
    ("deployment", "my-stack-prod"),
    ("managed-by", "setup"),
    ("resource", "db"),
];

fn setup_statements() -> Vec<AwsIamStatement> {
    generate_aws_initial_setup_policy(&common::create_test_context())
        .unwrap()
        .statement
}

fn template_setup_calls() -> Vec<Request> {
    let cfn_management_policy =
        format!("{IAM}:policy/my-stack-ManagementRoleManagementPolicy1-gTTeXRckRPqq");
    let cfn_role_policy =
        format!("{IAM}:policy/my-stack-ExecutionSaRoleManagedPermissions1-1nkjESb3B8Pa");
    let tf_role_policy = format!("{IAM}:policy/my-stack-execution-sa-setup-0d036fcd");
    let tf_management_policy = format!("{IAM}:policy/my-stack-deployment-management-2");
    let cluster_pg = format!("{RDS}:cluster-pg:my-stack-dbclusterparametergroup-ca6eanueb6cu");
    let managed_secret =
        format!("{SECRETS}:rds!cluster-5d2e7185-b053-4ac5-a304-1bcf996efbcb-StovOD");
    let cluster_arn = format!("{RDS}:cluster:my-stack-db");
    let managed_secret_tags = [
        SETUP_TAGS,
        &[("aws:rds:primaryDBClusterArn", cluster_arn.as_str())],
    ]
    .concat();

    let mut calls = Vec::new();
    // Managed policies CloudFormation names after the stack, and the Terraform setup policies.
    for policy in [&cfn_management_policy, &cfn_role_policy, &tf_role_policy] {
        for action in [
            "iam:CreatePolicy",
            "iam:GetPolicy",
            "iam:GetPolicyVersion",
            "iam:ListPolicyVersions",
            "iam:ListEntitiesForPolicy",
            "iam:DeletePolicy",
        ] {
            calls.push(Request::new(action, policy.clone()));
        }
    }
    for action in ["iam:GetPolicy", "iam:GetPolicyVersion"] {
        calls.push(Request::new(action, tf_management_policy.clone()));
    }
    for policy in [&cfn_role_policy, &tf_role_policy] {
        calls.push(
            Request::new(
                "iam:AttachRolePolicy",
                format!("{IAM}:role/my-stack-execution-sa"),
            )
            .resource_tags(SETUP_TAGS)
            .with("iam:PolicyARN", policy.clone()),
        );
    }
    // Terraform reads a role's instance profiles before deleting it.
    calls.push(
        Request::new(
            "iam:ListInstanceProfilesForRole",
            format!("{IAM}:role/my-stack-execution-sa"),
        )
        .resource_tags(SETUP_TAGS),
    );
    // The CloudFormation master-password secret.
    calls.push(Request::new("secretsmanager:GetRandomPassword", "*"));
    calls.push(
        Request::new(
            "secretsmanager:GetSecretValue",
            format!("{SECRETS}:my-stack-db-credentials-KFPp24"),
        )
        .resource_tags(SETUP_TAGS),
    );
    // The CloudFormation connection-password secret: created with the stack's tags, then
    // overwritten or deleted.
    let connection_secret = format!("{SECRETS}:my-stack-db-credentials-KFPp24");
    for action in ["secretsmanager:CreateSecret", "secretsmanager:TagResource"] {
        calls.push(Request::new(action, connection_secret.clone()).request_tags(SETUP_TAGS));
    }
    for action in [
        "secretsmanager:PutSecretValue",
        "secretsmanager:DeleteSecret",
    ] {
        calls.push(Request::new(action, connection_secret.clone()).resource_tags(SETUP_TAGS));
    }
    // The database security group, tagged `managed-by=setup` by both templates.
    calls.push(
        Request::new(
            "ec2:CreateTags",
            format!("{EC2}:security-group/sg-0123456789abcdef0"),
        )
        .request_tags(SETUP_TAGS)
        .with("ec2:CreateAction", "CreateSecurityGroup"),
    );
    for action in ["ec2:DeleteSecurityGroup", "ec2:RevokeSecurityGroupEgress"] {
        calls.push(
            Request::new(action, format!("{EC2}:security-group/sg-0123456789abcdef0"))
                .resource_tags(SETUP_TAGS),
        );
    }
    // Bucket configuration the templates set or the Terraform provider reads on refresh.
    for action in [
        "s3:PutBucketOwnershipControls",
        "s3:GetBucketOwnershipControls",
        "s3:GetBucketCORS",
        "s3:GetBucketWebsite",
        "s3:GetAccelerateConfiguration",
        "s3:GetBucketRequestPayment",
        "s3:GetBucketLogging",
        "s3:GetReplicationConfiguration",
        "s3:GetBucketObjectLockConfiguration",
        "s3:ListTagsForResource",
    ] {
        calls.push(Request::new(action, "arn:aws:s3:::my-stack-files-e0770440"));
    }
    // Aurora: creates carry the tags; the parameter change reboots the writer.
    for (action, resource) in [
        ("rds:CreateDBCluster", format!("{RDS}:cluster:my-stack-db")),
        (
            "rds:CreateDBSubnetGroup",
            format!("{RDS}:subgrp:my-stack-db-subnets"),
        ),
        ("rds:CreateDBClusterParameterGroup", cluster_pg.clone()),
        ("rds:CreateDBInstance", format!("{RDS}:db:my-stack-db-1")),
        ("rds:AddTagsToResource", format!("{RDS}:db:my-stack-db-1")),
    ] {
        calls.push(Request::new(action, resource).request_tags(SETUP_TAGS));
    }
    for (action, resource) in [
        ("rds:DescribeDBClusterParameters", cluster_pg.clone()),
        ("rds:ModifyDBClusterParameterGroup", cluster_pg.clone()),
        ("rds:ResetDBClusterParameterGroup", cluster_pg.clone()),
        ("rds:ModifyDBCluster", format!("{RDS}:cluster:my-stack-db")),
        ("rds:RebootDBInstance", format!("{RDS}:db:my-stack-db-1")),
        ("rds:DeleteDBInstance", format!("{RDS}:db:my-stack-db-1")),
        ("rds:DeleteDBCluster", format!("{RDS}:cluster:my-stack-db")),
    ] {
        calls.push(Request::new(action, resource).resource_tags(SETUP_TAGS));
    }
    // CloudFormation reads and sets a new parameter group's parameters, and creates the cluster
    // in the new groups, before RDS sees their create-time tags.
    for (action, resource) in [
        ("rds:DescribeDBClusterParameters", cluster_pg.clone()),
        ("rds:ModifyDBClusterParameterGroup", cluster_pg.clone()),
        ("rds:DeleteDBClusterParameterGroup", cluster_pg.clone()),
        ("rds:CreateDBCluster", cluster_pg.clone()),
        (
            "rds:CreateDBCluster",
            format!("{RDS}:subgrp:my-stack-db-subnets"),
        ),
    ] {
        calls.push(Request::new(action, resource));
    }
    calls.push(Request::new(
        "rds:DescribeGlobalClusters",
        "arn:aws:rds::123456789012:global-cluster:*",
    ));
    // Terraform has RDS manage the master password.
    for action in ["secretsmanager:CreateSecret", "secretsmanager:TagResource"] {
        calls.push(Request::new(action, managed_secret.clone()).request_tags(&managed_secret_tags));
    }
    for action in [
        "secretsmanager:DescribeSecret",
        "secretsmanager:RotateSecret",
        "secretsmanager:CancelRotateSecret",
    ] {
        calls
            .push(Request::new(action, managed_secret.clone()).resource_tags(&managed_secret_tags));
    }
    for (action, alias) in [
        ("kms:DescribeKey", "alias/aws/rds"),
        ("kms:CreateGrant", "alias/aws/rds"),
        ("kms:DescribeKey", "alias/aws/secretsmanager"),
    ] {
        let mut call = Request::new(action, KMS_KEY)
            .with("kms:ViaService", "rds.us-east-1.amazonaws.com")
            .with_values("kms:ResourceAliases", &[alias]);
        if action == "kms:CreateGrant" {
            call = call.with("kms:GrantIsForAWSResource", "true");
        }
        calls.push(call);
    }
    calls
}

#[test]
fn setup_policy_allows_every_call_the_template_setups_make() {
    let statements = setup_statements();
    let denied: Vec<_> = template_setup_calls()
        .into_iter()
        .filter(|call| evaluate(&statements, call) != Decision::Allowed)
        .map(|call| format!("{} on {}", call.action, call.resource))
        .collect();
    assert!(
        denied.is_empty(),
        "the setup policy denies:\n{}",
        denied.join("\n")
    );
}

/// Calls on objects of stack `my-stack-prod`, whose names also start with `my-stack-`.
fn calls_on_another_stack() -> Vec<Request> {
    let cluster = format!("{RDS}:cluster:my-stack-prod-db");
    let instance = format!("{RDS}:db:my-stack-prod-db-1");
    let cluster_pg = format!("{RDS}:cluster-pg:my-stack-prod-db-pg");
    let secret = format!("{SECRETS}:rds!cluster-0f0e0d0c-0b0a-0908-0706-050403020100-AbCdEf");
    let mut calls = Vec::new();
    for (action, resource) in [
        ("rds:ModifyDBClusterParameterGroup", &cluster_pg),
        ("rds:ResetDBClusterParameterGroup", &cluster_pg),
        ("rds:DescribeDBClusterParameters", &cluster_pg),
        ("rds:ModifyDBCluster", &cluster),
        ("rds:DeleteDBCluster", &cluster),
        ("rds:ModifyDBInstance", &instance),
        ("rds:RebootDBInstance", &instance),
        ("rds:DeleteDBInstance", &instance),
        ("rds:RemoveTagsFromResource", &cluster),
    ] {
        calls.push(Request::new(action, resource.clone()).resource_tags(OTHER_STACK_TAGS));
    }
    // Claiming the other stack's object by retagging it.
    for resource in [&cluster, &instance, &cluster_pg] {
        calls.push(
            Request::new("rds:AddTagsToResource", resource.clone())
                .resource_tags(OTHER_STACK_TAGS)
                .request_tags(&[("deployment", "my-stack")]),
        );
    }
    // Creating a cluster or instance inside the other stack's subnet group, parameter group or
    // cluster, with this stack's tags on the request.
    for resource in [
        format!("{RDS}:subgrp:my-stack-prod-db-subnets"),
        cluster_pg.clone(),
    ] {
        calls.push(
            Request::new("rds:CreateDBCluster", resource)
                .resource_tags(OTHER_STACK_TAGS)
                .request_tags(SETUP_TAGS),
        );
    }
    calls.push(
        Request::new("rds:CreateDBInstance", cluster.clone())
            .resource_tags(OTHER_STACK_TAGS)
            .request_tags(SETUP_TAGS),
    );
    // Handing an own object to another stack, or releasing it.
    calls.push(
        Request::new(
            "rds:AddTagsToResource",
            format!("{RDS}:cluster:my-stack-db"),
        )
        .resource_tags(SETUP_TAGS)
        .request_tags(&[("deployment", "my-stack-prod")]),
    );
    calls.push(
        Request::new(
            "rds:RemoveTagsFromResource",
            format!("{RDS}:cluster:my-stack-db"),
        )
        .resource_tags(SETUP_TAGS)
        .with_values("aws:TagKeys", &["deployment"]),
    );
    // The other stack's secrets and roles.
    let other_secret_tags = [
        OTHER_STACK_TAGS,
        &[("aws:rds:primaryDBClusterArn", cluster.as_str())],
    ]
    .concat();
    for action in [
        "secretsmanager:RotateSecret",
        "secretsmanager:CancelRotateSecret",
        "secretsmanager:DescribeSecret",
    ] {
        calls.push(Request::new(action, secret.clone()).resource_tags(&other_secret_tags));
    }
    calls.push(
        Request::new("secretsmanager:TagResource", secret.clone())
            .resource_tags(&other_secret_tags)
            .request_tags(&[("deployment", "my-stack")]),
    );
    // The secret of a cluster the account owns itself, outside any stack.
    let account_cluster = format!("{RDS}:cluster:orders");
    calls.push(
        Request::new("secretsmanager:TagResource", secret.clone())
            .resource_tags(&[("aws:rds:primaryDBClusterArn", account_cluster.as_str())])
            .request_tags(&[("deployment", "my-stack")]),
    );
    // Creating, overwriting or deleting the other stack's password secret.
    let other_connection_secret = format!("{SECRETS}:my-stack-prod-db-credentials-AbCdEf");
    calls.push(
        Request::new(
            "secretsmanager:CreateSecret",
            other_connection_secret.clone(),
        )
        .request_tags(OTHER_STACK_TAGS),
    );
    for action in [
        "secretsmanager:PutSecretValue",
        "secretsmanager:DeleteSecret",
    ] {
        calls.push(
            Request::new(action, other_connection_secret.clone()).resource_tags(OTHER_STACK_TAGS),
        );
    }
    // Retagging the other stack's password secret, then reading it.
    calls.push(
        Request::new(
            "secretsmanager:TagResource",
            format!("{SECRETS}:my-stack-prod-db-credentials-AbCdEf"),
        )
        .resource_tags(OTHER_STACK_TAGS)
        .request_tags(&[("deployment", "my-stack")]),
    );
    calls.push(
        Request::new(
            "secretsmanager:GetSecretValue",
            format!("{SECRETS}:my-stack-prod-db-credentials-AbCdEf"),
        )
        .resource_tags(OTHER_STACK_TAGS),
    );
    calls.push(
        Request::new(
            "iam:AttachRolePolicy",
            format!("{IAM}:role/my-stack-prod-execution-sa"),
        )
        .resource_tags(OTHER_STACK_TAGS)
        .with(
            "iam:PolicyARN",
            format!("{IAM}:policy/my-stack-prod-execution-sa-setup-0d036fcd"),
        ),
    );
    // A key that is not one of the AWS managed database keys, and a grant to a principal.
    calls.push(
        Request::new("kms:CreateGrant", KMS_KEY)
            .with("kms:ViaService", "rds.us-east-1.amazonaws.com")
            .with("kms:GrantIsForAWSResource", "true")
            .with_values("kms:ResourceAliases", &["alias/orders"]),
    );
    calls.push(
        Request::new("kms:CreateGrant", KMS_KEY)
            .with("kms:ViaService", "rds.us-east-1.amazonaws.com")
            .with("kms:GrantIsForAWSResource", "false")
            .with_values("kms:ResourceAliases", &["alias/aws/rds"]),
    );
    calls
}

#[test]
fn a_stack_cannot_reach_another_stack_database_or_account_keys() {
    let context = common::create_test_context();
    let mut policies = vec![("setup", setup_statements())];
    for id in [
        "postgres/provision",
        "postgres/management",
        "postgres/data-access",
    ] {
        policies.push((
            id,
            AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    get_permission_set(id).unwrap(),
                    BindingTarget::Stack,
                    &context,
                )
                .unwrap()
                .statement,
        ));
    }
    for (name, statements) in policies {
        let allowed: Vec<_> = calls_on_another_stack()
            .into_iter()
            .filter(|call| evaluate(&statements, call) == Decision::Allowed)
            .map(|call| format!("{} on {}", call.action, call.resource))
            .collect();
        assert!(allowed.is_empty(), "{name} allows:\n{}", allowed.join("\n"));
    }
}

/// A workload reads its own stack's connection secret, which every create path tags.
#[test]
fn a_workload_reads_only_its_own_stack_connection_secret() {
    let statements = AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("postgres/data-access").unwrap(),
            BindingTarget::Stack,
            &common::create_test_context(),
        )
        .unwrap()
        .statement;
    let read = |tags: &[(&str, &str)]| {
        evaluate(
            &statements,
            &Request::new(
                "secretsmanager:GetSecretValue",
                format!("{SECRETS}:my-stack-db-credentials-KFPp24"),
            )
            .resource_tags(tags),
        )
    };
    assert_eq!(read(SETUP_TAGS), Decision::Allowed);
    assert_ne!(read(OTHER_STACK_TAGS), Decision::Allowed);
    // No create path has ever made the secret without the stack tag.
    assert_ne!(read(&[]), Decision::Allowed);
}

/// Two role writes the setup policy still allows on a dash-prefixed stack's roles and policies.
/// Both reach no further than `iam:PutRolePolicy` on `<prefix>-*` roles, which the setup policy
/// holds without a tag or boundary condition (the setup context strips the boundary condition),
/// as it did before.
#[test]
fn setup_role_writes_on_a_dash_prefixed_stack_stay_as_wide_as_put_role_policy() {
    let statements = setup_statements();
    let role = format!("{IAM}:role/my-stack-prod-execution-sa");
    let allowed = |call: Request| evaluate(&statements, &call) == Decision::Allowed;

    assert!(allowed(
        Request::new("iam:PutRolePolicy", role.clone()).resource_tags(OTHER_STACK_TAGS)
    ));
    // Tag the other stack's role for this stack, then attach a policy to it.
    assert!(allowed(
        Request::new("iam:TagRole", role.clone())
            .resource_tags(OTHER_STACK_TAGS)
            .request_tags(&[("deployment", "my-stack")])
    ));
    assert!(allowed(
        Request::new("iam:AttachRolePolicy", role)
            .resource_tags(SETUP_TAGS)
            .with(
                "iam:PolicyARN",
                format!("{IAM}:policy/my-stack-prod-execution-sa-setup-0d036fcd"),
            )
    ));
    // A new default version of the other stack's setup-managed role policy.
    assert!(allowed(Request::new(
        "iam:CreatePolicyVersion",
        format!("{IAM}:policy/my-stack-prod-execution-sa-setup-0d036fcd"),
    )));
}

/// Tag sets for the stack's secret: complete, and with each stack tag missing or foreign.
fn secret_tag_cases() -> Vec<(&'static str, Vec<(&'static str, &'static str)>, Decision)> {
    vec![
        (
            "both tags",
            vec![("deployment", "my-stack"), ("resource", "db")],
            Decision::Allowed,
        ),
        (
            "no deployment tag",
            vec![("resource", "db")],
            Decision::NotAllowed,
        ),
        (
            "no resource tag",
            vec![("deployment", "my-stack")],
            Decision::NotAllowed,
        ),
        (
            "another stack's tag",
            vec![("deployment", "my-stack-prod"), ("resource", "db")],
            Decision::NotAllowed,
        ),
        ("no tags", vec![], Decision::NotAllowed),
    ]
}

/// The Terraform setup grants a workload `postgres/data-access` with the resource binding and
/// only swaps the resource for the cluster's exact RDS-managed secret ARN, keeping the
/// conditions. RDS copies the cluster's tags onto that secret, so the read works only while the
/// secret carries both stack tags.
#[test]
fn a_terraform_workload_reads_the_managed_secret_only_with_both_stack_tags() {
    let managed_secret =
        format!("{SECRETS}:rds!cluster-5d2e7185-b053-4ac5-a304-1bcf996efbcb-StovOD");
    let mut set = get_permission_set("postgres/data-access").unwrap().clone();
    for entry in set.platforms.aws.iter_mut().flatten() {
        entry.binding.resource.as_mut().unwrap().resources = vec!["${resourceId}".to_string()];
    }
    let context = PermissionContext::new()
        .with_stack_prefix("my-stack")
        .with_resource_name("db")
        .with_resource_id(&managed_secret)
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1");
    let statements = AwsRuntimePermissionsGenerator::new()
        .generate_policy(&set, BindingTarget::Resource, &context)
        .unwrap()
        .statement;
    for (case, tags, expected) in secret_tag_cases() {
        let call = Request::new("secretsmanager:GetSecretValue", managed_secret.clone())
            .resource_tags(&tags);
        assert_eq!(evaluate(&statements, &call), expected, "{case}");
    }
}

/// The setup identity's rotation calls on the RDS-managed secret need the stack tag.
#[test]
fn setup_rotation_calls_on_the_managed_secret_need_the_stack_tag() {
    let statements = setup_statements();
    let managed_secret =
        format!("{SECRETS}:rds!cluster-5d2e7185-b053-4ac5-a304-1bcf996efbcb-StovOD");
    for action in [
        "secretsmanager:DescribeSecret",
        "secretsmanager:RotateSecret",
        "secretsmanager:CancelRotateSecret",
    ] {
        for (tags, expected) in [
            (
                vec![("deployment", "my-stack"), ("resource", "db")],
                Decision::Allowed,
            ),
            (vec![("resource", "db")], Decision::NotAllowed),
            (vec![("deployment", "my-stack-prod")], Decision::NotAllowed),
        ] {
            let call = Request::new(action, managed_secret.clone()).resource_tags(&tags);
            assert_eq!(
                evaluate(&statements, &call),
                expected,
                "{action} with {tags:?}"
            );
        }
    }
}

/// Resolves the `Fn::Sub` pseudo parameters a CloudFormation-rendered policy uses.
fn resolve_cloudformation(value: Value) -> Value {
    match value {
        Value::Object(map) if map.len() == 1 && map.contains_key("Fn::Sub") => Value::String(
            map["Fn::Sub"]
                .as_str()
                .unwrap()
                .replace("${AWS::Partition}", "aws")
                .replace("${AWS::Region}", "us-east-1")
                .replace("${AWS::AccountId}", "123456789012")
                .replace("${AWS::StackName}", "my-stack"),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, resolve_cloudformation(value)))
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.into_iter().map(resolve_cloudformation).collect())
        }
        other => other,
    }
}

/// The CloudFormation setup renders the workload's read as the emitter does (stack-name prefix,
/// resource binding) for the setup-created `<stack>-<id>-credentials` secret.
#[test]
fn a_cloudformation_workload_reads_the_connection_secret_only_with_both_stack_tags() {
    let context = PermissionContext::new()
        .with_stack_prefix("")
        .with_resource_name("db")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1");
    let policy = AwsCloudFormationPermissionsGenerator::new()
        .generate_policy(
            get_permission_set("postgres/data-access").unwrap(),
            BindingTarget::Resource,
            &context,
        )
        .unwrap();
    let rendered = resolve_cloudformation(serde_json::to_value(&policy).unwrap());
    let statements: Vec<AwsIamStatement> =
        serde_json::from_value(rendered["Statement"].clone()).unwrap();
    for (case, tags, expected) in secret_tag_cases() {
        let call = Request::new(
            "secretsmanager:GetSecretValue",
            format!("{SECRETS}:my-stack-db-credentials-KFPp24"),
        )
        .resource_tags(&tags);
        assert_eq!(evaluate(&statements, &call), expected, "{case}");
    }
}

/// IAM authorizes `rds:ModifyDBCluster` against every resource the request names. CloudFormation's
/// cluster update names the cluster and its parameter group (`DBClusterParameterGroupName`, from
/// CloudTrail `requestParameters` of a live stack update); Terraform and the controller name only
/// the cluster. Replays both against the setup policy and the runtime provision set.
#[test]
fn a_cloudformation_cluster_update_is_authorized_on_the_cluster_and_its_parameter_group() {
    let context = common::create_test_context();
    let provision = AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            get_permission_set("postgres/provision").unwrap(),
            BindingTarget::Stack,
            &context,
        )
        .unwrap()
        .statement;
    // {"dBClusterIdentifier": "my-stack-db", "dBClusterParameterGroupName":
    //  "my-stack-dbclusterparametergroup-ca6eanueb6cu", "applyImmediately": true, ...}
    let request = [
        format!("{RDS}:cluster:my-stack-db"),
        format!("{RDS}:cluster-pg:my-stack-dbclusterparametergroup-ca6eanueb6cu"),
    ];
    for (name, statements) in [("setup", setup_statements()), ("provision", provision)] {
        for resource in &request {
            let call =
                Request::new("rds:ModifyDBCluster", resource.clone()).resource_tags(SETUP_TAGS);
            assert_eq!(
                evaluate(&statements, &call),
                Decision::Allowed,
                "{name}: {resource}"
            );
        }
    }
}

/// The parameter-group leg of ModifyDBCluster needs this stack's tag. ModifyDBCluster never
/// authorizes against a subnet group, and neither path sends an option group, so neither is
/// granted.
#[test]
fn modify_cluster_reaches_only_this_stack_tagged_parameter_group() {
    let statements = setup_statements();
    let modify = |resource: String, tags: &[(&str, &str)]| {
        evaluate(
            &statements,
            &Request::new("rds:ModifyDBCluster", resource).resource_tags(tags),
        )
    };
    let own_pg = format!("{RDS}:cluster-pg:my-stack-dbclusterparametergroup-ca6eanueb6cu");
    assert_eq!(modify(own_pg.clone(), SETUP_TAGS), Decision::Allowed);
    assert_ne!(
        modify(own_pg, &[]),
        Decision::Allowed,
        "untagged under the prefix"
    );
    assert_ne!(
        modify(
            format!("{RDS}:cluster-pg:my-stack-prod-db-pg"),
            OTHER_STACK_TAGS
        ),
        Decision::Allowed,
        "another stack's group"
    );
    for (kind, name) in [
        ("subgrp", "my-stack-db-subnets"),
        ("og", "my-stack-db-options"),
    ] {
        for tags in [SETUP_TAGS, OTHER_STACK_TAGS, &[]] {
            assert_ne!(
                modify(format!("{RDS}:{kind}:{name}"), tags),
                Decision::Allowed,
                "{kind} with {tags:?}"
            );
        }
    }
}
