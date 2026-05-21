//! Azure ServiceAccount — User-Assigned Managed Identity (UAMI) plus
//! per-permission-set `azurerm_role_definition` + `azurerm_role_assignment`.
//!
//! Mirrors `AzureServiceAccountController` exactly:
//!
//! 1. `azurerm_user_assigned_identity` for the workload identity. The
//!    UAMI's `principal_id` is what gets bound to role assignments.
//! 2. One `azurerm_role_definition` per `PermissionSet` attached via
//!    `stack_permission_sets`, produced through
//!    `AzureRuntimePermissionsGenerator::generate_role_definition` so the
//!    same role schema lands on push and pull deployments.
//! 3. One `azurerm_role_assignment` per permission set, bound at
//!    subscription/resource-group scope via the helper.
//!
//! AKS workload-identity wiring (federated identity credentials trusting
//! the cluster's OIDC issuer for the matching K8s service account)
//! happens in the K8s identity overlay
//! ([`crate::k8s_identity::apply_aks`]) — that overlay activates as soon
//! as this emitter pushes a `azurerm_user_assigned_identity` block into
//! the per-resource fragment.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, emit_role_definition_and_assignments, permission_context, required_label,
        resource_prefix_template, tags,
    },
    expr,
};
use alien_core::{import::EmitContext, Result, ServiceAccount};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureServiceAccountEmitter;

impl TfEmitter for AzureServiceAccountEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let service_account = downcast::<ServiceAccount>(ctx, ServiceAccount::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(resource_block(
            "azurerm_user_assigned_identity",
            label,
            [
                attr("name", resource_prefix_template(&service_account.id)),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("tags", tags(ctx, "service-account")),
            ],
        ));

        let principal_id_expr =
            expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]);
        let context = permission_context(label);

        for (role_index, permission_set) in service_account.stack_permission_sets.iter().enumerate()
        {
            emit_role_definition_and_assignments(
                &mut fragment,
                label,
                &service_account.id,
                role_index,
                principal_id_expr.clone(),
                permission_set,
                &context,
            )?;
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "identityId",
                expr::traversal(["azurerm_user_assigned_identity", label, "id"]),
            ),
            (
                "principalId",
                expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]),
            ),
            (
                "clientId",
                expr::traversal(["azurerm_user_assigned_identity", label, "client_id"]),
            ),
            ("stackPermissionsApplied", Expression::Bool(true)),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            (
                "service",
                Expression::String("azuremanagedidentity".to_string()),
            ),
            (
                "clientId",
                expr::traversal(["azurerm_user_assigned_identity", label, "client_id"]),
            ),
            (
                "resourceId",
                expr::traversal(["azurerm_user_assigned_identity", label, "id"]),
            ),
            (
                "principalId",
                expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]),
            ),
        ])))
    }
}
