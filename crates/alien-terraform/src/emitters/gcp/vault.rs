//! GCP Vault — Secret Manager namespace.
//!
//! Secret Manager is project-scoped; the vault is realized as a name
//! prefix that secrets carry. ImportData ships the prefix so the
//! controller can list / fetch / put secrets without an extra cloud
//! lookup.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        downcast, emit_custom_role_and_bindings_for_target, permission_context, required_label,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, PermissionSetReference, RemoteStackManagement, Result, Vault,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpVaultEmitter;

impl TfEmitter for GcpVaultEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let vault_label = required_label(ctx)?;
        let Some(management_label) = remote_stack_management_label(ctx) else {
            return Ok(TfFragment::empty());
        };

        let member = service_account_member_for_label(management_label);
        let role_owner_label = format!("{management_label}_{vault_label}");
        let context = permission_context(management_label, ctx.stack.id())
            .with_resource_name(format!("${{local.resource_prefix}}-{}", vault.id()));
        let mut fragment = TfFragment::default();

        for permission_set_ref in management_permission_refs(ctx) {
            if let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            {
                if permission_set.id.ends_with("/provision") {
                    continue;
                }
                emit_custom_role_and_bindings_for_target(
                    &mut fragment,
                    &role_owner_label,
                    &member,
                    &permission_set,
                    &context,
                    BindingTarget::Resource,
                )?;
            }
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "secretPrefix",
                expr::template(format!("${{local.resource_prefix}}-{}", vault.id())),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        Ok(Some(expr::object([
            ("service", Expression::String("secret-manager".to_string())),
            (
                "vaultPrefix",
                expr::template(format!("${{local.resource_prefix}}-{}", vault.id())),
            ),
        ])))
    }
}

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}

fn management_permission_refs<'a>(ctx: &'a EmitContext<'_>) -> Vec<&'a PermissionSetReference> {
    let Some(profile) = ctx.stack.management().profile() else {
        return Vec::new();
    };
    profile
        .0
        .get(ctx.resource_id)
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}
