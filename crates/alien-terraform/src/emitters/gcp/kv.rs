//! GCP KV — Firestore Native database.
//!
//! GCP exposes Firestore as the native key-value primitive. The runtime
//! binding uses Firestore document APIs, so Terraform must provision the
//! same Native-mode database that the runtime controller creates.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        downcast, emit_custom_role_and_bindings_for_target, permission_context, required_label,
        resource_prefix_template, service_account_member_for_label,
    },
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Kv, RemoteBindings, Result};
use alien_error::AlienError;
use alien_permissions::BindingTarget;
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

        let mut fragment = TfFragment::default().with_resource(database);
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
                let context = permission_context(access_label, ctx.stack.id())
                    .with_resource_name(kv.id().to_string());
                emit_custom_role_and_bindings_for_target(
                    &mut fragment,
                    &format!("{label}_access"),
                    &service_account_member_for_label(access_label),
                    permission_set,
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
