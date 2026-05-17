//! Azure RemoteStackManagement — management User-Assigned Managed Identity
//! plus optional Federated Identity Credential.
//!
//! Mirrors `AzureRemoteStackManagementController`:
//!
//! 1. `azurerm_user_assigned_identity` for the management identity
//!    living in the customer's subscription / resource group.
//! 2. One combined custom management role built from global
//!    `/provision` permission sets.
//! 3. Role assignment to the management identity, plus optional service
//!    principal fallback for local development.
//! 4. Optional `azurerm_federated_identity_credential` when the caller
//!    supplies `azure_oidc_issuer` and `azure_oidc_subject`.
//! 5. AcrPush subscription-scope assignment when management permissions
//!    need registry access.

use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, permission_context, required_label, tags},
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionProfile, PermissionSet, PermissionSetReference,
    RemoteStackManagement, Result,
};
use alien_error::Context;
use alien_permissions::{generators::AzureRuntimePermissionsGenerator, BindingTarget};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureRemoteStackManagementEmitter;

impl TfEmitter for AzureRemoteStackManagementEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        fragment.data_blocks.push(data_block(
            "azurerm_client_config",
            &format!("{label}_current"),
            [],
        ));

        fragment.resource_blocks.push(resource_block(
            "azurerm_user_assigned_identity",
            label,
            [
                attr(
                    "name",
                    expr::template("${var.stack_name}-management-identity".to_string()),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("tags", tags(ctx, "remote-stack-management")),
            ],
        ));

        let global_refs = ctx
            .stack
            .management()
            .profile()
            .map(global_permission_refs)
            .unwrap_or_default();
        emit_management_role(&mut fragment, label, &global_refs)?;
        emit_management_identity_assignment(&mut fragment, label);
        emit_service_principal_fallback_assignment(&mut fragment, label);

        fragment.resource_blocks.push(resource_block(
            "azurerm_federated_identity_credential",
            &format!("{label}_fic"),
            [
                attr(
                    "count",
                    expr::raw(
                        "var.azure_oidc_issuer == \"\" || var.azure_oidc_subject == \"\" ? 0 : 1",
                    ),
                ),
                attr(
                    "name",
                    expr::template("${var.stack_name}-alien-fic".to_string()),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr(
                    "parent_id",
                    expr::traversal(["azurerm_user_assigned_identity", label, "id"]),
                ),
                attr(
                    "audience",
                    Expression::Array(vec![Expression::String(
                        "api://AzureADTokenExchange".to_string(),
                    )]),
                ),
                attr("issuer", expr::raw("var.azure_oidc_issuer")),
                attr("subject", expr::raw("var.azure_oidc_subject")),
            ],
        ));
        if needs_acr_push_assignment(&global_refs) {
            emit_acr_push_assignment(&mut fragment, label);
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "tenantId",
                expr::traversal([
                    "data",
                    "azurerm_client_config",
                    &format!("{label}_current"),
                    "tenant_id",
                ]),
            ),
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
            ("managementPermissionsApplied", Expression::Bool(true)),
        ]))
    }
}

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}

fn emit_management_role(
    fragment: &mut TfFragment,
    label: &str,
    global_refs: &[&PermissionSetReference],
) -> Result<()> {
    let (actions, data_actions) = combined_provision_permissions(label, global_refs)?;
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_definition",
        &format!("{label}_management_role"),
        [
            attr("name", expr::template("${var.stack_name}-management-role".to_string())),
            attr(
                "role_definition_id",
                expr::raw("uuidv5(\"oid\", \"alien:azure:mgmt-role-def:${var.stack_name}\")"),
            ),
            attr(
                "scope",
                expr::raw(
                    "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"",
                ),
            ),
            attr(
                "description",
                expr::template("Management role for Alien stack '${var.stack_name}'".to_string()),
            ),
            nested(block(
                "permissions",
                [
                    attr("actions", string_array(actions)),
                    attr("data_actions", string_array(data_actions)),
                    attr("not_actions", Expression::Array(Vec::new())),
                    attr("not_data_actions", Expression::Array(Vec::new())),
                ],
            )),
            attr(
                "assignable_scopes",
                Expression::Array(vec![expr::raw(
                    "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"",
                )]),
            ),
        ],
    ));
    Ok(())
}

