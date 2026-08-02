use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{downcast, iam_role_name_template, jsonencode, required_label, tags},
    expr,
};
use alien_core::{import::EmitContext, RemoteBindings, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsRemoteBindingsEmitter;

impl TfEmitter for AwsRemoteBindingsEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<RemoteBindings>(ctx, RemoteBindings::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            label,
            [
                attr("name", iam_role_name_template("remote-bindings")),
                attr("assume_role_policy", trust_policy()),
                attr("tags", tags(ctx, "remote-bindings")),
            ],
        ));
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("roleName", expr::traversal(["aws_iam_role", label, "name"])),
            ("roleArn", expr::traversal(["aws_iam_role", label, "arn"])),
            ("externalId", expr::raw("local.resource_prefix")),
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
                        expr::object([
                            ("aws:PrincipalArn", expr::raw("var.managing_role_arn")),
                            ("sts:ExternalId", expr::raw("local.resource_prefix")),
                        ]),
                    )]),
                ),
            ])]),
        ),
    ]))
}
