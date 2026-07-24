//! AWS KV — DynamoDB on-demand table with composite key, TTL, and SSE.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{required_logical_id, resource_config, tags},
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Kv, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKvEmitter;

impl CfEmitter for AwsKvEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
        let table_id = required_logical_id(ctx)?;
        let mut table = CfResource::new(table_id.to_string(), "AWS::DynamoDB::Table".to_string());

        table.properties.insert(
            "BillingMode".to_string(),
            CfExpression::from("PAY_PER_REQUEST"),
        );
        table.properties.insert(
            "AttributeDefinitions".to_string(),
            CfExpression::list([
                CfExpression::object([
                    ("AttributeName", CfExpression::from("pk")),
                    ("AttributeType", CfExpression::from("S")),
                ]),
                CfExpression::object([
                    ("AttributeName", CfExpression::from("sk")),
                    ("AttributeType", CfExpression::from("S")),
                ]),
            ]),
        );
        table.properties.insert(
            "KeySchema".to_string(),
            CfExpression::list([
                CfExpression::object([
                    ("AttributeName", CfExpression::from("pk")),
                    ("KeyType", CfExpression::from("HASH")),
                ]),
                CfExpression::object([
                    ("AttributeName", CfExpression::from("sk")),
                    ("KeyType", CfExpression::from("RANGE")),
                ]),
            ]),
        );
        table.properties.insert(
            "SSESpecification".to_string(),
            CfExpression::object([("SSEEnabled", CfExpression::from(true))]),
        );
        table.properties.insert(
            "TimeToLiveSpecification".to_string(),
            CfExpression::object([
                ("AttributeName", CfExpression::from("ttl")),
                ("Enabled", CfExpression::from(true)),
            ]),
        );
        table.properties.insert(
            "PointInTimeRecoverySpecification".to_string(),
            CfExpression::object([("PointInTimeRecoveryEnabled", CfExpression::from(true))]),
        );
        table.properties.insert("Tags".to_string(), tags(ctx));
        table.deletion_policy = Some("Retain".to_string());
        table.update_replace_policy = Some("Retain".to_string());

        Ok(vec![table])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let table_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("tableName", CfExpression::ref_(table_id)),
            ("tableArn", CfExpression::get_att(table_id, "Arn")),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let table_id = required_logical_id(ctx)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("dynamodb")),
            ("tableName", CfExpression::ref_(table_id)),
            ("region", CfExpression::ref_("AWS::Region")),
        ])))
    }
}
