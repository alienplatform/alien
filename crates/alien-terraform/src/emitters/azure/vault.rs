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
    emitters::azure::helpers::{downcast, required_label, tags},
    expr,
};
use alien_core::{import::EmitContext, Result, Vault};
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
        "substr(lower(\"${{var.stack_name}}-{}\"), 0, 24)",
        vault_id
    ))
}
