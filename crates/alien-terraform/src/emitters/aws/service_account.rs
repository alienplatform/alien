//! AWS ServiceAccount — IAM role per permission profile.
//!
//! Trust policy is service-principal-based (the AWS service principals
//! consuming the role: `lambda.amazonaws.com`, `codebuild.amazonaws.com`,
//! `ec2.amazonaws.com`). Inline policies come straight from
//! `AwsRuntimePermissionsGenerator` so push and pull deployments
//! converge on the same effective IAM (no extra managed-policy
//! attachments — every grant flows through alien-permissions).

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy,
        emit_iam_role_policy_for_target_with_label, iam_role_name_template, jsonencode,
        required_label, service_assume_role_policy, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, permissions::PermissionSetReference, Build, ComputeCluster, Result,
    ServiceAccount, Worker,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsServiceAccountEmitter;

impl TfEmitter for AwsServiceAccountEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let service_account = downcast::<ServiceAccount>(ctx, ServiceAccount::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let TrustPrincipals {
            services,
            role_arns,
        } = trust_principals(ctx, service_account);
        let services_ref: Vec<&str> = services.iter().copied().collect();

        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            label,
            [
                attr("name", iam_role_name_template(&service_account.id)),
                attr(
                    "assume_role_policy",
                    trust_assume_role_policy(&services_ref, role_arns),
                ),
                attr("tags", tags(ctx, "service-account")),
            ],
        ));

        let context =
            aws_terraform_permission_context().with_resource_name(service_account.id.clone());
        for (index, permission_set) in service_account.stack_permission_sets.iter().enumerate() {
            emit_iam_role_policy(&mut fragment, label, permission_set, index, &context)?;
        }

        for (target, sets) in &service_account.resource_permission_sets {
            let target_id = if ctx.stack.resources().any(|(id, entry)| {
                id == target && entry.config.downcast_ref::<ServiceAccount>().is_some()
            }) {
                target.clone()
            } else {
                format!("{target}-sa")
            };
            if !ctx.stack.resources().any(|(id, entry)| {
                id == &target_id && entry.config.downcast_ref::<ServiceAccount>().is_some()
            }) {
                continue;
            }
            let context = aws_terraform_permission_context().with_resource_name(target_id.clone());
            for (index, set) in sets
                .iter()
                .enumerate()
                .filter(|(_, set)| set.id == "service-account/impersonate")
            {
                emit_iam_role_policy_for_target_with_label(
                    &mut fragment,
                    label,
                    set,
                    &format!("{label}_{target_id}_impersonate_{index}"),
                    &format!("impersonate-{target_id}-{index}"),
                    &context,
                    BindingTarget::Resource,
                )?;
            }
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("roleName", expr::traversal(["aws_iam_role", label, "name"])),
            ("roleArn", expr::traversal(["aws_iam_role", label, "arn"])),
            ("stackPermissionsApplied", Expression::Bool(true)),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("awsiam".to_string())),
            ("roleName", expr::traversal(["aws_iam_role", label, "name"])),
            ("roleArn", expr::traversal(["aws_iam_role", label, "arn"])),
        ])))
    }
}

struct TrustPrincipals {
    services: BTreeSet<&'static str>,
    role_arns: Vec<Expression>,
}

fn trust_principals(ctx: &EmitContext<'_>, service_account: &ServiceAccount) -> TrustPrincipals {
    let profile_name = service_account.id.strip_suffix("-sa");
    let mut services: BTreeSet<&'static str> = BTreeSet::new();
    let mut role_arns = Vec::new();

    for (_id, entry) in ctx.stack.resources() {
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
        if let Some(cluster) = entry.config.downcast_ref::<ComputeCluster>() {
            // Build the ARN from the deterministic role name instead of
            // traversing the compute-cluster IAM resource. The compute role's
            // execute policy may reference this service-account role; a direct
            // traversal here would therefore create a Terraform dependency
            // cycle between the two roles.
            for suffix in ["instances", "isolation-v1"] {
                role_arns.push(expr::template(format!(
                    "arn:aws:iam::${{data.aws_caller_identity.current.account_id}}:role/${{local.resource_prefix}}-{}-{suffix}",
                    cluster.id
                )));
            }
        }
    }

    // Explicit impersonation grants require trust as well as an IAM action.
    // Use exact role ARN conditions to avoid role creation dependency cycles.
    for (profile, permissions) in &ctx.stack.permissions.profiles {
        let can_impersonate = [Some(service_account.id.as_str()), profile_name]
            .into_iter()
            .flatten()
            .filter_map(|scope| permissions.0.get(scope))
            .flatten()
            .any(|permission| match permission {
                PermissionSetReference::Name(name) => name == "service-account/impersonate",
                PermissionSetReference::Inline(set) => set.id == "service-account/impersonate",
            });
        let impersonator_id = format!("{profile}-sa");
        if can_impersonate
            && impersonator_id != service_account.id
            && ctx.stack.resources().any(|(id, entry)| {
                id == &impersonator_id && entry.config.downcast_ref::<ServiceAccount>().is_some()
            })
        {
            role_arns.push(expr::template(format!(
                "arn:aws:iam::${{data.aws_caller_identity.current.account_id}}:role/${{local.resource_prefix}}-{impersonator_id}"
            )));
        }
    }

    if services.is_empty() && role_arns.is_empty() {
        services.insert("lambda.amazonaws.com");
        services.insert("codebuild.amazonaws.com");
        services.insert("ec2.amazonaws.com");
    }

    TrustPrincipals {
        services,
        role_arns,
    }
}

fn trust_assume_role_policy(services: &[&str], role_arns: Vec<Expression>) -> Expression {
    if role_arns.is_empty() {
        return service_assume_role_policy(services);
    }

    let mut statements = Vec::new();
    if !services.is_empty() {
        let service_principal = if services.len() == 1 {
            Expression::String(services[0].to_string())
        } else {
            Expression::Array(
                services
                    .iter()
                    .map(|service| Expression::String((*service).to_string()))
                    .collect(),
            )
        };
        statements.push(expr::object([
            ("Effect", Expression::String("Allow".to_string())),
            ("Principal", expr::object([("Service", service_principal)])),
            ("Action", Expression::String("sts:AssumeRole".to_string())),
        ]));
    }
    statements.push(expr::object([
        ("Effect", Expression::String("Allow".to_string())),
        (
            "Principal",
            expr::object([(
                "AWS",
                expr::template(
                    "arn:aws:iam::${data.aws_caller_identity.current.account_id}:root".to_string(),
                ),
            )]),
        ),
        ("Action", Expression::String("sts:AssumeRole".to_string())),
        (
            "Condition",
            expr::object([(
                "ArnEquals",
                expr::object([("aws:PrincipalArn", Expression::Array(role_arns))]),
            )]),
        ),
    ]));

    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        ("Statement", Expression::Array(statements)),
    ]))
}
