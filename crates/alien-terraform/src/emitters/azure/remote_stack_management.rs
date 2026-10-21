//! Azure management access — User-Assigned Managed Identity
//! plus Federated Identity Credential.
//!
//! Mirrors `AzureRemoteStackManagementController`:
//!
//! 1. `azurerm_user_assigned_identity` for the management identity
//!    living in the customer's subscription / resource group.
//! 2. Predefined Azure role assignments plus, when needed, residual custom
//!    management roles built from the materialized global management
//!    permission profile: one per scope the grants bind at, so a
//!    resource-group grant is never assigned at the subscription.
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
    import::EmitContext, ErrorData, NetworkSettings, PermissionProfile, PermissionSet,
    PermissionSetReference, RemoteStackManagement, ResourceLifecycle, Result,
};
use alien_error::Context;
use alien_permissions::{
    generators::{
        azure_role_assignment_delegation_condition, dedupe_azure_role_bindings,
        management_roles_by_scope, protected_management_assignable_scopes, AzureGrantPlan,
        AzureManagementRoles, AzureManagementScopeKind, AzureRoleDefinitionRef,
        AzureRuntimePermissionsGenerator, AzureScopedCustomRole,
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

        // The management heartbeat reads its own identity even when the setup
        // imports bindings only and grants no resource-group-wide Reader role.
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_self_read"),
            [
                attr(
                    "scope",
                    expr::traversal(["azurerm_user_assigned_identity", label, "id"]),
                ),
                attr(
                    "role_definition_name",
                    Expression::String("Reader".to_string()),
                ),
                attr(
                    "principal_id",
                    expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]),
                ),
            ],
        ));

        let (global_refs, resource_scoped_refs) = ctx
            .stack
            .management()
            .profile()
            .map(|profile| management_permission_refs(ctx, profile))
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
        let roles =
            management_roles_by_scope(&grant_plan.plan).context(ErrorData::GenericError {
                message: "failed to group Azure management grants by scope".to_string(),
            })?;
        emit_management_roles(&mut fragment, label, &roles);
        let merged_gates = merged_grant_gates(ctx, &resource_scoped_refs);
        emit_management_assignments(
            &mut fragment,
            label,
            &grant_plan,
            &roles,
            merged_gates.as_deref(),
        )?;
        // A bindings-only setup has no Live resource that uses the selected
        // application network. Emitting this grant there is both unnecessary
        // privilege and invalid because setup-only registration deliberately
        // omits application network settings.
        let setup_only =
            ctx.stack.resources.values().any(|entry| {
                alien_core::remote_bindings::remote_binding_for_entry(entry).is_some()
            }) && !ctx
                .stack
                .resources
                .values()
                .any(|entry| entry.lifecycle == ResourceLifecycle::Live);
        if !setup_only
            && matches!(
                ctx.stack_settings.network.as_ref(),
                Some(NetworkSettings::ByoVnetAzure { .. })
            )
        {
            emit_existing_network_reader_assignments(&mut fragment, label);
        }

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
        let fields = vec![
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
        ];
        Ok(expr::object(fields))
    }
}

fn emit_existing_network_reader_assignments(fragment: &mut TfFragment, label: &str) {
    let is_existing_azure_vnet =
        "try(local.deployment_settings.network.type, \"\") == \"byo-vnet-azure\"";
    let existing_vnet_resource_id = "try(local.deployment_settings.network.vnet_resource_id, \"\")";
    let reader_role_definition_id = "\"/subscriptions/${var.azure_subscription_id}/providers/Microsoft.Authorization/roleDefinitions/acdd72a7-3385-48ef-bd42-f606fba81ae7\"";

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

fn global_permission_refs<'a>(
    ctx: &EmitContext<'_>,
    profile: &'a PermissionProfile,
) -> Vec<&'a PermissionSetReference> {
    alien_permissions::management_identity_global_refs(ctx.stack.resources.values(), profile)
}

/// Global refs, plus resource-scoped refs paired with the resource that asked
/// for them — the pairing is what lets a grant follow that resource's gate.
fn management_permission_refs<'a>(
    ctx: &EmitContext<'_>,
    profile: &'a PermissionProfile,
) -> (
    Vec<&'a PermissionSetReference>,
    Vec<(&'a String, &'a PermissionSetReference)>,
) {
    let global_refs = global_permission_refs(ctx, profile);
    let resource_scoped_refs = profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .flat_map(|(resource_id, refs)| refs.iter().map(move |r| (resource_id, r)))
        .collect();
    (global_refs, resource_scoped_refs)
}

