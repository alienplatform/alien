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
    emitters::azure::helpers::{
        binding_string_expr, container_apps_environment_binding, downcast, required_label, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, AzureContainerAppsEnvironment, Network, NetworkSettings, Result,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureContainerAppsEnvironmentEmitter;

impl TfEmitter for AzureContainerAppsEnvironmentEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<AzureContainerAppsEnvironment>(
            ctx,
            AzureContainerAppsEnvironment::RESOURCE_TYPE,
        )?;
        if container_apps_environment_binding(ctx, ctx.resource_id)?.is_some() {
            return Ok(TfFragment::empty());
        }
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
                        "replace(lower(\"${{local.resource_prefix}}-{label}-logs\"), \"_\", \"-\")"
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

        let mut env_body = vec![
            attr(
                "name",
                expr::raw(format!(
                    "replace(lower(\"${{local.resource_prefix}}-{label}\"), \"_\", \"-\")"
                )),
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
        ];

        // VNet-integrate the environment when the stack has a Network, so Container Apps can reach
        // private-only resources (a Flexible Server reachable only through its private endpoint).
        // Without this the environment is public and a worker's connection to the private server
        // times out. Managed networks give the environment its own delegated infrastructure
        // subnet so VMSS NICs can continue using the private workload subnet. BYO networks retain
        // their explicitly supplied private subnet contract. `internal_load_balancer_enabled =
        // false` keeps public ingress while egressing through the VNet.
        if let Some(infrastructure_subnet_id) = infrastructure_subnet_id(ctx) {
            env_body.push(attr("infrastructure_subnet_id", infrastructure_subnet_id));
            env_body.push(attr(
                "internal_load_balancer_enabled",
                Expression::Bool(false),
            ));
        }

        env_body.push(attr("tags", tags(ctx, "container-apps-environment")));

        fragment.resource_blocks.push(resource_block(
            "azurerm_container_app_environment",
            label,
            env_body,
        ));

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        if let Some(binding) = container_apps_environment_binding(ctx, ctx.resource_id)? {
            return Ok(expr::object([
                ("subscriptionId", expr::raw("var.azure_subscription_id")),
                (
                    "resourceGroup",
                    binding_string_expr(
                        ctx.resource_id,
                        "resource_group_name",
                        &binding.resource_group_name,
                    )?,
                ),
                (
                    "environmentName",
                    binding_string_expr(
                        ctx.resource_id,
                        "environment_name",
                        &binding.environment_name,
                    )?,
                ),
                (
                    "resourceId",
                    binding_string_expr(ctx.resource_id, "resource_id", &binding.resource_id)?,
                ),
                (
                    "defaultDomain",
                    binding_string_expr(
                        ctx.resource_id,
                        "default_domain",
                        &binding.default_domain,
                    )?,
                ),
                ("customDomainVerificationId", expr::raw("null")),
            ]));
        }

        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "environmentName",
                expr::traversal(["azurerm_container_app_environment", label, "name"]),
            ),
            (
                "resourceId",
                expr::traversal(["azurerm_container_app_environment", label, "id"]),
            ),
            (
                "defaultDomain",
                expr::traversal(["azurerm_container_app_environment", label, "default_domain"]),
            ),
            (
                "customDomainVerificationId",
                expr::traversal([
                    "azurerm_container_app_environment",
                    label,
                    "custom_domain_verification_id",
                ]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        if let Some(binding) = container_apps_environment_binding(ctx, ctx.resource_id)? {
            return Ok(Some(expr::object([
                (
                    "environmentName",
                    binding_string_expr(
                        ctx.resource_id,
                        "environment_name",
                        &binding.environment_name,
                    )?,
                ),
                (
                    "resourceId",
                    binding_string_expr(ctx.resource_id, "resource_id", &binding.resource_id)?,
                ),
                (
                    "resourceGroupName",
                    binding_string_expr(
                        ctx.resource_id,
                        "resource_group_name",
                        &binding.resource_group_name,
                    )?,
                ),
                (
                    "defaultDomain",
                    binding_string_expr(
                        ctx.resource_id,
                        "default_domain",
                        &binding.default_domain,
                    )?,
                ),
                ("customDomainVerificationId", expr::raw("null")),
            ])));
        }

        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            (
                "environmentName",
                expr::traversal(["azurerm_container_app_environment", label, "name"]),
            ),
            (
                "resourceId",
                expr::traversal(["azurerm_container_app_environment", label, "id"]),
            ),
            (
                "resourceGroupName",
                expr::raw("var.azure_resource_group_name"),
            ),
            (
                "defaultDomain",
                expr::traversal(["azurerm_container_app_environment", label, "default_domain"]),
            ),
            (
                "customDomainVerificationId",
                expr::traversal([
                    "azurerm_container_app_environment",
                    label,
                    "custom_domain_verification_id",
                ]),
            ),
        ])))
    }
}

/// The `infrastructure_subnet_id` expression for the environment, or `None` when the stack has no
/// Network (then the environment stays public, matching the runtime controller's
/// `get_vnet_configuration` returning `None`).
///
/// Resolves to the network emitter's dedicated Container Apps subnet for managed networks and the
/// explicitly supplied private subnet in bring-your-own-VNet mode.
fn infrastructure_subnet_id(ctx: &EmitContext<'_>) -> Option<Expression> {
    let (network_id, entry) = ctx
        .stack
        .resources()
        .find(|(_id, entry)| entry.config.resource_type() == Network::RESOURCE_TYPE)?;
    let network = entry.config.downcast_ref::<Network>()?;
    let network_label = ctx.name_for(network_id)?;
    let byo = matches!(network.settings, NetworkSettings::ByoVnetAzure { .. });
    Some(if byo {
        expr::traversal([
            "data",
            "azurerm_subnet",
            &format!("{network_label}_private"),
            "id",
        ])
    } else {
        expr::traversal([
            "azurerm_subnet",
            &format!("{network_label}_container_apps"),
            "id",
        ])
    })
}