fn emit_management_identity_assignment(fragment: &mut TfFragment, label: &str) {
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{label}_management_uami_assignment"),
        [
            attr(
                "name",
                expr::raw("uuidv5(\"oid\", \"alien:azure:mgmt-role-assign:${var.stack_name}:uami\")"),
            ),
            attr(
                "scope",
                expr::raw(
                    "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"",
                ),
            ),
            attr(
                "role_definition_id",
                expr::traversal([
                    "azurerm_role_definition",
                    &format!("{label}_management_role"),
                    "role_definition_resource_id",
                ]),
            ),
            attr(
                "principal_id",
                expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]),
            ),
        ],
    ));
}

fn emit_service_principal_fallback_assignment(fragment: &mut TfFragment, label: &str) {
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{label}_management_sp_assignment"),
        [
            attr(
                "count",
                expr::raw("var.azure_management_principal_id == \"\" ? 0 : 1"),
            ),
            attr(
                "name",
                expr::raw("uuidv5(\"oid\", \"alien:azure:mgmt-role-assign:${var.stack_name}:sp\")"),
            ),
            attr(
                "scope",
                expr::raw(
                    "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"",
                ),
            ),
            attr(
                "role_definition_id",
                expr::traversal([
                    "azurerm_role_definition",
                    &format!("{label}_management_role"),
                    "role_definition_resource_id",
                ]),
            ),
            attr("principal_id", expr::raw("var.azure_management_principal_id")),
        ],
    ));
}

fn emit_acr_push_assignment(fragment: &mut TfFragment, label: &str) {
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{label}_acr_push_assignment"),
        [
            attr(
                "name",
                expr::raw("uuidv5(\"oid\", \"alien:azure:mgmt-acr-assign:${var.stack_name}\")"),
            ),
            attr("scope", expr::raw("\"/subscriptions/${var.azure_subscription_id}\"")),
            attr(
                "role_definition_id",
                expr::raw("\"/subscriptions/${var.azure_subscription_id}/providers/Microsoft.Authorization/roleDefinitions/8311e382-0749-4cb8-b61a-304f252e45ec\""),
            ),
            attr(
                "principal_id",
                expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]),
            ),
        ],
    ));
}

fn combined_provision_permissions(
    label: &str,
    global_refs: &[&PermissionSetReference],
) -> Result<(Vec<String>, Vec<String>)> {
    let mut actions = Vec::new();
    let mut data_actions = Vec::new();
    let context = permission_context(label);
    let generator = AzureRuntimePermissionsGenerator::new();
    for permission_set in global_refs
        .iter()
        .filter_map(resolve_provision_permission_set)
    {
        let role_definition = generator
            .generate_role_definition(&permission_set, BindingTarget::Stack, &context)
            .context(ErrorData::GenericError {
                message: format!(
                    "failed to generate Azure management role permissions for '{}'",
                    permission_set.id
                ),
            })?;
        actions.extend(role_definition.actions);
        data_actions.extend(role_definition.data_actions);
    }

    actions.sort();
    actions.dedup();
    data_actions.sort();
    data_actions.dedup();
    Ok((actions, data_actions))
}

fn resolve_provision_permission_set(reference: &&PermissionSetReference) -> Option<PermissionSet> {
    if !reference.id().ends_with("/provision") {
        return None;
    }
    reference.resolve(|name| alien_permissions::get_permission_set(name).cloned())
}

fn needs_acr_push_assignment(global_refs: &[&PermissionSetReference]) -> bool {
    global_refs.iter().any(|reference| {
        let id = reference.id();
        id.starts_with("artifact-registry/")
            || id.starts_with("worker/")
            || id.starts_with("compute-cluster/")
    })
}

fn string_array(items: Vec<String>) -> Expression {
    Expression::Array(items.into_iter().map(Expression::String).collect())
}
