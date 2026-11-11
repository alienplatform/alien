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
        azure_role_assignment_allowlist_conditions, azure_runs_no_code, azure_runtime_roles_for,
        azure_write_or_delete, dedupe_azure_role_bindings, management_roles_by_scope,
        protected_management_assignable_scopes, AzureGrantPlan, AzureManagementRoles,
        AzureManagementScopeKind, AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator,
        AzureRuntimeRoleHolder, AzureRuntimeRoles, AzureScopedCustomRole, AzureWorkloadPrincipals,
        AZURE_REMOTE_BINDINGS_PROFILE,
    },
    BindingTarget,
};
use hcl::expr::Expression;
use std::collections::BTreeSet;
use uuid::Uuid;

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
            generate_management_grant_plan(ctx, label, &global_refs, &resource_scoped_refs)?;
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
        let mut roles =
            management_roles_by_scope(&grant_plan.plan).context(ErrorData::GenericError {
                message: "failed to group Azure management grants by scope".to_string(),
            })?;
        // The roles the runtime assigns after setup, and the conditions that limit management's
        // role-assignment writes to them. Mirrors `AzureRemoteStackManagementController`.
        let runtime_roles =
            azure_runtime_roles_for(ctx.stack, ctx.stack_settings.signing.is_some()).context(
                ErrorData::GenericError {
                    message: "failed to list the Azure roles the runtime assigns".to_string(),
                },
            )?;
        let conditions = runtime_role_conditions(ctx, &mut fragment, label, &runtime_roles)?;
        // The runtime assigns those roles even when the stack grants management no
        // role-assignment writes itself; the conditions make the role safe to add.
        if !runtime_roles.is_empty()
            && !roles
                .role_assignment_roles
                .iter()
                .any(|role| role.scope == OWN_RESOURCE_GROUP_SCOPE)
        {
            roles.role_assignment_roles.push(AzureScopedCustomRole {
                scope: OWN_RESOURCE_GROUP_SCOPE.to_string(),
                scope_kind: AzureManagementScopeKind::ResourceGroup,
                actions: vec![
                    "Microsoft.Authorization/roleAssignments/delete".to_string(),
                    "Microsoft.Authorization/roleAssignments/write".to_string(),
                ],
                data_actions: Vec::new(),
                not_actions: Vec::new(),
            });
        }
        emit_management_roles(&mut fragment, label, &roles, conditions.len());
        if ctx.stack_settings.signing.is_some() {
            emit_protection(ctx, &mut fragment, label, &roles)?;
        }
        let merged_gates = merged_grant_gates(ctx, &resource_scoped_refs);
        emit_management_assignments(
            &mut fragment,
            label,
            &grant_plan,
            &roles,
            merged_gates.as_deref(),
        )?;
        emit_role_assignment_role_assignments(&mut fragment, label, &roles, &conditions);
        emit_remote_bindings_runtime_roles(&mut fragment, &runtime_roles);
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

/// Deployment stack deny settings hold at most this many excluded actions.
const DENY_SETTINGS_MAX_EXCLUDED_ACTIONS: usize = 200;

