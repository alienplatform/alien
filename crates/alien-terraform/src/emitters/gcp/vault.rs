//! GCP Vault — Secret Manager namespace.
//!
//! Secret Manager is project-scoped; the vault is realized as a name
//! prefix that secrets carry. ImportData ships the prefix so the
//! controller can list / fetch / put secrets without an extra cloud
//! lookup.

use crate::{
    block::{attr, data_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{
        downcast, emit_custom_role_and_bindings_for_target, permission_context, required_label,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSetReference, RemoteStackManagement, Result,
    ServiceAccount, Vault,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpVaultEmitter;

impl TfEmitter for GcpVaultEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let vault_label = required_label(ctx)?;
        let mut fragment = TfFragment::default();
        let vault_permission_owners = vault_permission_owners(ctx);

        if !vault_permission_owners.is_empty() && remote_stack_management_label(ctx).is_none() {
            fragment.data_blocks.push(data_block(
                "google_project",
                "current",
                [attr("project_id", expr::raw("var.gcp_project"))],
            ));
        }

        for (owner_label, permission_set_refs) in vault_permission_owners {
            let member = service_account_member_for_label(&owner_label);
            let role_owner_label = format!("{owner_label}_{vault_label}");
            let context = permission_context(&owner_label, ctx.stack.id())
                .with_resource_name(format!("${{local.resource_prefix}}-{}", vault.id()));

            for permission_set_ref in permission_set_refs {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    if permission_set.id.ends_with("/provision") {
                        continue;
                    }
                    let binding_owner_label =
                        binding_owner_label(&role_owner_label, &permission_set.id);
                    emit_custom_role_and_bindings_for_target(
                        &mut fragment,
                        &binding_owner_label,
                        &member,
                        &permission_set,
                        &context,
                        BindingTarget::Resource,
                    )?;
                }
            }
        }

        let Some(management_label) = remote_stack_management_label(ctx) else {
            return Ok(fragment);
        };

        let member = service_account_member_for_label(management_label);
        let role_owner_label = format!("{management_label}_{vault_label}");
        let context = permission_context(management_label, ctx.stack.id())
            .with_resource_name(format!("${{local.resource_prefix}}-{}", vault.id()));

        for permission_set_ref in management_permission_refs(ctx) {
            if let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            {
                if permission_set.id.ends_with("/provision") {
                    continue;
                }
                let binding_owner_label =
                    binding_owner_label(&role_owner_label, &permission_set.id);
                emit_custom_role_and_bindings_for_target(
                    &mut fragment,
                    &binding_owner_label,
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
    resource_permission_refs(profile, ctx.resource_id)
}

fn vault_permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some((label, _service_account)) = service_account_for_id(ctx, &service_account_id) {
            owners.push((label.to_string(), refs));
        }
    }

    owners
}

fn resource_permission_refs<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Vec<&'a PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("vault/"))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    refs
}

fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<(&'a str, &'a ServiceAccount)> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    let service_account = entry.config.downcast_ref::<ServiceAccount>()?;
    let label = ctx.name_for(service_account_id)?;
    Some((label, service_account))
}

fn binding_owner_label(owner_label: &str, permission_set_id: &str) -> String {
    format!(
        "{owner_label}_{}",
        permission_set_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    )
}
