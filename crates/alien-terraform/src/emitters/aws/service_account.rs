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
        aws_terraform_permission_context, downcast, emit_iam_role_policy, required_label,
        service_assume_role_policy, stack_name_template, tags,
    },
    expr,
};
use alien_core::{import::EmitContext, Build, Function, Result, ServiceAccount};
use hcl::expr::Expression;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsServiceAccountEmitter;

impl TfEmitter for AwsServiceAccountEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let service_account = downcast::<ServiceAccount>(ctx, ServiceAccount::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let services = trust_principals(ctx, service_account);
        let services_ref: Vec<&str> = services.iter().copied().collect();

        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            label,
            [
                attr("name", stack_name_template(&service_account.id)),
                attr(
                    "assume_role_policy",
                    service_assume_role_policy(&services_ref),
                ),
                attr("tags", tags(ctx, "service-account")),
            ],
        ));

        let context =
            aws_terraform_permission_context().with_resource_name(service_account.id.clone());
        for (index, permission_set) in service_account.stack_permission_sets.iter().enumerate() {
            emit_iam_role_policy(&mut fragment, label, permission_set, index, &context)?;
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

fn trust_principals<'a>(
    ctx: &'a EmitContext<'_>,
    service_account: &'a ServiceAccount,
) -> BTreeSet<&'static str> {
    let profile_name = service_account.id.strip_suffix("-sa");
    let mut services: BTreeSet<&'static str> = BTreeSet::new();

    for (_id, entry) in ctx.stack.resources() {
        if let Some(function) = entry.config.downcast_ref::<Function>() {
            if Some(function.permissions.as_str()) == profile_name {
                services.insert("lambda.amazonaws.com");
            }
        }
        if let Some(build) = entry.config.downcast_ref::<Build>() {
            if Some(build.permissions.as_str()) == profile_name {
                services.insert("codebuild.amazonaws.com");
            }
        }
    }

    if services.is_empty() {
        services.insert("lambda.amazonaws.com");
        services.insert("codebuild.amazonaws.com");
        services.insert("ec2.amazonaws.com");
    }

    services
}