/// Protect an install's resource group: a subscription-scoped deployment stack manages the group
/// and denies writes and deletes of the group itself (its tags hold the signing settings) to
/// every principal but two: the verifier and the principal running setup, so setup can run
/// again. Removing the deny needs `Microsoft.Resources/deploymentStacks/manageDenySetting/action`
/// at the subscription, which the management identity never holds.
///
/// Azure doesn't carry this deny to the resources in the group (the deny assignment it creates
/// says `doNotApplyToChildScopes`, whatever the stack asks; seen on real Azure), so the
/// management roles themselves leave out every write and delete of what runs code, Key Vault
/// writes and the actions that run code (see `alien_permissions::generators::verifier_owned`).
///
/// To check a change here against Azure, run `scripts/azure-setup-locks/check.py`.
fn emit_protection(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
    roles: &AzureManagementRoles,
) -> Result<()> {
    let resource_group = ctx
        .stack
        .resources()
        .find(|(_, entry)| {
            entry.config.resource_type() == alien_core::AzureResourceGroup::RESOURCE_TYPE
        })
        .and_then(|(resource_id, _)| ctx.name_for(resource_id))
        .ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::OperationNotSupported {
                operation: "protect an Azure deployment whose resource group setup does not create"
                    .to_string(),
                reason: "the deny settings are held by a deployment stack that manages the group"
                    .to_string(),
            })
        })?;
    let excluded_actions = deny_excluded_actions(roles)?;
    let verifier = format!("{label}_verifier");
    fragment.resource_blocks.push(resource_block(
        "azurerm_user_assigned_identity",
        &verifier,
        [
            attr(
                "name",
                expr::template("${local.resource_prefix}-verifier".to_string()),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr("tags", tags(ctx, "verifier")),
        ],
    ));
    emit_verifier_grants(fragment, label, &verifier, resource_group)?;
    let group =
        |attribute: &str| expr::traversal(["azurerm_resource_group", resource_group, attribute]);
    let template = expr::object([
        (
            "$schema",
            Expression::String(
                "https://schema.management.azure.com/schemas/2018-05-01/subscriptionDeploymentTemplate.json#"
                    .to_string(),
            ),
        ),
        ("contentVersion", Expression::String("1.0.0.0".to_string())),
        (
            "resources",
            Expression::Array(vec![expr::object([
                (
                    "type",
                    Expression::String("Microsoft.Resources/resourceGroups".to_string()),
                ),
                ("apiVersion", Expression::String("2022-09-01".to_string())),
                ("name", group("name")),
                ("location", group("location")),
                ("tags", group("tags")),
            ])]),
        ),
    ]);
    let detach = Expression::String("detach".to_string());
    fragment.resource_blocks.push(resource_block(
        "azapi_resource",
        &format!("{label}_protection"),
        [
            attr(
                "type",
                Expression::String("Microsoft.Resources/deploymentStacks@2024-03-01".to_string()),
            ),
            attr(
                "name",
                expr::template("${local.resource_prefix}-protection".to_string()),
            ),
            attr(
                "parent_id",
                expr::template("/subscriptions/${var.azure_subscription_id}".to_string()),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr(
                "body",
                expr::object([(
                    "properties",
                    expr::object([
                        (
                            "description",
                            Expression::String(
                                "Only the verifier writes what runs code in this install."
                                    .to_string(),
                            ),
                        ),
                        (
                            "actionOnUnmanage",
                            expr::object([
                                ("resources", detach.clone()),
                                ("resourceGroups", detach.clone()),
                                ("managementGroups", detach),
                            ]),
                        ),
                        (
                            "denySettings",
                            expr::object([
                                ("mode", Expression::String("denyWriteAndDelete".to_string())),
                                ("applyToChildScopes", Expression::Bool(true)),
                                (
                                    "excludedPrincipals",
                                    Expression::Array(vec![
                                        expr::traversal([
                                            "azurerm_user_assigned_identity",
                                            &verifier,
                                            "principal_id",
                                        ]),
                                        expr::traversal([
                                            "data",
                                            "azurerm_client_config",
                                            &format!("{label}_current"),
                                            "object_id",
                                        ]),
                                    ]),
                                ),
                                ("excludedActions", string_array(excluded_actions)),
                            ]),
                        ),
                        ("template", template),
                    ]),
                )]),
            ),
        ],
    ));
    Ok(())
}

/// What the verifier may do in the install's group (byoc review §1). No `runCommand` and no
/// `runCommands/write`: those run a script on a machine outside what the release signed.
const AZURE_VERIFIER_ACTIONS: &[&str] = &[
    "Microsoft.App/containerApps/read",
    "Microsoft.App/containerApps/write",
    "Microsoft.App/containerApps/delete",
    "Microsoft.App/containerApps/revisions/activate/action",
    "Microsoft.App/containerApps/revisions/deactivate/action",
    "Microsoft.App/containerApps/revisions/restart/action",
    "Microsoft.App/jobs/read",
    "Microsoft.App/jobs/write",
    "Microsoft.App/jobs/delete",
    "Microsoft.App/jobs/start/action",
    // It finds the one Container Apps environment of the group to create a workload's app in.
    "Microsoft.App/managedEnvironments/read",
    "Microsoft.App/managedEnvironments/join/action",
    "Microsoft.Compute/virtualMachineScaleSets/read",
    "Microsoft.Compute/virtualMachineScaleSets/write",
    "Microsoft.Compute/virtualMachineScaleSets/delete",
    "Microsoft.Compute/virtualMachineScaleSets/delete/action",
    "Microsoft.Compute/virtualMachineScaleSets/manualupgrade/action",
    "Microsoft.Compute/virtualMachineScaleSets/reimage/action",
    "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/read",
    "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/write",
    "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/delete",
    "Microsoft.Compute/virtualMachineScaleSets/extensions/read",
    "Microsoft.Compute/virtualMachineScaleSets/extensions/write",
    "Microsoft.Compute/virtualMachineScaleSets/extensions/delete",
    "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
    "Microsoft.Network/virtualNetworks/subnets/join/action",
    "Microsoft.Network/networkSecurityGroups/join/action",
    "Microsoft.Resources/subscriptions/resourceGroups/read",
    // The deployer secrets vault's name ends in a random suffix, so it finds the vault in the
    // group to check the binding a worker is given for it.
    "Microsoft.KeyVault/vaults/read",
    // It creates and updates the blob containers of the install's storages, whose public access
    // it checks against the signed stack.
    "Microsoft.Storage/storageAccounts/blobServices/containers/read",
    "Microsoft.Storage/storageAccounts/blobServices/containers/write",
];

/// What the verifier reads outside the group: the publisher's community gallery images its
/// scale sets boot. Reads only; granted at the subscription because gallery images live there.
const AZURE_VERIFIER_IMAGE_ACTIONS: &[&str] = &[
    "Microsoft.Compute/locations/communityGalleries/images/read",
    "Microsoft.Compute/locations/communityGalleries/images/versions/read",
];

/// What follows the install's group name in the name of the group that holds the signing vault.
/// Must equal `alien_signing::settings::names::azure_signing_resource_group`.
const SIGNING_GROUP_SUFFIX: &str = "-signing";

/// The Terraform label of the group that holds the signing vault, for management label `label`.
fn signing_group_label(label: &str) -> String {
    format!("{label}_signing_group")
}

/// Built-in role that reads and writes Key Vault secrets.
const KEY_VAULT_SECRETS_OFFICER: &str = "b86a8fe4-44ce-4948-aee5-eccb2c155cd7";

/// What a protected install's machines read, in the role `<prefix>-machine-signing`: the
/// group's `signing-fingerprint` tag, the vaults in the group (the signing vault's name ends in
/// a random suffix, so machines list them) and the identities (to resolve a workload's client id).
pub const AZURE_MACHINE_SIGNING_ACTIONS: &[&str] = &[
    "Microsoft.Resources/subscriptions/resourceGroups/read",
    "Microsoft.KeyVault/vaults/read",
    "Microsoft.ManagedIdentity/userAssignedIdentities/read",
];

/// The grants a protected install's machine identity needs to boot: `<prefix>-machine-signing`
/// on the install's group and Key Vault Secrets Officer on the signing vault (it reads the
/// ledger and inputs and writes the ledger back). Labels start with `label`. None on an
/// unprotected install.
///
/// For the emitter that creates the identity the machines run as (compute clusters render
/// through emitters a composed CLI registers): without these, protected machines refuse to boot.
pub fn azure_machine_signing_grants(
    ctx: &EmitContext<'_>,
    label: &str,
    principal_id: Expression,
) -> Result<Vec<hcl::Block>> {
    if ctx.stack_settings.signing.is_none() {
        return Ok(Vec::new());
    }
    let management = ctx
        .stack
        .resources()
        .find(|(_, entry)| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
        .and_then(|(resource_id, _)| ctx.name_for(resource_id))
        .ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::OperationNotSupported {
                operation: "grant a protected Azure install's machines their signing settings"
                    .to_string(),
                reason: "the stack has no remote management, which creates the signing vault"
                    .to_string(),
            })
        })?;
    let resource_group = ctx
        .stack
        .resources()
        .find(|(_, entry)| {
            entry.config.resource_type() == alien_core::AzureResourceGroup::RESOURCE_TYPE
        })
        .and_then(|(resource_id, _)| ctx.name_for(resource_id))
        .ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::OperationNotSupported {
                operation: "grant a protected Azure install's machines their signing settings"
                    .to_string(),
                reason: "setup does not create the install's resource group".to_string(),
            })
        })?;
    Ok(vec![
        resource_block(
            "azurerm_role_assignment",
            &format!("{label}_machine_signing"),
            [
                attr(
                    "scope",
                    expr::traversal(["azurerm_resource_group", resource_group, "id"]),
                ),
                attr(
                    "role_definition_id",
                    expr::traversal([
                        "azurerm_role_definition",
                        &format!("{management}_machine_signing"),
                        "role_definition_resource_id",
                    ]),
                ),
                attr("principal_id", principal_id.clone()),
                attr("principal_type", Expression::String("ServicePrincipal".to_string())),
            ],
        ),
        resource_block(
            "azurerm_role_assignment",
            &format!("{label}_machine_signing_vault_group"),
            [
                attr(
                    "scope",
                    expr::traversal([
                        "azurerm_resource_group",
                        &signing_group_label(management),
                        "id",
                    ]),
                ),
                attr(
                    "role_definition_id",
                    expr::traversal([
                        "azurerm_role_definition",
                        &format!("{management}_machine_signing"),
                        "role_definition_resource_id",
                    ]),
                ),
                attr("principal_id", principal_id.clone()),
                attr("principal_type", Expression::String("ServicePrincipal".to_string())),
            ],
        ),
        resource_block(
            "azurerm_role_assignment",
            &format!("{label}_signing_vault"),
            [
                attr(
                    "scope",
                    expr::traversal(["azurerm_key_vault", &format!("{management}_signing"), "id"]),
                ),
                attr(
                    "role_definition_id",
                    expr::template(format!(
                        "/subscriptions/${{var.azure_subscription_id}}/providers/Microsoft.Authorization/roleDefinitions/{KEY_VAULT_SECRETS_OFFICER}"
                    )),
                ),
                attr("principal_id", principal_id),
                attr("principal_type", Expression::String("ServicePrincipal".to_string())),
            ],
        ),
    ])
}

