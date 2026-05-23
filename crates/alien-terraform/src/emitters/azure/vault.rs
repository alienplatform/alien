//! Azure Vault — Key Vault namespace.
//!
//! Mirrors `AzureVaultController`:
//!
//! * Standard SKU, soft-delete on (90-day default), purge protection on
//!   — secrets stay recoverable for the regulatory window even after
//!   `terraform destroy`.
//! * RBAC enabled (`rbac_authorization_enabled = true`); legacy access
//!   policies are not used.
//! * Tenant id sourced from the AzureRM provider's
//!   `data.azurerm_client_config.<resource>_current.tenant_id` so the rendered
//!   module doesn't need an extra customer-supplied variable.

use crate::{
    block::{attr, data_block, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, setup_management_role_label, tags},
    expr,
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSet, PermissionSetReference,
    RemoteStackManagement, Result, Vault,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureVaultEmitter;

impl TfEmitter for AzureVaultEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        // Scope the data source label to the vault resource. Multiple vaults can
        // be rendered into one module, and Terraform requires data labels to be
        // unique per type.
        let client_config_label = format!("{label}_current");
        fragment.data_blocks.push(data_block(
            "azurerm_client_config",
            &client_config_label,
            Vec::<hcl::structure::Structure>::new(),
        ));

        fragment.resource_blocks.push(resource_block(
            "azurerm_key_vault",
            label,
            [
                attr("name", vault_name_expr(vault.id())),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr(
                    "tenant_id",
                    expr::raw(format!(
                        "data.azurerm_client_config.{client_config_label}.tenant_id"
                    )),
                ),
                attr("sku_name", Expression::String("standard".to_string())),
                attr("rbac_authorization_enabled", Expression::Bool(true)),
                attr("purge_protection_enabled", Expression::Bool(true)),
                attr(
                    "soft_delete_retention_days",
                    Expression::Number(hcl::Number::from(90i64)),
                ),
                attr("public_network_access_enabled", Expression::Bool(true)),
                attr("tags", tags(ctx, "vault")),
            ],
        ));

        emit_management_permissions(ctx, label, &mut fragment)?;

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "vaultName",
                expr::traversal(["azurerm_key_vault", label, "name"]),
            ),
            (
                "vaultUri",
                expr::traversal(["azurerm_key_vault", label, "vault_uri"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let _ = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("key-vault".to_string())),
            (
                "vaultName",
                expr::traversal(["azurerm_key_vault", label, "name"]),
            ),
        ])))
    }
}

/// Key Vault names are 3-24 alphanumeric-or-dash characters, globally
/// unique. The runtime controller already enforces these constraints at
/// resource-prefix derivation time; the HCL side trusts that and emits
/// the lower-cased `${stack}-{id}` template.
fn vault_name_expr(vault_id: &str) -> Expression {
    expr::raw(format!(
        "substr(lower(\"${{local.resource_prefix}}-{}\"), 0, 24)",
        vault_id
    ))
}

fn emit_management_permissions(
    ctx: &EmitContext<'_>,
    vault_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let Some(management_label) = remote_stack_management_label(ctx) else {
        return Ok(());
    };

    let principal_id_expr = expr::traversal([
        "azurerm_user_assigned_identity",
        management_label,
        "principal_id",
    ]);

    for permission_set_ref in management_permission_refs(ctx) {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        else {
            continue;
        };
        if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none() {
            continue;
        }

        let role_label = setup_management_role_label(&permission_set.id);
        emit_management_role_assignment(
            fragment,
            ctx.resource_id,
            vault_label,
            &role_label,
            principal_id_expr.clone(),
            &permission_set,
        );
    }

    Ok(())
}

fn emit_management_role_assignment(
    fragment: &mut TfFragment,
    vault_id: &str,
    vault_label: &str,
    role_label: &str,
    principal_id_expr: Expression,
    permission_set: &PermissionSet,
) {
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{vault_label}_{role_label}_assignment"),
        [
            attr(
                "name",
                expr::raw(&format!(
                    "uuidv5(\"oid\", \"deployment:azure:mgmt-res-role-assign:${{local.resource_prefix}}:{}:{}\")",
                    vault_id, permission_set.id
                )),
            ),
            attr(
                "scope",
                expr::traversal(["azurerm_key_vault", vault_label, "id"]),
            ),
            attr(
                "role_definition_id",
                expr::traversal([
                    "azurerm_role_definition",
                    role_label,
                    "role_definition_resource_id",
                ]),
            ),
            attr("principal_id", principal_id_expr),
        ],
    ));
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
    let mut refs = Vec::new();
    refs.extend(resource_permission_refs(profile, ctx.resource_id));
    refs.extend(
        profile
            .0
            .get("*")
            .into_iter()
            .flat_map(|items| items.iter())
            .filter(|reference| reference.id().starts_with("vault/")),
    );
    refs
}

fn resource_permission_refs<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Vec<&'a PermissionSetReference> {
    profile
        .0
        .get(resource_id)
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}
