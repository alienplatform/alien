//! AWS ServiceAccount — IAM role per permission profile.
//!
//! Trust policy is service-principal-based (the AWS service principals
//! consuming the role: `lambda.amazonaws.com`, `codebuild.amazonaws.com`,
//! `ec2.amazonaws.com`). Inline policy is generated from the stack's
//! permission sets through the `alien-permissions` IAM generator.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        cf_from_json, managing_role_trust_statement, required_logical_id, resource_config,
        service_trust_policy, stack_name, tags, uniquify_iam_statement_sids, INLINE_POLICY_NAME,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, Build, ComputeCluster, ErrorData, RemoteStackManagement, Result,
    ServiceAccount, Worker,
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
            let profile =
                alien_core::permission_profile_from_service_account_id(&service_account.id);
            // Generated launch links use a lowercase stack prefix of at most 35 characters.
            // Preserve an exact IRSA subject; CloudFormation cannot normalize dynamic names.
            if profile.len() > 24
                || alien_core::kubernetes_service_account_name("prefix", &profile)
                    != format!("prefix-{profile}-sa")
            {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate EKS CloudFormation workload identity".to_string(),
                    reason: match &service_account.extension {
                        // The identity's id is the extension's name and `-ext`.
                        Some(extension) => format!("extension '{extension}' needs a name of at most 20 characters for the generated EKS CloudFormation stack naming contract, which names the service account of its controller's pod after it"),
                        None => format!("permission profile '{profile}' must be a lowercase DNS label of at most 24 characters for the generated EKS CloudFormation stack naming contract"),
                    },
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
            service_account_trust_policy(ctx, service_account)?,
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
) -> Result<CfExpression> {
    if service_account.extension.is_some() {
        // On Kubernetes the extension's controller runs in a pod of its own, whose service
        // account is named after the identity, and no deployment there is pushed by the
        // manager, so the managing role is never trusted there.
        if let Some(statement) =
            super::kubernetes_cluster::eks_pod_trust_statement(ctx, &service_account.id)
        {
            return Ok(CfExpression::sub_with(
                r#"{"Version":"2012-10-17","Statement":[${PodTrust}]}"#,
                [("PodTrust", statement)],
            ));
        }
        if ctx.targets_kubernetes {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate an extension's identity for Kubernetes".to_string(),
                reason: format!(
                    "service account '{}' is an extension's identity, which the pod of its controller assumes through the cluster this stack creates, and the stack creates none",
                    service_account.id
                ),
            }));
        }
        return Ok(CfExpression::object([
            ("Version", CfExpression::from("2012-10-17")),
            (
                "Statement",
                CfExpression::list([managing_role_trust_statement()]),
            ),
        ]));
    }
    let profile_name = service_account.id.strip_suffix("-sa");
    if let Some(statement) = super::kubernetes_cluster::eks_pod_trust_statement(
        ctx,
        profile_name.unwrap_or(&service_account.id),
    ) {
        // Fn::Sub builds JSON so dynamic OIDC issuer names can be condition keys.
        return Ok(CfExpression::sub_with(
            r#"{"Version":"2012-10-17","Statement":[${PodTrust}]}"#,
            [("PodTrust", statement)],
        ));
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
        return Ok(service_trust_policy(services));
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

    Ok(CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
    ]))
}

fn service_account_policy_document(
    ctx: &EmitContext<'_>,
    service_account: &ServiceAccount,
) -> Result<Option<CfExpression>> {
    let mut statements = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = stack_permission_context(ctx).with_resource_name(service_account.id.clone());

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

/// Compile setup grants with the ceiling installed by this package's management emitter.
pub(crate) fn stack_permission_context(ctx: &EmitContext<'_>) -> PermissionContext {
    let mut context = permission_context();
    if ctx
        .stack
        .resources()
        .any(|(_, entry)| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
    {
        context.aws_permissions_boundary_arn = Some(
            "arn:aws:iam::${AWS::AccountId}:policy/${AWS::StackName}-deployment-management-boundary"
                .to_string(),
        );
    }
    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Platform, ResourceLifecycle, Stack, StackSettings};
    use indexmap::IndexMap;

    /// On Kubernetes the pod of an extension's controller assumes the extension's identity through
    /// the cluster the stack creates; with no cluster there, the identity is refused rather than
    /// given to the managing role, which pushes no deployment there.
    #[test]
    fn an_extension_identity_on_kubernetes_with_no_cluster_is_refused() {
        let account = ServiceAccount::new("acme-ext".to_string())
            .extension("acme".to_string())
            .build();
        let stack = Stack::new("app".to_string())
            .add(account, ResourceLifecycle::Frozen)
            .build();
        let (_, entry) = stack.resources().next().unwrap();
        let names = IndexMap::from([("acme-ext".to_string(), "AcmeExt".to_string())]);
        let settings = StackSettings::default();
        let ctx = |targets_kubernetes| EmitContext {
            stack: &stack,
            resource: entry,
            resource_id: "acme-ext",
            platform: Platform::Aws,
            targets_kubernetes,
            stack_settings: &settings,
            names: &names,
        };

        let error = AwsServiceAccountEmitter
            .emit_resources(&ctx(true))
            .unwrap_err();
        assert!(error.message.contains("the stack creates none"), "{error}");
        AwsServiceAccountEmitter
            .emit_resources(&ctx(false))
            .expect("on AWS the managing role assumes it");
    }
}
