//! GCP Storage — Cloud Storage bucket with uniform bucket-level access.
//!
//! Defaults closed by design: uniform bucket-level access (no ACLs),
//! public access prevention enforced, soft-delete versioning when
//! requested, lifecycle rules translated to GCP `lifecycle_rule`
//! blocks.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, labels, required_label, stack_name_template},
    expr,
};
use alien_core::{import::EmitContext, LifecycleRule, Result, Storage};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpStorageEmitter;

impl TfEmitter for GcpStorageEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let storage = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(bucket(label, ctx, storage));

        if storage.public_read {
            fragment.resource_blocks.push(public_iam_binding(label));
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "bucketName",
                expr::traversal(["google_storage_bucket", label, "name"]),
            ),
            (
                "bucketSelfLink",
                expr::traversal(["google_storage_bucket", label, "self_link"]),
            ),
            ("projectId", expr::raw("var.gcp_project")),
            (
                "location",
                expr::traversal(["google_storage_bucket", label, "location"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("gcs".to_string())),
            (
                "bucketName",
                expr::traversal(["google_storage_bucket", label, "name"]),
            ),
        ])))
    }
}

fn bucket(label: &str, ctx: &EmitContext<'_>, storage: &Storage) -> hcl::structure::Block {
    let mut body: Vec<hcl::structure::Structure> = vec![
        attr("name", stack_name_template(storage.id())),
        attr("project", expr::raw("var.gcp_project")),
        attr("location", expr::raw("upper(var.gcp_region)")),
        attr("storage_class", Expression::String("STANDARD".to_string())),
        attr("uniform_bucket_level_access", Expression::Bool(true)),
        attr("force_destroy", Expression::Bool(true)),
        attr(
            "public_access_prevention",
            Expression::String(if storage.public_read {
                "inherited".to_string()
            } else {
                "enforced".to_string()
            }),
        ),
        attr("labels", labels(ctx, "storage")),
    ];

    if storage.versioning {
        body.push(nested(block(
            "versioning",
            [attr("enabled", Expression::Bool(true))],
        )));
    }

    for rule in &storage.lifecycle_rules {
        body.push(nested(lifecycle_rule_block(rule)));
    }

    resource_block("google_storage_bucket", label, body)
}

fn lifecycle_rule_block(rule: &LifecycleRule) -> hcl::structure::Block {
    let mut condition_attrs: Vec<hcl::structure::Structure> = vec![attr(
        "age",
        Expression::Number(hcl::Number::from(i64::from(rule.days))),
    )];
    if let Some(prefix) = &rule.prefix {
        condition_attrs.push(attr(
            "matches_prefix",
            Expression::Array(vec![Expression::String(prefix.clone())]),
        ));
    }
    block(
        "lifecycle_rule",
        [
            nested(block(
                "action",
                [attr("type", Expression::String("Delete".to_string()))],
            )),
            nested(block("condition", condition_attrs)),
        ],
    )
}

fn public_iam_binding(label: &str) -> hcl::structure::Block {
    resource_block(
        "google_storage_bucket_iam_member",
        &format!("{label}_public_read"),
        [
            attr(
                "bucket",
                expr::traversal(["google_storage_bucket", label, "name"]),
            ),
            attr(
                "role",
                Expression::String("roles/storage.objectViewer".to_string()),
            ),
            attr("member", Expression::String("allUsers".to_string())),
        ],
    )
}
