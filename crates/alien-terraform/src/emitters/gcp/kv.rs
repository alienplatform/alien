//! GCP KV — Firestore Native database.
//!
//! GCP exposes Firestore as the native key-value primitive. The runtime
//! binding uses Firestore document APIs, so Terraform must provision the
//! same Native-mode database that the runtime controller creates.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label, resource_prefix_template},
    expr,
};
use alien_core::{import::EmitContext, Kv, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpKvEmitter;

impl TfEmitter for GcpKvEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let kv = downcast::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let database = resource_block(
            "google_firestore_database",
            label,
            [
                attr("name", resource_prefix_template(kv.id())),
                attr("project", expr::raw("var.gcp_project")),
                attr("location_id", expr::raw("var.gcp_region")),
                attr("type", Expression::String("FIRESTORE_NATIVE".to_string())),
                attr(
                    "concurrency_mode",
                    Expression::String("OPTIMISTIC".to_string()),
                ),
                attr(
                    "app_engine_integration_mode",
                    Expression::String("DISABLED".to_string()),
                ),
                attr(
                    "delete_protection_state",
                    Expression::String("DELETE_PROTECTION_DISABLED".to_string()),
                ),
                attr("deletion_policy", Expression::String("DELETE".to_string())),
            ],
        );

        Ok(TfFragment::default().with_resource(database))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "databaseId",
                expr::traversal(["google_firestore_database", label, "name"]),
            ),
            (
                "location",
                expr::traversal(["google_firestore_database", label, "location_id"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let kv = downcast::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("firestore".to_string())),
            ("projectId", expr::raw("var.gcp_project")),
            (
                "databaseId",
                expr::traversal(["google_firestore_database", label, "name"]),
            ),
            ("collectionName", Expression::String(kv.id().to_string())),
        ])))
    }
}
