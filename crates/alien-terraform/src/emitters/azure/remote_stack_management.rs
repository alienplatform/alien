//! Azure management access — User-Assigned Managed Identity
//! plus Federated Identity Credential.
//!
//! Mirrors `AzureRemoteStackManagementController`:
//!
//! 1. `azurerm_user_assigned_identity` for the management identity
//!    living in the customer's subscription / resource group.
//! 2. Predefined Azure role assignments plus, when needed, one combined
//!    residual custom management role built from the materialized global
//!    management permission profile.
//! 3. Role assignments to the management identity.
//! 4. `azurerm_federated_identity_credential` trusting the manager OIDC
//!    issuer and subject.

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
use alien_permissions::{
    generators::{
        dedupe_azure_role_bindings, AzureGrantPlan, AzureRoleDefinition, AzureRoleDefinitionRef,
        AzureRuntimePermissionsGenerator,
    },
    BindingTarget,
};
use hcl::expr::Expression;
use std::collections::BTreeSet;

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
                    expr::template("${local.resource_prefix}-management-identity".to_string()),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("tags", tags(ctx, "management")),
            ],
        ));

        let (global_refs, resource_scoped_refs) = ctx
            .stack
            .management()
            .profile()
            .map(management_permission_refs)
            .unwrap_or_default();
        let grant_plan =
            generate_management_grant_plan(label, &global_refs, &resource_scoped_refs)?;
        emit_management_role(&mut fragment, label, &grant_plan);
        emit_management_assignments(&mut fragment, label, &grant_plan);
        emit_existing_network_reader_assignments(&mut fragment, label);

        fragment.resource_blocks.push(resource_block(
            "azurerm_federated_identity_credential",
            &format!("{label}_fic"),
            [
                attr(
                    "count",
                    expr::raw(
                        "var.azure_oidc_issuer != \"\" && var.azure_oidc_subject != \"\" ? 1 : 0",
                    ),
                ),
                attr(
                    "name",
                    expr::template("${local.resource_prefix}-federated-credential".to_string()),
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

fn emit_existing_network_reader_assignments(fragment: &mut TfFragment, label: &str) {
    let is_existing_azure_vnet =
        "try(local.deployment_settings.network.type, \"\") == \"byo-vnet-azure\"";
    let existing_vnet_resource_id = "try(local.deployment_settings.network.vnet_resource_id, \"\")";
    let reader_role_definition_id =
        "\"/subscriptions/${var.azure_subscription_id}/providers/Microsoft.Authorization/roleDefinitions/acdd72a7-3385-48ef-bd42-f606fba81ae7\"";

    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{label}_existing_vnet_reader_uami"),
        [
            attr("count", expr::raw(format!("{is_existing_azure_vnet} ? 1 : 0"))),
            attr(
                "name",
                expr::raw(format!(
                    "uuidv5(\"oid\", \"deployment:azure:existing-vnet-reader:${{local.resource_prefix}}:uami:${{azurerm_user_assigned_identity.{label}.principal_id}}:${{{existing_vnet_resource_id}}}\")"
                )),
            ),
            attr("scope", expr::raw(existing_vnet_resource_id)),
            attr("role_definition_id", expr::raw(reader_role_definition_id)),
            attr(
                "principal_id",
                expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]),
            ),
        ],
    ));

    // The management service principal is shared across deployments. Azure role
    // assignments are unique by (principal, role, scope), so a package cannot
    // safely own that shared VNet grant per deployment.
}

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}

fn management_permission_refs(
    profile: &PermissionProfile,
) -> (Vec<&PermissionSetReference>, Vec<&PermissionSetReference>) {
    let global_refs = global_permission_refs(profile);
    let resource_scoped_refs = profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .flat_map(|(_, refs)| refs.iter())
        .collect();
    (global_refs, resource_scoped_refs)
}

fn emit_management_role(fragment: &mut TfFragment, label: &str, grant_plan: &AzureGrantPlan) {
    let Some(role_definition) = combined_management_role_definition(label, grant_plan) else {
        return;
    };

    fragment.resource_blocks.push(resource_block(
        "azurerm_role_definition",
        &format!("{label}_management_role"),
        [
            attr("name", expr::template(role_definition.name.clone())),
            attr(
                "role_definition_id",
                expr::raw(
                    "uuidv5(\"oid\", \"deployment:azure:mgmt-role-def:${local.resource_prefix}\")",
                ),
            ),
            attr("scope", management_role_definition_scope(&role_definition)),
            attr(
                "description",
                Expression::String(role_definition.description),
            ),
            nested(block(
                "permissions",
                [
                    attr("actions", string_array(role_definition.actions)),
                    attr("data_actions", string_array(role_definition.data_actions)),
                    attr("not_actions", Expression::Array(Vec::new())),
                    attr("not_data_actions", Expression::Array(Vec::new())),
                ],
            )),
            attr(
                "assignable_scopes",
                Expression::Array(
                    role_definition
                        .assignable_scopes
                        .into_iter()
                        .map(expr::template)
                        .collect(),
                ),
            ),
        ],
    ));
}

