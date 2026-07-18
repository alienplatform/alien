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
        aws_terraform_permission_context, downcast, emit_iam_role_policy, iam_role_name_template,
        jsonencode, required_label, service_assume_role_policy, tags,
    },
    emitters::gates::permission_gate_count,
    expr,
};
use alien_core::{import::EmitContext, Build, ComputeCluster, Result, ServiceAccount, Worker};
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
            compute_role_arns,
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
                    trust_assume_role_policy(&services_ref, compute_role_arns),
                ),
                attr("tags", tags(ctx, "service-account")),
            ],
        ));

        let profile_name = service_account.id.strip_suffix("-sa");
        let context =
            aws_terraform_permission_context().with_resource_name(service_account.id.clone());
        for (index, permission_set) in service_account.stack_permission_sets.iter().enumerate() {
            let gate_count = match profile_name {
                Some(profile) => permission_gate_count(ctx, profile, &permission_set.id, &["*"])?,
                None => None,
            };
            emit_iam_role_policy(
                &mut fragment,
                label,
                permission_set,
                index,
                &context,
                gate_count,
            )?;
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
    compute_role_arns: Vec<Expression>,
}

fn trust_principals(ctx: &EmitContext<'_>, service_account: &ServiceAccount) -> TrustPrincipals {
    let profile_name = service_account.id.strip_suffix("-sa");
    let mut services: BTreeSet<&'static str> = BTreeSet::new();
    let mut compute_role_arns = Vec::new();

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
            compute_role_arns.push(expr::template(format!(
                "arn:aws:iam::${{data.aws_caller_identity.current.account_id}}:role/${{local.resource_prefix}}-{}-instances",
                cluster.id
            )));
        }
    }

    if services.is_empty() && compute_role_arns.is_empty() {
        services.insert("lambda.amazonaws.com");
        services.insert("codebuild.amazonaws.com");
        services.insert("ec2.amazonaws.com");
    }

    TrustPrincipals {
        services,
        compute_role_arns,
    }
}

fn trust_assume_role_policy(services: &[&str], compute_role_arns: Vec<Expression>) -> Expression {
    if compute_role_arns.is_empty() {
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
                "StringEquals",
                expr::object([("aws:PrincipalArn", Expression::Array(compute_role_arns))]),
            )]),
        ),
    ]));

    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        ("Statement", Expression::Array(statements)),
    ]))
}