/// The verifier's custom role on the group, and the signing vault: a Key Vault only setup and
/// the verifier write, holding the ledger. Management holds no vault writes on a protected
/// install and no secret writes in the group (refused above).
fn emit_verifier_grants(
    fragment: &mut TfFragment,
    label: &str,
    verifier: &str,
    resource_group: &str,
) -> Result<()> {
    let group_id = expr::traversal(["azurerm_resource_group", resource_group, "id"]);
    let principal = expr::traversal(["azurerm_user_assigned_identity", verifier, "principal_id"]);
    let role = format!("{label}_verifier");
    // The signing vault's own group. Management holds role-assignment rights in the install's
    // group only, so nothing management assigns can reach the vault.
    let signing_group = signing_group_label(label);
    let signing_group_id = expr::traversal(["azurerm_resource_group", &signing_group, "id"]);
    fragment.resource_blocks.push(resource_block(
        "azurerm_resource_group",
        &signing_group,
        [
            attr(
                "name",
                expr::template(format!(
                    "${{var.azure_resource_group_name}}{SIGNING_GROUP_SUFFIX}"
                )),
            ),
            attr("location", expr::raw("var.azure_location")),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_definition",
        &role,
        [
            attr("name", expr::template("${local.resource_prefix}-verifier".to_string())),
            attr("scope", group_id.clone()),
            attr(
                "description",
                Expression::String(
                    "Applies only releases the vendor signed: the install's container apps, jobs and scale sets."
                        .to_string(),
                ),
            ),
            nested(block(
                "permissions",
                [attr("actions", string_array(
                    AZURE_VERIFIER_ACTIONS.iter().map(|action| action.to_string()).collect(),
                ))],
            )),
            attr("assignable_scopes", Expression::Array(vec![group_id.clone()])),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &role,
        [
            attr("scope", group_id.clone()),
            attr(
                "role_definition_id",
                expr::traversal([
                    "azurerm_role_definition",
                    &role,
                    "role_definition_resource_id",
                ]),
            ),
            attr("principal_id", principal.clone()),
            attr(
                "principal_type",
                Expression::String("ServicePrincipal".to_string()),
            ),
        ],
    ));

    let images = format!("{label}_verifier_images");
    let subscription = expr::template(OWN_SUBSCRIPTION_SCOPE.to_string());
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_definition",
        &images,
        [
            attr(
                "name",
                expr::template("${local.resource_prefix}-verifier-images".to_string()),
            ),
            attr("scope", subscription.clone()),
            attr(
                "description",
                Expression::String(
                    "Reads the community gallery images the install's scale sets boot.".to_string(),
                ),
            ),
            nested(block(
                "permissions",
                [attr(
                    "actions",
                    string_array(
                        AZURE_VERIFIER_IMAGE_ACTIONS
                            .iter()
                            .map(|action| action.to_string())
                            .collect(),
                    ),
                )],
            )),
            attr(
                "assignable_scopes",
                Expression::Array(vec![subscription.clone()]),
            ),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &images,
        [
            attr("scope", subscription),
            attr(
                "role_definition_id",
                expr::traversal([
                    "azurerm_role_definition",
                    &images,
                    "role_definition_resource_id",
                ]),
            ),
            attr("principal_id", principal.clone()),
            attr(
                "principal_type",
                Expression::String("ServicePrincipal".to_string()),
            ),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_role_definition",
        &format!("{label}_machine_signing"),
        [
            attr("name", expr::template("${local.resource_prefix}-machine-signing".to_string())),
            attr("scope", expr::template(OWN_SUBSCRIPTION_SCOPE.to_string())),
            attr(
                "description",
                Expression::String(
                    "What the install's machines read to find their signing settings and the signing vault."
                        .to_string(),
                ),
            ),
            nested(block(
                "permissions",
                [attr("actions", string_array(
                    AZURE_MACHINE_SIGNING_ACTIONS.iter().map(|action| action.to_string()).collect(),
                ))],
            )),
            attr(
                "assignable_scopes",
                Expression::Array(vec![group_id.clone(), signing_group_id.clone()]),
            ),
        ],
    ));

    let vault = format!("{label}_signing");
    let current = format!("{label}_current");
    fragment.resource_blocks.push(resource_block(
        "random_id",
        &format!("{vault}_name_suffix"),
        [attr(
            "byte_length",
            Expression::Number(hcl::Number::from(3i64)),
        )],
    ));
    fragment.resource_blocks.push(resource_block(
        "azurerm_key_vault",
        &vault,
        [
            attr(
                "name",
                expr::raw(format!(
                    "format(\"%s-%s\", trim(substr(lower(replace(\"${{local.resource_prefix}}-signing\", \"_\", \"-\")), 0, 17), \"-\"), random_id.{vault}_name_suffix.hex)"
                )),
            ),
            attr(
                "resource_group_name",
                expr::traversal(["azurerm_resource_group", &signing_group, "name"]),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr(
                "tenant_id",
                expr::raw(format!("data.azurerm_client_config.{current}.tenant_id")),
            ),
            attr("sku_name", Expression::String("standard".to_string())),
            attr("rbac_authorization_enabled", Expression::Bool(true)),
            attr("purge_protection_enabled", Expression::Bool(false)),
            attr("soft_delete_retention_days", Expression::Number(hcl::Number::from(7i64))),
            attr(
                "depends_on",
                Expression::Array(vec![expr::traversal(["azurerm_resource_group", &signing_group])]),
            ),
        ],
    ));
    for (name, member) in [
        (
            "setup",
            expr::raw(format!("data.azurerm_client_config.{current}.object_id")),
        ),
        ("verifier", principal),
    ] {
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{vault}_{name}"),
            [
                attr("scope", expr::traversal(["azurerm_key_vault", &vault, "id"])),
                attr(
                    "role_definition_id",
                    expr::template(format!(
                        "/subscriptions/${{var.azure_subscription_id}}/providers/Microsoft.Authorization/roleDefinitions/{KEY_VAULT_SECRETS_OFFICER}"
                    )),
                ),
                attr("principal_id", member),
            ],
        ));
    }
    let ledger = alien_signing::Ledger::new("${local.resource_prefix}").to_json();
    fragment.resource_blocks.push(resource_block(
        "azurerm_key_vault_secret",
        &format!("{vault}_ledger"),
        [
            attr(
                "name",
                Expression::String(alien_signing::settings::names::AZURE_LEDGER_SECRET.to_string()),
            ),
            attr(
                "key_vault_id",
                expr::traversal(["azurerm_key_vault", &vault, "id"]),
            ),
            attr(
                "value",
                expr::template(ledger.replace('\\', "\\\\").replace('"', "\\\"")),
            ),
            // The ledger only rises; the verifier writes new versions.
            nested(block(
                "lifecycle",
                [attr(
                    "ignore_changes",
                    Expression::Array(vec![expr::raw("value")]),
                )],
            )),
            attr(
                "depends_on",
                Expression::Array(vec![expr::traversal([
                    "azurerm_role_assignment",
                    &format!("{vault}_setup"),
                ])]),
            ),
        ],
    ));
    Ok(())
}

/// The writes and deletes the management roles grant that change only infrastructure running no
/// code, which the deny leaves to the management identity.
fn deny_excluded_actions(roles: &AzureManagementRoles) -> Result<Vec<String>> {
    let actions: BTreeSet<String> = roles
        .roles
        .iter()
        .flat_map(|role| role.actions.iter())
        .filter(|action| azure_write_or_delete(action) && azure_runs_no_code(action))
        // A vault write could turn the signing vault to access policies and grant management
        // its secrets, so on a protected install vaults stay under the deny.
        .filter(|action| {
            !action
                .to_ascii_lowercase()
                .starts_with("microsoft.keyvault/vaults")
        })
        .cloned()
        .collect();
    if actions.len() > DENY_SETTINGS_MAX_EXCLUDED_ACTIONS {
        return Err(alien_error::AlienError::new(ErrorData::OperationNotSupported {
            operation: "protect this Azure deployment".to_string(),
            reason: format!(
                "the management identity keeps {} write and delete actions, more than the {DENY_SETTINGS_MAX_EXCLUDED_ACTIONS} a deny setting can exclude",
                actions.len()
            ),
        }));
    }
    Ok(actions.into_iter().collect())
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

fn emit_management_roles(
    fragment: &mut TfFragment,
    label: &str,
    roles: &AzureManagementRoles,
    role_assignment_shards: usize,
) {
    for role in &roles.roles {
        emit_management_role(fragment, label, role, ManagementRoleKind::Management, 0);
    }
    for role in &roles.role_assignment_roles {
        for shard in 0..role_assignment_shards.max(1) {
            emit_management_role(
                fragment,
                label,
                role,
                ManagementRoleKind::RoleAssignments,
                shard,
            );
        }
    }
}

/// The conditions management's role-assignment roles are assigned with, one per shard, and a
/// local holding the ids of the setup-owned roles they name.
///
/// The ids are derived in Terraform from the resource prefix, so the conditions reference them
/// through `local.<label>_runtime_role_ids`. Shards are cut by the length the conditions have once
/// Terraform resolves every reference to a GUID.
fn runtime_role_conditions(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
    runtime_roles: &AzureRuntimeRoles,
) -> Result<Vec<String>> {
    let local_name = format!("{label}_runtime_role_ids");
    let placeholder = |seed: &str| Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string();
    let mut references: Vec<(String, String)> = Vec::new();
    let mut ids = Vec::new();
    let mut management_ids = BTreeSet::new();
    let mut workload_ids = BTreeSet::new();
    let mut remote_bindings_ids = BTreeSet::new();
    for role in &runtime_roles.custom_roles {
        let seed = role.holder.role_definition_seed(
            "${local.resource_prefix}",
            &role.permission_set_id,
            &role.key,
        );
        let key = format!("r{}", ids.len());
        let stand_in = placeholder(&seed);
        references.push((stand_in.clone(), format!("${{local.{local_name}.{key}}}")));
        ids.push((key, expr::raw(format!("uuidv5(\"oid\", \"{seed}\")"))));
        if role.holder == AzureRuntimeRoleHolder::Management {
            management_ids.insert(stand_in);
        } else if role.holder.is_remote_bindings() {
            remote_bindings_ids.insert(stand_in);
        } else {
            workload_ids.insert(stand_in);
        }
    }
    management_ids.extend(runtime_roles.management_predefined_role_ids.iter().cloned());
    workload_ids.extend(runtime_roles.workload_predefined_role_ids.iter().cloned());
    remote_bindings_ids.extend(
        runtime_roles
            .remote_bindings_predefined_role_ids
            .iter()
            .cloned(),
    );
    if !ids.is_empty() {
        fragment.locals.insert(
            local_name,
            expr::object(
                ids.iter()
                    .map(|(key, value)| (key.as_str(), value.clone()))
                    .collect::<Vec<_>>(),
            ),
        );
    }
    let principal = placeholder("management-principal");
    references.push((
        principal.clone(),
        format!("${{azurerm_user_assigned_identity.{label}.principal_id}}"),
    ));
    // The application's identities: every service account's and the remote-bindings identity,
    // each an `azurerm_user_assigned_identity` under its resource's label. The condition names
    // their principals, so setup must create every one of them.
    let mut workload_principals = AzureWorkloadPrincipals::default();
    for (resource_id, entry) in ctx.stack.resources() {
        let kind = entry.config.resource_type();
        if kind != alien_core::ServiceAccount::RESOURCE_TYPE
            && kind != alien_core::RemoteBindings::RESOURCE_TYPE
        {
            continue;
        }
        if !alien_core::emits_package_scaffolding(
            entry,
            ctx.stack_settings.setup_management,
            ctx.platform,
        ) {
            return Err(alien_error::AlienError::new(
                ErrorData::OperationNotSupported {
                    operation: format!("Azure setup without identity '{resource_id}'"),
                    reason: "management's role-assignment condition names every application \
                         identity, so setup must create it"
                        .to_string(),
                },
            ));
        }
        let Some(identity_label) = ctx.name_for(resource_id) else {
            continue;
        };
        let stand_in = placeholder(&format!("principal:{resource_id}"));
        references.push((
            stand_in.clone(),
            format!("${{azurerm_user_assigned_identity.{identity_label}.principal_id}}"),
        ));
        if kind == alien_core::RemoteBindings::RESOURCE_TYPE {
            workload_principals.remote_bindings.insert(stand_in);
        } else {
            workload_principals.service.insert(stand_in);
        }
    }
    Ok(azure_role_assignment_allowlist_conditions(
        &principal,
        &workload_principals,
        &management_ids,
        &workload_ids,
        &remote_bindings_ids,
    )
    .into_iter()
    .map(|condition| {
        references
            .iter()
            .fold(condition, |condition, (stand_in, reference)| {
                condition.replace(stand_in.as_str(), reference)
            })
    })
    .collect())
}

/// One conditioned assignment per role-assignment role and shard. Not gated: the conditions
/// already admit only the stack's roles.
fn emit_role_assignment_role_assignments(
    fragment: &mut TfFragment,
    label: &str,
    roles: &AzureManagementRoles,
    conditions: &[String],
) {
    for role in &roles.role_assignment_roles {
        for (shard, condition) in conditions.iter().enumerate() {
            let address = management_role_address(
                label,
                &role.scope,
                role.scope_kind,
                ManagementRoleKind::RoleAssignments,
                shard,
            );
            let shard_part = if shard == 0 {
                String::new()
            } else {
                format!(":shard-{shard}")
            };
            let assignment_name = format!(
                "deployment:azure:mgmt-role-assignments-assign:${{local.resource_prefix}}:{}{shard_part}",
                role.scope
            );
            fragment.resource_blocks.push(resource_block(
                "azurerm_role_assignment",
                &format!("{address}_assignment"),
                [
                    attr(
                        "name",
                        expr::raw(&format!("uuidv5(\"oid\", \"{assignment_name}\")")),
                    ),
                    attr("scope", expr::template(role.scope.clone())),
                    attr(
                        "role_definition_id",
                        expr::traversal([
                            "azurerm_role_definition",
                            &address,
                            "role_definition_resource_id",
                        ]),
                    ),
                    attr(
                        "principal_id",
                        expr::traversal(["azurerm_user_assigned_identity", label, "principal_id"]),
                    ),
                    attr("condition", expr::template(condition.clone())),
                    attr("condition_version", Expression::String("2.0".to_string())),
                ],
            ));
        }
    }
}

/// The remote-bindings identity's roles the runtime assigns, under the runtime's ids. The
/// profiles' and management's are emitted with the resources that use them.
fn emit_remote_bindings_runtime_roles(
    fragment: &mut TfFragment,
    runtime_roles: &AzureRuntimeRoles,
) {
    let holder = AzureRuntimeRoleHolder::Profile(AZURE_REMOTE_BINDINGS_PROFILE.to_string());
    for (index, role) in runtime_roles
        .custom_roles
        .iter()
        .filter(|role| role.holder == holder)
        .enumerate()
    {
        let seed = holder.role_definition_seed(
            "${local.resource_prefix}",
            &role.permission_set_id,
            &role.key,
        );
        let definition = &role.role_definition;
        let assignable_scopes = protected_management_assignable_scopes(
            OWN_RESOURCE_GROUP_SCOPE,
            OWN_SUBSCRIPTION_SCOPE,
        )
        .into_iter()
        .map(expr::template)
        .collect();
        fragment.push_shared_resource(resource_block(
            "azurerm_role_definition",
            &format!("runtime_remote_bindings_{index}"),
            [
                attr(
                    "name",
                    expr::template(format!(
                        "${{local.resource_prefix}}-{} [{AZURE_REMOTE_BINDINGS_PROFILE}]",
                        definition.name
                    )),
                ),
                attr(
                    "role_definition_id",
                    expr::raw(format!("uuidv5(\"oid\", \"{seed}\")")),
                ),
                attr(
                    "scope",
                    expr::template(OWN_RESOURCE_GROUP_SCOPE.to_string()),
                ),
                attr(
                    "description",
                    expr::template(definition.description.clone()),
                ),
                nested(block(
                    "permissions",
                    [
                        attr("actions", string_array(definition.actions.clone())),
                        attr(
                            "data_actions",
                            string_array(definition.data_actions.clone()),
                        ),
                        attr("not_actions", string_array(definition.not_actions.clone())),
                        attr(
                            "not_data_actions",
                            string_array(definition.not_data_actions.clone()),
                        ),
                    ],
                )),
                attr("assignable_scopes", Expression::Array(assignable_scopes)),
            ],
        ));
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
    shard: usize,
) {
    let kind = role.scope_kind.as_str();
    let part = role_kind.name_part();
    let shard_name = if shard == 0 {
        String::new()
    } else {
        format!("-{shard}")
    };
    let name = match scope_suffix(&role.scope) {
        None => format!("${{local.resource_prefix}}-management{part}-{kind}-role{shard_name}"),
        Some(suffix) => format!(
            "${{local.resource_prefix}}-management{part}-{kind}-role-{}{shard_name}",
            suffix
        ),
    };
    let shard_id = if shard == 0 {
        String::new()
    } else {
        format!(":shard-{shard}")
    };
    let description = match role_kind {
        ManagementRoleKind::Management => {
            format!("Management role for Terraform resource '{label}'. Scope: {kind}.")
        }
        ManagementRoleKind::RoleAssignments => format!(
            "Role assignments the management identity of Terraform resource '{label}' makes, limited to the roles setup made for the stack. Scope: {kind}."
        ),
    };
    let assignable_scopes =
        protected_management_assignable_scopes(&role.scope, OWN_SUBSCRIPTION_SCOPE)
            .into_iter()
            .map(expr::template)
            .collect();
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_definition",
        &management_role_address(label, &role.scope, role.scope_kind, role_kind, shard),
        [
            attr("name", expr::template(name)),
            attr(
                "role_definition_id",
                expr::raw(format!(
                    "uuidv5(\"oid\", \"deployment:azure:{}:${{local.resource_prefix}}:{}{shard_id}\")",
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
                    attr("not_actions", string_array(role.not_actions.clone())),
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
    shard: usize,
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
    let base = match scope_suffix(scope) {
        None => base,
        Some(suffix) => format!("{base}_{suffix}"),
    };
    if shard == 0 {
        base
    } else {
        format!("{base}_shard_{shard}")
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
    ctx: &EmitContext<'_>,
    label: &str,
    global_refs: &[&PermissionSetReference],
    resource_scoped_refs: &[(&String, &PermissionSetReference)],
) -> Result<ManagementGrants> {
    let mut custom_roles = Vec::new();
    let mut bindings = Vec::new();
    let context = permission_context(label).with_signing(ctx.stack_settings);
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
                &management_role_address(label, scope, kind, role_kind, 0),
                "role_definition_resource_id",
            ]))
        }
    }
}

fn string_array(items: Vec<String>) -> Expression {
    Expression::Array(items.into_iter().map(Expression::String).collect())
}

#[cfg(test)]
mod signing_group_tests {
    use super::*;

    /// The machines and the verifier find the signing vault by this group's name, which the
    /// template must give it.
    #[test]
    fn the_template_names_the_signing_group_as_the_readers_do() {
        assert_eq!(
            alien_signing::settings::names::azure_signing_resource_group("group"),
            format!("group{SIGNING_GROUP_SUFFIX}")
        );
    }
}