fn emit_management_assignments(
    fragment: &mut TfFragment,
    label: &str,
    grant_plan: &AzureGrantPlan,
) {
    let mut seen_assignments = BTreeSet::new();
    for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
        if !seen_assignments.insert(management_assignment_key(binding)) {
            continue;
        }

        let role_definition_id = management_role_definition_id(label, &binding.role_definition);
        let assignment_name = format!(
            "deployment:azure:mgmt-role-assign:${{local.resource_prefix}}:uami:{binding_index}"
        );
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_management_uami_assignment_{binding_index}"),
            [
                attr(
                    "name",
                    expr::raw(&format!("uuidv5(\"oid\", \"{assignment_name}\")")),
                ),
                attr("scope", expr::template(binding.scope.clone())),
                attr("role_definition_id", role_definition_id.clone()),
                attr(
                    "principal_id",
                    expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]),
                ),
            ],
        ));
    }
}

fn management_assignment_key(binding: &alien_permissions::generators::AzureRoleBinding) -> String {
    let role_key = match &binding.role_definition {
        AzureRoleDefinitionRef::Predefined { role_definition_id } => {
            format!("predefined:{role_definition_id}")
        }
        AzureRoleDefinitionRef::Custom { .. } => "combined-custom-management-role".to_string(),
    };
    format!("{}:{role_key}", binding.scope)
}

fn generate_management_grant_plan(
    label: &str,
    global_refs: &[&PermissionSetReference],
    resource_scoped_refs: &[&PermissionSetReference],
) -> Result<AzureGrantPlan> {
    let mut custom_roles = Vec::new();
    let mut bindings = Vec::new();
    let context = permission_context(label);
    let generator = AzureRuntimePermissionsGenerator::new();

    for permission_set in global_refs.iter().filter_map(resolve_permission_set) {
        if permission_set.platforms.azure.is_none() {
            continue;
        }

        let grant_plan = generator
            .generate_grant_plan(&permission_set, BindingTarget::Stack, &context)
            .context(ErrorData::GenericError {
                message: format!(
                    "failed to generate Azure management grant plan for '{}'",
                    permission_set.id
                ),
            })?;
        custom_roles.extend(grant_plan.custom_roles);
        bindings.extend(grant_plan.bindings);
    }

    let mut seen_stack_management_refs = BTreeSet::new();
    for permission_set in resource_scoped_refs
        .iter()
        .filter_map(resolve_stack_management_permission_set)
    {
        if !seen_stack_management_refs.insert(permission_set.id.clone()) {
            continue;
        }
        if permission_set.platforms.azure.is_none() {
            continue;
        }

        let grant_plan = generator
            .generate_grant_plan(&permission_set, BindingTarget::Stack, &context)
            .context(ErrorData::GenericError {
                message: format!(
                    "failed to generate Azure management grant plan for '{}'",
                    permission_set.id
                ),
            })?;
        custom_roles.extend(grant_plan.custom_roles);
        bindings.extend(grant_plan.bindings);
    }

    Ok(AzureGrantPlan {
        custom_roles,
        bindings: dedupe_azure_role_bindings(bindings),
    })
}

fn combined_management_role_definition(
    label: &str,
    grant_plan: &AzureGrantPlan,
) -> Option<AzureRoleDefinition> {
    if grant_plan.custom_roles.is_empty() {
        return None;
    }

    let mut actions = Vec::new();
    let mut data_actions = Vec::new();
    let mut assignable_scopes = Vec::new();

    for custom_role in &grant_plan.custom_roles {
        actions.extend(custom_role.role_definition.actions.clone());
        data_actions.extend(custom_role.role_definition.data_actions.clone());
        assignable_scopes.extend(custom_role.role_definition.assignable_scopes.clone());
    }

    actions.sort();
    actions.dedup();
    data_actions.sort();
    data_actions.dedup();
    assignable_scopes.sort();
    assignable_scopes.dedup();

    Some(AzureRoleDefinition {
        name: "${local.resource_prefix}-management-role".to_string(),
        id: None,
        is_custom: true,
        description: format!("Management role for Terraform resource '{label}'"),
        actions,
        not_actions: vec![],
        data_actions,
        not_data_actions: vec![],
        assignable_scopes,
    })
}

fn resolve_permission_set(reference: &&PermissionSetReference) -> Option<PermissionSet> {
    reference.resolve(|name| alien_permissions::get_permission_set(name).cloned())
}

fn resolve_stack_management_permission_set(
    reference: &&PermissionSetReference,
) -> Option<PermissionSet> {
    match reference.id() {
        "worker/dispatch-command" => {
            reference.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        }
        _ => None,
    }
}

fn management_role_definition_id(label: &str, role_ref: &AzureRoleDefinitionRef) -> Expression {
    match role_ref {
        AzureRoleDefinitionRef::Predefined { role_definition_id } => {
            expr::template(role_definition_id.clone())
        }
        AzureRoleDefinitionRef::Custom { .. } => expr::traversal([
            "azurerm_role_definition",
            &format!("{label}_management_role"),
            "role_definition_resource_id",
        ]),
    }
}

fn management_role_definition_scope(role_definition: &AzureRoleDefinition) -> Expression {
    if role_definition
        .assignable_scopes
        .iter()
        .any(|scope| scope == "/subscriptions/${var.azure_subscription_id}")
    {
        expr::raw("\"/subscriptions/${var.azure_subscription_id}\"")
    } else {
        expr::raw(
            "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"",
        )
    }
}

fn string_array(items: Vec<String>) -> Expression {
    Expression::Array(items.into_iter().map(Expression::String).collect())
}
