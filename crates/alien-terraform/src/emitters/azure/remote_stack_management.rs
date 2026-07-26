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
    emitters::enabled,
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
        // The role definition stays ungated: a definition nothing is assigned
        // to grants nothing, so leaving it is the same unbound shell a shared
        // custom role leaves. The assignment is the grant, and that is what
        // follows the gate.
        //
        // What that gate can and cannot express: these grants render at
        // resource-group scope, merged across every resource that asked for
        // one, so the assignment can only go away when the LAST contributor is
        // declined. Declining one worker while a sibling stays enabled leaves
        // the grant — and its resource-group reach — in place. Revoking
        // per-worker needs per-resource assignments, which the permission set
        // already describes but this emitter does not yet render.
        emit_management_role(&mut fragment, label, &grant_plan.plan);
        let merged_gates = merged_grant_gates(ctx, &resource_scoped_refs);
        emit_management_assignments(&mut fragment, label, &grant_plan, merged_gates.as_deref())?;
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

/// Global refs, plus resource-scoped refs paired with the resource that asked
/// for them — the pairing is what lets a grant follow that resource's gate.
fn management_permission_refs(
    profile: &PermissionProfile,
) -> (
    Vec<&PermissionSetReference>,
    Vec<(&String, &PermissionSetReference)>,
) {
    let global_refs = global_permission_refs(profile);
    let resource_scoped_refs = profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .flat_map(|(resource_id, refs)| refs.iter().map(move |r| (resource_id, r)))
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
    grants: &ManagementGrants,
    merged_gates: Option<&[String]>,
) -> Result<()> {
    let mut seen_assignments = BTreeSet::new();
    for (binding_index, binding) in grants.plan.bindings.iter().enumerate() {
        let assignment_key = management_assignment_key(binding);
        if !seen_assignments.insert(assignment_key.clone()) {
            continue;
        }

        let role_definition_id = management_role_definition_id(label, &binding.role_definition);
        let assignment_name = format!(
            "deployment:azure:mgmt-role-assign:${{local.resource_prefix}}:uami:{binding_index}"
        );
        let block = resource_block(
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
        );
        // A grant a global permission set also asks for is unconditional; only
        // one owed purely to gated resources follows their gates.
        let gates = match merged_gates {
            Some(gates) if !grants.unconditional_bindings.contains(&assignment_key) => gates,
            _ => &[],
        };
        fragment.push_gated_resource(block, gates);
    }
    Ok(())
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

/// Gates for a merged management grant, or `None` when it must stay
/// unconditional.
///
/// Azure folds every resource-scoped management permission set into one
/// stack-scoped assignment, so the grant belongs to all of its contributors at
/// once. It can only be gated when every contributor is gated — one ungated
/// contributor needs the grant unconditionally, and declining a sibling must
/// not take it away.
fn merged_grant_gates(
    ctx: &EmitContext<'_>,
    resource_scoped_refs: &[(&String, &PermissionSetReference)],
) -> Option<Vec<String>> {
    let mut gates: Vec<String> = Vec::new();
    for (resource_id, permission_set_ref) in resource_scoped_refs {
        // The same filters the plan applies, so the two cannot disagree about
        // who contributed: a set that renders nothing must not get a vote on
        // the gate either.
        let Some(permission_set) = resolve_stack_management_permission_set(permission_set_ref)
        else {
            continue;
        };
        if permission_set.platforms.azure.is_none() {
            continue;
        }
        let input_id = ctx
            .stack
            .resources
            .get(resource_id.as_str())
            .and_then(|entry| entry.enabled_when.clone())?;
        if !gates.contains(&input_id) {
            gates.push(input_id);
        }
    }
    // Stable order: the profile's iteration order would otherwise reshuffle
    // the rendered condition when a stack author reorders resources.
    gates.sort();
    (!gates.is_empty()).then_some(gates)
}

fn generate_management_grant_plan(
    label: &str,
    global_refs: &[&PermissionSetReference],
    resource_scoped_refs: &[(&String, &PermissionSetReference)],
) -> Result<ManagementGrants> {
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
    // Anything a global permission set already grants is unconditional, so a
    // gated resource asking for the same grant cannot make it conditional.
    let unconditional_bindings: BTreeSet<String> =
        bindings.iter().map(management_assignment_key).collect();

    let mut seen_stack_management_refs = BTreeSet::new();
    for permission_set in resource_scoped_refs
        .iter()
        .map(|(_resource_id, permission_set_ref)| permission_set_ref)
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

    Ok(ManagementGrants {
        plan: AzureGrantPlan {
            custom_roles,
            bindings: dedupe_azure_role_bindings(bindings),
        },
        unconditional_bindings,
    })
}

/// The management grant plan plus which of its assignments a global permission
/// set already made unconditional.
struct ManagementGrants {
    plan: AzureGrantPlan,
    unconditional_bindings: BTreeSet<String>,
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
