//! AWS KV — DynamoDB on-demand table with composite key, TTL, SSE, PITR.

use crate::{
    block::{attr, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{downcast, nested_block, required_label, stack_name_template, tags},
    expr,
};
use alien_core::{import::EmitContext, Kv, Result};
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
                attr("name", stack_name_template(kv.id())),
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

        Ok(TfFragment::default().with_resource(table))
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
}
