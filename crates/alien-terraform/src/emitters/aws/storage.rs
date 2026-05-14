//! AWS Storage — S3 bucket emitter.
//!
//! Uses the modern AWS provider idiom: one `aws_s3_bucket` plus separate
//! configuration sub-resources (encryption, ownership controls, public
//! access block, optional versioning, optional lifecycle).

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{downcast, required_label, stack_name_template, tags},
    expr,
};
use alien_core::{import::EmitContext, LifecycleRule, Result, Storage};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsStorageEmitter;

impl TfEmitter for AwsStorageEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let storage = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(bucket(label, ctx, storage));
        fragment.resource_blocks.push(encryption(label));
        fragment.resource_blocks.push(ownership_controls(label));
        fragment
            .resource_blocks
            .push(public_access_block(label, !storage.public_read));
        fragment.resource_blocks.push(bucket_policy(label, storage));

        if storage.versioning {
            fragment.resource_blocks.push(versioning(label));
        }
        if !storage.lifecycle_rules.is_empty() {
            fragment
                .resource_blocks
                .push(lifecycle(label, &storage.lifecycle_rules));
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "bucketName",
                expr::traversal(["aws_s3_bucket", label, "bucket"]),
            ),
            (
                "bucketArn",
                expr::traversal(["aws_s3_bucket", label, "arn"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("s3".to_string())),
            (
                "bucketName",
                expr::traversal(["aws_s3_bucket", label, "bucket"]),
            ),
        ])))
    }
}

fn bucket(label: &str, ctx: &EmitContext<'_>, storage: &Storage) -> hcl::structure::Block {
    resource_block(
        "aws_s3_bucket",
        label,
        [
            attr("bucket", stack_name_template(storage.id())),
            attr("force_destroy", Expression::Bool(true)),
            attr("tags", tags(ctx, "storage")),
        ],
    )
}

fn encryption(label: &str) -> hcl::structure::Block {
    let inner = block(
        "apply_server_side_encryption_by_default",
        [attr(
            "sse_algorithm",
            Expression::String("AES256".to_string()),
        )],
    );
    let rule = block("rule", [nested(inner)]);
    resource_block(
        "aws_s3_bucket_server_side_encryption_configuration",
        label,
        [attr("bucket", bucket_id(label)), nested(rule)],
    )
}

fn ownership_controls(label: &str) -> hcl::structure::Block {
    let rule = block(
        "rule",
        [attr(
            "object_ownership",
            Expression::String("BucketOwnerEnforced".to_string()),
        )],
    );
    resource_block(
        "aws_s3_bucket_ownership_controls",
        label,
        [attr("bucket", bucket_id(label)), nested(rule)],
    )
}

fn public_access_block(label: &str, block_public: bool) -> hcl::structure::Block {
    let value = Expression::Bool(block_public);
    resource_block(
        "aws_s3_bucket_public_access_block",
        label,
        [
            attr("bucket", bucket_id(label)),
            attr("block_public_acls", value.clone()),
            attr("block_public_policy", value.clone()),
            attr("ignore_public_acls", value.clone()),
            attr("restrict_public_buckets", value),
        ],
    )
}

fn bucket_policy(label: &str, storage: &Storage) -> hcl::structure::Block {
    let mut statements = vec![expr::object([
        (
            "Sid",
            Expression::String("DenyInsecureTransport".to_string()),
        ),
        ("Effect", Expression::String("Deny".to_string())),
        ("Principal", Expression::String("*".to_string())),
        ("Action", Expression::String("s3:*".to_string())),
        (
            "Resource",
            Expression::Array(vec![
                expr::traversal(["aws_s3_bucket", label, "arn"]),
                expr::raw(format!("\"${{aws_s3_bucket.{label}.arn}}/*\"")),
            ]),
        ),
        (
            "Condition",
            expr::object([(
                "Bool",
                expr::object([(
                    "aws:SecureTransport",
                    Expression::String("false".to_string()),
                )]),
            )]),
        ),
    ])];

    if storage.public_read {
        statements.push(expr::object([
            ("Sid", Expression::String("AllowPublicRead".to_string())),
            ("Effect", Expression::String("Allow".to_string())),
            ("Principal", Expression::String("*".to_string())),
            ("Action", Expression::String("s3:GetObject".to_string())),
            (
                "Resource",
                expr::raw(format!("\"${{aws_s3_bucket.{label}.arn}}/*\"")),
            ),
        ]));
    }

    let policy = expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        ("Statement", Expression::Array(statements)),
    ]);

    resource_block(
        "aws_s3_bucket_policy",
        label,
        [
            attr("bucket", bucket_id(label)),
            attr("policy", crate::emitters::aws::helpers::jsonencode(policy)),
        ],
    )
}

fn versioning(label: &str) -> hcl::structure::Block {
    let inner = block(
        "versioning_configuration",
        [attr("status", Expression::String("Enabled".to_string()))],
    );
    resource_block(
        "aws_s3_bucket_versioning",
        label,
        [attr("bucket", bucket_id(label)), nested(inner)],
    )
}

fn lifecycle(label: &str, rules: &[LifecycleRule]) -> hcl::structure::Block {
    let mut body: Vec<hcl::structure::Structure> = vec![attr("bucket", bucket_id(label))];
    for (index, rule) in rules.iter().enumerate() {
        let prefix = rule.prefix.clone().unwrap_or_default();
        let filter = block("filter", [attr("prefix", Expression::String(prefix))]);
        let expiration = block(
            "expiration",
            [attr(
                "days",
                Expression::Number(hcl::Number::from(i64::from(rule.days))),
            )],
        );
        let rule_block = block(
            "rule",
            [
                attr("id", Expression::String(format!("rule-{}", index + 1))),
                attr("status", Expression::String("Enabled".to_string())),
                nested(filter),
                nested(expiration),
            ],
        );
        body.push(nested(rule_block));
    }
    resource_block("aws_s3_bucket_lifecycle_configuration", label, body)
}

fn bucket_id(label: &str) -> Expression {
    expr::traversal(["aws_s3_bucket", label, "id"])
}
