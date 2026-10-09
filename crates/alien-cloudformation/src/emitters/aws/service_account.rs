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
        if ctx.targets_kubernetes {
            let profile = service_account
                .id
                .strip_suffix("-sa")
                .unwrap_or(&service_account.id);
            // Generated launch links use a lowercase stack prefix of at most 35 characters.
            // Preserve an exact IRSA subject; CloudFormation cannot normalize dynamic names.
            if profile.len() > 24
                || alien_core::kubernetes_service_account_name("prefix", profile)
                    != format!("prefix-{profile}-sa")
            {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate EKS CloudFormation workload identity".to_string(),
                    reason: format!("permission profile '{profile}' must be a lowercase DNS label of at most 24 characters for the generated EKS CloudFormation stack naming contract"),
                }));
            }
        }
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
    if let Some(statement) = super::kubernetes_cluster::eks_pod_trust_statement(
        ctx,
        profile_name.unwrap_or(&service_account.id),
    ) {
        // Fn::Sub builds JSON so dynamic OIDC issuer names can be condition keys.
        return CfExpression::sub_with(
            r#"{"Version":"2012-10-17","Statement":[${PodTrust}]}"#,
            [("PodTrust", statement)],
        );
    }
    let mut services = BTreeSet::new();
    let mut role_arns = Vec::new();

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
                role_arns.push(CfExpression::get_att(
                    format!("{logical_id}InstanceRole"),
                    "Arn",
                ));
                // An exact ARN condition can retain both node generations without
                // resolving a not-yet-created role or adding a dependency cycle.
                role_arns.push(CfExpression::sub(format!(
                    "arn:${{AWS::Partition}}:iam::${{AWS::AccountId}}:role/${{AWS::StackName}}-{id}-isolation-v1"
                )));
            }
        }
    }

    // Explicit impersonation grants require trust as well as an IAM action.
    // Use exact role ARN conditions to avoid role creation dependency cycles.
    for impersonator_id in service_account.impersonators(ctx.stack) {
        role_arns.push(CfExpression::sub(format!(
            "arn:${{AWS::Partition}}:iam::${{AWS::AccountId}}:role/${{AWS::StackName}}-{impersonator_id}"
        )));
    }

    if services.is_empty() && role_arns.is_empty() {
        services.insert("lambda.amazonaws.com");
        services.insert("codebuild.amazonaws.com");
        services.insert("ec2.amazonaws.com");
    }

    if role_arns.is_empty() {
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
            CfExpression::object([(
                "AWS",
                CfExpression::sub("arn:${AWS::Partition}:iam::${AWS::AccountId}:root"),
            )]),
        ),
        ("Action", CfExpression::from("sts:AssumeRole")),
        (
            "Condition",
            CfExpression::object([(
                "ArnEquals",
                CfExpression::object([("aws:PrincipalArn", CfExpression::list(role_arns))]),
            )]),
        ),
    ]));

    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
    ])
}

fn service_account_policy_document(
    ctx: &EmitContext<'_>,
    service_account: &ServiceAccount,
) -> Result<Option<CfExpression>> {
    let mut statements = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = permission_context().with_resource_name(service_account.id.clone());

    let mut grants: Vec<_> = service_account
        .stack_permission_sets
        .iter()
        .map(|set| (set, BindingTarget::Stack, context.clone()))
        .collect();
    for (target, sets) in &service_account.resource_permission_sets {
        let Some(target_id) = ServiceAccount::impersonation_target(ctx.stack, target) else {
            continue;
        };
        for set in sets
            .iter()
            .filter(|set| set.id == "service-account/impersonate")
        {
            grants.push((
                set,
                BindingTarget::Resource,
                context.clone().with_resource_name(target_id.clone()),
            ));
        }
    }
    for (permission_set, target, context) in grants {
        let policy = generator
            .generate_policy(permission_set, target, &context)
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
