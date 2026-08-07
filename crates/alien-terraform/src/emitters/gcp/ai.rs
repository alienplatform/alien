//! GCP AI — Vertex AI inference gateway.
//!
//! Vertex AI is a project-level service with no per-stack resource to
//! provision. The emitter returns an empty fragment and carries the project
//! and location in the import ref so the controller can reconstruct the
//! Vertex AI endpoint without a cloud round-trip.
//!
//! The `aiplatform.googleapis.com` API enablement is handled by the
//! `GcpServiceActivationEmitter` when the preflight injects a
//! `ServiceActivation` for that API. The `ai/invoke` custom IAM role (predict
//! only) is emitted by `GcpServiceAccountEmitter` when a permission profile
//! references `ai/invoke`.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        downcast, emit_custom_role_and_bindings_for_target, permission_context, required_label,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{import::EmitContext, Ai, PermissionSetReference, RemoteBindings, Result};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpAiEmitter;

impl TfEmitter for GcpAiEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let ai = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let mut fragment = TfFragment::empty();
        if let (Some(definition), Some(access_label)) = (
            alien_core::remote_bindings::remote_binding_for_entry(ctx.resource),
            remote_bindings_label(ctx),
        ) {
            let permission_ref = PermissionSetReference::from_name(definition.permission_set);
            if let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            {
                let context = permission_context(access_label, ctx.stack.id())
                    .with_resource_name(ai.id().to_string());
                emit_custom_role_and_bindings_for_target(
                    &mut fragment,
                    access_label,
                    &service_account_member_for_label(access_label),
                    &permission_set,
                    &context,
                    BindingTarget::Resource,
                )?;
            }
        }
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            ("location", expr::raw("var.gcp_region")),
        ]))
    }
}

fn remote_bindings_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    })
}
