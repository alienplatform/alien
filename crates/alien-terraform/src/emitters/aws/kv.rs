//! AWS KV — DynamoDB on-demand table with composite key, TTL, SSE, PITR.

use crate::{
    block::{attr, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        nested_block, required_label, resource_prefix_template, tags,
    },
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Kv, RemoteBindings, Result};
use alien_error::AlienError;
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKvEmitter;

impl TfEmitter for AwsKvEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let kv = downcast::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let table = resource_block(
            "aws_dynamodb_table",
            label,
            [
                attr("name", resource_prefix_template(kv.id())),
                attr(
                    "billing_mode",
                    Expression::String("PAY_PER_REQUEST".to_string()),
                ),
                attr("hash_key", Expression::String("pk".to_string())),
                attr("range_key", Expression::String("sk".to_string())),
                nested_block(
                    "attribute",
                    vec![
                        attr("name", Expression::String("pk".to_string())),
                        attr("type", Expression::String("S".to_string())),
                    ],
                ),
                nested_block(
                    "attribute",
                    vec![
                        attr("name", Expression::String("sk".to_string())),
                        attr("type", Expression::String("S".to_string())),
                    ],
                ),
                nested_block(
                    "ttl",
                    vec![
                        attr("attribute_name", Expression::String("ttl".to_string())),
                        attr("enabled", Expression::Bool(true)),
                    ],
                ),
                nested_block(
                    "server_side_encryption",
                    vec![attr("enabled", Expression::Bool(true))],
                ),
                nested_block(
                    "point_in_time_recovery",
                    vec![attr("enabled", Expression::Bool(true))],
                ),
                attr("tags", tags(ctx, "kv")),
                nested(crate::block::block(
                    "lifecycle",
                    [attr("prevent_destroy", Expression::Bool(false))],
                )),
            ],
        );

        let mut fragment = TfFragment::default().with_resource(table);
        if let Some(definition) =
            alien_core::remote_bindings::remote_binding_for_entry(ctx.resource)
        {
            if let Some(access_label) = ctx.stack.resources().find_map(|(id, entry)| {
                (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
                    .then(|| ctx.name_for(id))
                    .flatten()
            }) {
                let permission_set = alien_permissions::get_permission_set(
                    definition.permission_set,
                )
                .ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!(
                            "Remote KV permission set {} is not registered",
                            definition.permission_set
                        ),
                    })
                })?;
                let context = aws_terraform_permission_context()
                    .with_resource_name(format!("${{aws_dynamodb_table.{label}.name}}"));
                emit_iam_role_policy_for_target_with_label(
                    &mut fragment,
                    access_label,
                    permission_set,
                    &format!("{label}_remote_access"),
                    &format!("access-{}", kv.id()),
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
            (
                "tableName",
                expr::traversal(["aws_dynamodb_table", label, "name"]),
            ),
            (
                "tableArn",
                expr::traversal(["aws_dynamodb_table", label, "arn"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("dynamodb".to_string())),
            (
                "tableName",
                expr::traversal(["aws_dynamodb_table", label, "name"]),
            ),
            ("region", expr::raw("data.aws_region.current.region")),
        ])))
    }
}
