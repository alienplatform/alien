//! AWS RemoteStackManagement — cross-account IAM role.
//!
//! Trust policy allows the manager identity (passed as
//! `var.managing_role_arn`) to assume the role.
//!
//! Inline policies come from `AwsRuntimePermissionsGenerator` over the
//! materialised management profile (the `ManagementPermissionProfileMutation`
//! preflight turns `Auto` into an explicit `Extend` before generation
//! runs). One `aws_iam_role_policy` per permission set, sharing the
//! same role — symmetric with the per-permission-set custom roles GCP
//! emits and with what the runtime controller would attach via
//! `put_role_policy`.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, jsonencode, required_label,
        stack_name_template, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSetReference, RemoteStackManagement, Result,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsRemoteStackManagementEmitter;

impl TfEmitter for AwsRemoteStackManagementEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            label,
            [
                attr("name", stack_name_template("management")),
                attr("assume_role_policy", trust_policy()),
                attr("tags", tags(ctx, "remote-stack-management")),
            ],
        ));

        let context =
            aws_terraform_permission_context().with_resource_name("management".to_string());
        let mut policy_index = 0usize;
        if let Some(profile) = ctx.stack.management().profile() {
            for permission_set_ref in global_permission_refs(profile) {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    crate::emitters::aws::helpers::emit_iam_role_policy(
                        &mut fragment,
                        label,
                        &permission_set,
                        policy_index,
                        &context,
                    )?;
                    policy_index += 1;
                }
            }
        }

        // The management role always needs to read its own role —
        // `iam:GetRole` is what the manager calls to verify the role
        // still exists at heartbeat time. Lives outside alien-permissions
        // because it's a self-reference the controller can't model
        // through a permission set without weird circular naming.
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role_policy",
            &format!("{label}_self_read"),
            [
                attr(
                    "name",
                    Expression::String("alien-management-self-read".to_string()),
                ),
                attr("role", expr::traversal(["aws_iam_role", label, "id"])),
                attr("policy", self_read_policy(label)),
            ],
        ));

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("roleName", expr::traversal(["aws_iam_role", label, "name"])),
            ("roleArn", expr::traversal(["aws_iam_role", label, "arn"])),
            ("managementPermissionsApplied", Expression::Bool(true)),
        ]))
    }
}

fn trust_policy() -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                ("Sid", Expression::String("AllowManagingRole".to_string())),
                ("Effect", Expression::String("Allow".to_string())),
                (
                    "Principal",
                    expr::object([("AWS", expr::raw("var.managing_role_arn"))]),
                ),
                ("Action", Expression::String("sts:AssumeRole".to_string())),
                (
                    "Condition",
                    expr::object([(
                        "StringEquals",
                        expr::object([("aws:PrincipalArn", expr::raw("var.managing_role_arn"))]),
                    )]),
                ),
            ])]),
        ),
    ]))
}

fn self_read_policy(_label: &str) -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                (
                    "Sid",
                    Expression::String("ReadOwnManagementRole".to_string()),
                ),
                ("Effect", Expression::String("Allow".to_string())),
                ("Action", Expression::String("iam:GetRole".to_string())),
                (
                    "Resource",
                    expr::template(
                        "arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${var.stack_name}-management"
                            .to_string(),
                    ),
                ),
            ])]),
        ),
    ]))
}

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}
