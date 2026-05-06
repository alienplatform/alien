//! Azure ContainerAppsEnvironment (auxiliary) —
//! `azurerm_container_app_environment`.
//!
//! Preflight-injected as `default-container-apps-environment`. Every
//! Alien `Function` resource lands as a Container App inside this
//! environment.
//!
//! ## Log analytics requirement
//!
//! Container Apps environments require a Log Analytics workspace for
//! Diagnostic Settings (the only place container logs land). The runtime
//! controller co-creates one; the emitter does the same so the rendered
//! module is self-contained — pointer at the workspace via
//! `log_analytics_workspace_id` on the environment block.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, tags},
    expr,
};
use alien_core::{import::EmitContext, AzureContainerAppsEnvironment, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureContainerAppsEnvironmentEmitter;

impl TfEmitter for AzureContainerAppsEnvironmentEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<AzureContainerAppsEnvironment>(
            ctx,
            AzureContainerAppsEnvironment::RESOURCE_TYPE,
        )?;
        let label = required_label(ctx)?;
        let workspace_label = format!("{label}_logs");

        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "azurerm_log_analytics_workspace",
            &workspace_label,
            [
                attr(
                    "name",
                    expr::raw(format!(
                        "replace(lower(\"${{var.stack_name}}-{label}-logs\"), \"_\", \"-\")"
                    )),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("sku", Expression::String("PerGB2018".to_string())),
                attr(
                    "retention_in_days",
                    Expression::Number(hcl::Number::from(30i64)),
                ),
                attr("tags", tags(ctx, "container-apps-environment")),
            ],
        ));

        fragment.resource_blocks.push(resource_block(
            "azurerm_container_app_environment",
            label,
            [
                attr(
                    "name",
                    expr::template(format!("${{var.stack_name}}-{label}")),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr(
                    "log_analytics_workspace_id",
                    expr::traversal(["azurerm_log_analytics_workspace", &workspace_label, "id"]),
                ),
                attr("tags", tags(ctx, "container-apps-environment")),
            ],
        ));

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "environmentName",
                expr::traversal(["azurerm_container_app_environment", label, "name"]),
            ),
            (
                "defaultDomain",
                expr::traversal(["azurerm_container_app_environment", label, "default_domain"]),
            ),
        ]))
    }
}