fn emit_management_roles(fragment: &mut TfFragment, label: &str, roles: &AzureManagementRoles) {
    for role in &roles.roles {
        emit_management_role(fragment, label, role, ManagementRoleKind::Management);
    }
    for role in &roles.role_assignment_roles {
        emit_management_role(fragment, label, role, ManagementRoleKind::RoleAssignments);
    }
}

/// A management role holds either the identity's grants at a scope, or only its
/// role-assignment writes there, which are assigned with a condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagementRoleKind {
    Management,
    RoleAssignments,
}

impl ManagementRoleKind {
    fn name_part(self) -> &'static str {
        match self {
            Self::Management => "",
            Self::RoleAssignments => "-role-assignments",
        }
    }

    fn address_part(self) -> &'static str {
        match self {
            Self::Management => "",
            Self::RoleAssignments => "_role_assignments",
        }
    }

    fn id_part(self) -> &'static str {
        match self {
            Self::Management => "mgmt-role-def",
            Self::RoleAssignments => "mgmt-role-assignments-role-def",
        }
    }
}

fn emit_management_role(
    fragment: &mut TfFragment,
    label: &str,
    role: &AzureScopedCustomRole,
    role_kind: ManagementRoleKind,
) {
    let kind = role.scope_kind.as_str();
    let part = role_kind.name_part();
    let name = match scope_suffix(&role.scope) {
        None => format!("${{local.resource_prefix}}-management{part}-{kind}-role"),
        Some(suffix) => format!(
            "${{local.resource_prefix}}-management{part}-{kind}-role-{}",
            suffix
        ),
    };
    let description = match role_kind {
        ManagementRoleKind::Management => {
            format!("Management role for Terraform resource '{label}'. Scope: {kind}.")
        }
        ManagementRoleKind::RoleAssignments => format!(
            "Role assignments the management identity of Terraform resource '{label}' makes for workloads, never for itself. Scope: {kind}."
        ),
    };
    let assignable_scopes =
        protected_management_assignable_scopes(&role.scope, OWN_SUBSCRIPTION_SCOPE)
            .into_iter()
            .map(expr::template)
            .collect();
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_definition",
        &management_role_address(label, &role.scope, role.scope_kind, role_kind),
        [
            attr("name", expr::template(name)),
            attr(
                "role_definition_id",
                expr::raw(format!(
                    "uuidv5(\"oid\", \"deployment:azure:{}:${{local.resource_prefix}}:{}\")",
                    role_kind.id_part(),
                    role.scope
                )),
            ),
            attr("scope", expr::template(role.scope.clone())),
            attr("description", Expression::String(description)),
            nested(block(
                "permissions",
                [
                    attr("actions", string_array(role.actions.clone())),
                    attr("data_actions", string_array(role.data_actions.clone())),
                    attr("not_actions", Expression::Array(Vec::new())),
                    attr("not_data_actions", Expression::Array(Vec::new())),
                ],
            )),
            attr("assignable_scopes", Expression::Array(assignable_scopes)),
        ],
    ));
}

const OWN_SUBSCRIPTION_SCOPE: &str = "/subscriptions/${var.azure_subscription_id}";
const OWN_RESOURCE_GROUP_SCOPE: &str =
    "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}";

/// `None` for the deployment's own subscription and resource group. Any other scope (an inline
/// permission set may name another resource group) gets a short, stable hash of the full scope,
/// so its role has its own address and Azure name even when two group names differ only in
/// punctuation.
fn scope_suffix(scope: &str) -> Option<String> {
    if scope == OWN_SUBSCRIPTION_SCOPE || scope == OWN_RESOURCE_GROUP_SCOPE {
        return None;
    }
    // FNV-1a: stable across Rust versions and platforms, unlike the std hasher.
    let hash = scope.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    Some(format!("{:08x}", hash >> 32))
}

fn management_role_address(
    label: &str,
    scope: &str,
    kind: AzureManagementScopeKind,
    role_kind: ManagementRoleKind,
) -> String {
    let part = role_kind.address_part();
    let base = match kind {
        AzureManagementScopeKind::Subscription => {
            format!("{label}_management{part}_subscription_role")
        }
        AzureManagementScopeKind::ResourceGroup => {
            format!("{label}_management{part}_resource_group_role")
        }
    };
    match scope_suffix(scope) {
        None => base,
        Some(suffix) => format!("{base}_{suffix}"),
    }
}

