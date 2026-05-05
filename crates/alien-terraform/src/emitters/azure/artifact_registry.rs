//! Azure ArtifactRegistry — Azure Container Registry (ACR).
//!
//! One Premium-SKU registry per Alien `ArtifactRegistry`. Pull and push
//! flow through two purpose-built UAMIs (RBAC, not admin user — admin
//! is disabled by default). Each UAMI gets the matching built-in role
//! (`AcrPull` / `AcrPush`) at the registry scope so ECR-equivalent
//! image lifecycle works out of the box.
//!
//! Premium SKU is the only tier that supports both VNet integration and
//! private endpoints — required for the Container Apps + AKS ingestion
//! paths used downstream. Customers who want Standard pin the resource
//! by name and override at the controller layer; we don't surface a
//! per-resource SKU variable yet.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, stack_name_template, tags},
    expr,
};
use alien_core::{import::EmitContext, ArtifactRegistry, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureArtifactRegistryEmitter;

impl TfEmitter for AzureArtifactRegistryEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let registry = downcast::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let pull_label = format!("{label}_pull");
        let push_label = format!("{label}_push");

        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(resource_block(
            "azurerm_container_registry",
            label,
            [
                attr("name", registry_name_expr(registry.id())),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("sku", Expression::String("Premium".to_string())),
                attr("admin_enabled", Expression::Bool(false)),
                attr("tags", tags(ctx, "artifact-registry")),
            ],
        ));

        for (suffix, role, uami_label) in [
            ("pull", "AcrPull", pull_label.clone()),
            ("push", "AcrPush", push_label.clone()),
        ] {
            fragment.resource_blocks.push(resource_block(
                "azurerm_user_assigned_identity",
                &uami_label,
                [
                    attr(
                        "name",
                        stack_name_template(&format!("{}-{suffix}", registry.id())),
                    ),
                    attr(
                        "resource_group_name",
                        expr::raw("var.azure_resource_group_name"),
                    ),
                    attr("location", expr::raw("var.azure_location")),
                    attr("tags", tags(ctx, "artifact-registry")),
                ],
            ));

            fragment.resource_blocks.push(resource_block(
                "azurerm_role_assignment",
                &format!("{uami_label}_role"),
                [
                    attr(
                        "name",
                        expr::raw(format!(
                            "uuidv5(\"dns\", \"${{var.stack_name}}-{}-{suffix}-${{azurerm_user_assigned_identity.{uami_label}.principal_id}}\")",
                            registry.id()
                        )),
                    ),
                    attr(
                        "scope",
                        expr::traversal(["azurerm_container_registry", label, "id"]),
                    ),
                    attr(
                        "role_definition_name",
                        Expression::String(role.to_string()),
                    ),
                    attr(
                        "principal_id",
                        expr::traversal([
                            "azurerm_user_assigned_identity",
                            &uami_label,
                            "principal_id",
                        ]),
                    ),
                ],
            ));
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let pull_label = format!("{label}_pull");
        let push_label = format!("{label}_push");
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "registryName",
                expr::traversal(["azurerm_container_registry", label, "name"]),
            ),
            (
                "loginServer",
                expr::traversal(["azurerm_container_registry", label, "login_server"]),
            ),
            (
                "pullPrincipalId",
                expr::traversal([
                    "azurerm_user_assigned_identity",
                    &pull_label,
                    "principal_id",
                ]),
            ),
            (
                "pushPrincipalId",
                expr::traversal([
                    "azurerm_user_assigned_identity",
                    &push_label,
                    "principal_id",
                ]),
            ),
        ]))
    }
}

/// ACR names are 5-50 alphanumeric characters (no dashes!), globally
/// unique. Strip dashes from the rendered name so customer stack names
/// containing them don't break naming validation.
fn registry_name_expr(registry_id: &str) -> Expression {
    expr::raw(format!(
        "substr(replace(lower(\"${{var.stack_name}}{}\"), \"-\", \"\"), 0, 50)",
        registry_id
    ))
}
