//! The setup policy has to cover what the CloudFormation and Terraform setups call on AWS, not
//! only what the runtime controllers call. Each request below was denied to a role holding exactly
//! the rendered setup policy while it created, updated (cluster parameter change) or deleted a
//! Postgres + storage setup stack. The second half checks that a stack whose name is a dash-prefix
//! of another stack's cannot reach that stack's database objects.
mod common;
mod iam_policy;

use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set,
    initial_setup::generate_aws_initial_setup_policy,
    BindingTarget,
};
use iam_policy::{evaluate, Decision, Request};

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
    for id in ["postgres/provision", "postgres/management"] {
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