fn emit_management_assignments(
    fragment: &mut TfFragment,
    label: &str,
    grants: &ManagementGrants,
    roles: &AzureManagementRoles,
    merged_gates: Option<&[String]>,
) -> Result<()> {
    let has_role =
        |list: &[AzureScopedCustomRole], scope: &str| list.iter().any(|role| role.scope == scope);
    let mut seen_assignments = BTreeSet::new();
    for (binding_index, binding) in grants.plan.bindings.iter().enumerate() {
        let assignment_key = management_assignment_key(binding);
        if !seen_assignments.insert(assignment_key.clone()) {
            continue;
        }
        // A grant a global permission set also asks for is unconditional; only
        // one owed purely to gated resources follows their gates.
        let gates = match merged_gates {
            Some(gates) if !grants.unconditional_bindings.contains(&assignment_key) => gates,
            _ => &[],
        };
        let principal_id =
            expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]);
        let custom = matches!(
            binding.role_definition,
            AzureRoleDefinitionRef::Custom { .. }
        );
        if !custom || has_role(&roles.roles, &binding.scope) {
            let role_definition_id = management_role_definition_id(
                label,
                &binding.role_definition,
                &binding.scope,
                ManagementRoleKind::Management,
            )?;
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
                    attr("role_definition_id", role_definition_id),
                    attr("principal_id", principal_id.clone()),
                ],
            );
            fragment.push_gated_resource(block, gates);
        }
        if custom && has_role(&roles.role_assignment_roles, &binding.scope) {
            let role_definition_id = management_role_definition_id(
                label,
                &binding.role_definition,
                &binding.scope,
                ManagementRoleKind::RoleAssignments,
            )?;
            let assignment_name = format!(
                "deployment:azure:mgmt-role-assignments-assign:${{local.resource_prefix}}:{}",
                binding.scope
            );
            let condition = azure_role_assignment_delegation_condition(&format!(
                "${{azurerm_user_assigned_identity.{label}.principal_id}}"
            ));
            let block = resource_block(
                "azurerm_role_assignment",
                &format!("{label}_management_role_assignments_assignment_{binding_index}"),
                [
                    attr(
                        "name",
                        expr::raw(&format!("uuidv5(\"oid\", \"{assignment_name}\")")),
                    ),
                    attr("scope", expr::template(binding.scope.clone())),
                    attr("role_definition_id", role_definition_id),
                    attr("principal_id", principal_id),
                    attr("condition", expr::template(condition)),
                    attr("condition_version", Expression::String("2.0".to_string())),
                ],
            );
            fragment.push_gated_resource(block, gates);
        }
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

fn resolve_permission_set(reference: &&PermissionSetReference) -> Option<PermissionSet> {
    reference.resolve(|name| alien_permissions::get_permission_set(name).cloned())
}

/// The one set compiled at stack scope onto the management identity.
///
/// Matched on a name from the built-in registry, never on `id()`: an inline set carries whatever
/// id its author typed, so deciding by id would let one named after this set compile here.
fn resolve_stack_management_permission_set(
    reference: &&PermissionSetReference,
) -> Option<PermissionSet> {
    match reference {
        PermissionSetReference::Name(name) if name == "worker/dispatch-command" => {
            alien_permissions::get_permission_set(name).cloned()
        }
        _ => None,
    }
}

fn management_role_definition_id(
    label: &str,
    role_ref: &AzureRoleDefinitionRef,
    scope: &str,
    role_kind: ManagementRoleKind,
) -> Result<Expression> {
    match role_ref {
        AzureRoleDefinitionRef::Predefined { role_definition_id } => {
            Ok(expr::template(role_definition_id.clone()))
        }
        AzureRoleDefinitionRef::Custom { .. } => {
            let kind = AzureManagementScopeKind::of(scope).ok_or_else(|| {
                alien_error::AlienError::new(ErrorData::GenericError {
                    message: format!(
                        "custom management grant bound at '{scope}': stack-level grants bind at the subscription or the deployment resource group"
                    ),
                })
            })?;
            Ok(expr::traversal([
                "azurerm_role_definition",
                &management_role_address(label, scope, kind, role_kind),
                "role_definition_resource_id",
            ]))
        }
    }
}

fn string_array(items: Vec<String>) -> Expression {
    Expression::Array(items.into_iter().map(Expression::String).collect())
}
