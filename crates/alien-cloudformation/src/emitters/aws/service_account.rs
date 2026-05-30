//! AWS ServiceAccount — IAM role per permission profile.
//!
//! Trust policy is service-principal-based (the AWS service principals
//! consuming the role: `lambda.amazonaws.com`, `codebuild.amazonaws.com`,
//! `ec2.amazonaws.com`). Inline policy is generated from the stack's
//! permission sets through the `alien-permissions` IAM generator.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        cf_from_json, required_logical_id, resource_config, service_trust_policy, stack_name, tags,
        uniquify_iam_statement_sids, INLINE_POLICY_NAME,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, Build, ComputeCluster, ErrorData, Result, ServiceAccount, Worker,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{
    generators::AwsCloudFormationPermissionsGenerator, BindingTarget, PermissionContext,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsServiceAccountEmitter;

impl CfEmitter for AwsServiceAccountEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let service_account =
            resource_config::<ServiceAccount>(ctx, ServiceAccount::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = format!("{logical_id}Role");

        let mut role = CfResource::new(role_id.clone(), "AWS::IAM::Role".to_string());
        role.properties
            .insert("RoleName".to_string(), stack_name(&service_account.id));
        role.properties.insert(
            "AssumeRolePolicyDocument".to_string(),
            service_account_trust_policy(ctx, service_account),
        );
        // Note: we intentionally do NOT attach the legacy
        // `AWSLambdaBasicExecutionRole` / `AWSLambdaVPCAccessExecutionRole`
        // managed policies here. The runtime controller doesn't attach
        // them either — every permission grant flows through alien-
        // permissions (CloudWatch logs come from `worker/execute`,
        // VPC ENI access is the customer's call via a dedicated
        // permission set). Push and pull deployments must converge on
        // the same effective IAM, so the managed-policy attachment
        // would be a real drift, not a free safety net.

        let policy = service_account_policy_document(ctx, service_account)?;
        if let Some(policy) = policy {
            role.properties.insert(
                "Policies".to_string(),
                CfExpression::list([CfExpression::object([
                    ("PolicyName", CfExpression::from(INLINE_POLICY_NAME)),
                    ("PolicyDocument", policy),
                ])]),
            );
        }
        role.properties.insert("Tags".to_string(), tags(ctx));

        Ok(vec![role])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        resource_config::<ServiceAccount>(ctx, ServiceAccount::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = format!("{logical_id}Role");
        Ok(CfExpression::object([
            ("roleName", CfExpression::ref_(&role_id)),
            ("roleArn", CfExpression::get_att(&role_id, "Arn")),
            ("stackPermissionsApplied", CfExpression::from(true)),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        resource_config::<ServiceAccount>(ctx, ServiceAccount::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = format!("{logical_id}Role");
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("awsiam")),
            ("roleName", CfExpression::ref_(&role_id)),
            ("roleArn", CfExpression::get_att(&role_id, "Arn")),
        ])))
    }
}

fn service_account_trust_policy(
    ctx: &EmitContext<'_>,
    service_account: &ServiceAccount,
) -> CfExpression {
    let profile_name = service_account.id.strip_suffix("-sa");
    let mut services = BTreeSet::new();
    let mut compute_role_arns = Vec::new();

    for (id, entry) in ctx.stack.resources() {
        if let Some(function) = entry.config.downcast_ref::<Worker>() {
            if Some(function.permissions.as_str()) == profile_name {
                services.insert("lambda.amazonaws.com");
            }
        }
        if let Some(build) = entry.config.downcast_ref::<Build>() {
            if Some(build.permissions.as_str()) == profile_name {
                services.insert("codebuild.amazonaws.com");
            }
        }
        if entry.config.downcast_ref::<ComputeCluster>().is_some() {
            if let Some(logical_id) = ctx.name_for(id) {
                compute_role_arns.push(CfExpression::get_att(
                    format!("{logical_id}InstanceRole"),
                    "Arn",
                ));
            }
        }
    }

    if services.is_empty() && compute_role_arns.is_empty() {
        services.insert("lambda.amazonaws.com");
        services.insert("codebuild.amazonaws.com");
        services.insert("ec2.amazonaws.com");
    }

    if compute_role_arns.is_empty() {
        return service_trust_policy(services);
    }

    let mut statements = Vec::new();
    if !services.is_empty() {
        let service_principal = if services.len() == 1 {
            CfExpression::from(*services.iter().next().expect("one service"))
        } else {
            CfExpression::list(services.into_iter().map(CfExpression::from))
        };
        statements.push(CfExpression::object([
            ("Effect", CfExpression::from("Allow")),
            (
                "Principal",
                CfExpression::object([("Service", service_principal)]),
            ),
            ("Action", CfExpression::from("sts:AssumeRole")),
        ]));
    }
    statements.push(CfExpression::object([
        ("Effect", CfExpression::from("Allow")),
        (
            "Principal",
            CfExpression::object([("AWS", CfExpression::list(compute_role_arns))]),
        ),
        ("Action", CfExpression::from("sts:AssumeRole")),
    ]));

    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
    ])
}

fn service_account_policy_document(
    _ctx: &EmitContext<'_>,
    service_account: &ServiceAccount,
) -> Result<Option<CfExpression>> {
    let mut statements = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = permission_context().with_resource_name(service_account.id.clone());

    for permission_set in &service_account.stack_permission_sets {
        let policy = generator
            .generate_policy(permission_set, BindingTarget::Stack, &context)
            .context(ErrorData::GenericError {
                message: format!(
                    "failed to generate AWS CloudFormation policy for service account '{}'",
                    service_account.id
                ),
            })?;
        let policy_value = serde_json::to_value(policy).into_alien_error().context(
            ErrorData::TemplateSerializationFailed {
                format: "CloudFormation IAM policy".to_string(),
                reason: "Failed to serialize IAM policy".to_string(),
            },
        )?;
        let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
            return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                format: "CloudFormation IAM policy".to_string(),
                reason: "policy did not serialize to a JSON object".to_string(),
            }));
        };
        let Some(CfExpression::List(policy_statements)) = policy_object.shift_remove("Statement")
        else {
            continue;
        };
        statements.extend(policy_statements);
    }

    if statements.is_empty() {
        return Ok(None);
    }

    Ok(Some(CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list(uniquify_iam_statement_sids(statements)),
        ),
    ])))
}

pub(crate) fn permission_context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("")
        .with_aws_region("${AWS::Region}")
        .with_aws_account_id("${AWS::AccountId}")
        .with_managing_role_arn("${ManagingRoleArn}")
}
