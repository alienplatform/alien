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
    emitters::azure::helpers::{
        downcast, permission_context, required_label, service_account_principal_id,
        setup_execution_role_label, setup_management_role_label, tags,
    },
    emitters::enabled,
    expr,
};
use alien_core::{
    import::EmitContext, PermissionProfile, PermissionSetReference, RemoteStackManagement, Result,
    Vault,
};
use alien_error::Context;
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureVaultEmitter;

impl TfEmitter for AzureVaultEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let vault = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();
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

        // The random suffix is not gated: it is a local value with no cloud
        // footprint, and leaving it uncounted keeps the vault's reference to it
        // unindexed.
        let name_suffix_label = vault_name_suffix_label(label);
        fragment.resource_blocks.push(resource_block(
            "random_id",
            &name_suffix_label,
            [attr(
                "byte_length",
                Expression::Number(hcl::Number::from(3i64)),
            )],
        ));

        let mut key_vault = resource_block(
            "azurerm_key_vault",
            label,
            [
                attr("name", vault_name_expr(vault.id(), &name_suffix_label)),
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
        );
        enabled::gate(&mut key_vault, enabled_when)?;
        fragment.resource_blocks.push(key_vault);

        emit_vault_permissions(ctx, label, enabled_when, &mut fragment)?;

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "vaultName",
                enabled::attribute(enabled_when, "azurerm_key_vault", label, "name"),
            ),
            (
                "vaultUri",
                enabled::attribute(enabled_when, "azurerm_key_vault", label, "vault_uri"),
            ),
        ]))
    }

    fn supports_enabled_when(&self) -> bool {
        true
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

/// Key Vault names are 3-24 alphanumeric-or-dash characters and globally
/// unique. Azure retains soft-deleted vault names for the recovery window,
/// so each Terraform state gets a stable random suffix that changes only
/// after destroy/reinstall.
fn vault_name_expr(vault_id: &str, suffix_label: &str) -> Expression {
    expr::raw(format!(
        "format(\"%s-%s\", trim(substr(lower(replace(\"${{local.resource_prefix}}-{}\", \"_\", \"-\")), 0, 17), \"-\"), random_id.{}.hex)",
        vault_id, suffix_label
    ))
}

fn vault_name_suffix_label(vault_label: &str) -> String {
    format!("{vault_label}_name_suffix")
}

fn emit_vault_permissions(
    ctx: &EmitContext<'_>,
    vault_label: &str,
    enabled_when: Option<&str>,
    fragment: &mut TfFragment,
) -> Result<()> {
    let vault = VaultTarget {
        label: vault_label,
        enabled_when,
    };

    for (profile_name, permission_set_refs) in vault_permission_owners(ctx) {
        let Some(principal_id_expr) = service_account_principal_id(ctx, &profile_name) else {
            continue;
        };

        for permission_set_ref in permission_set_refs {
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none()
            {
                continue;
            }

            let generator = AzureRuntimePermissionsGenerator::new();
            let permission_context = permission_context(vault_label)
                .with_resource_name(format!("${{local.resource_prefix}}-{}", ctx.resource_id));
            let grant_plan = generator
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(alien_core::ErrorData::GenericError {
                    message: format!(
                        "failed to generate Azure vault grants for '{}'",
                        permission_set.id
                    ),
                })?;

            for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
                let role_definition_id = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        expr::template(role_definition_id.clone())
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        let index = grant_plan
                            .custom_roles
                            .iter()
                            .position(|role| role.key == *key)
                            .unwrap_or(0);
                        let role_label =
                            setup_execution_role_label(&profile_name, &binding.role_name, index);
                        expr::traversal([
                            "azurerm_role_definition",
                            role_label.as_str(),
                            "role_definition_resource_id",
                        ])
                    }
                };
                emit_role_assignment(
                    fragment,
                    vault,
                    &profile_name,
                    binding_index,
                    &binding.role_name,
                    role_definition_id,
                    principal_id_expr.clone(),
                )?;
            }
        }
    }

    let Some(management_label) = remote_stack_management_label(ctx) else {
        return Ok(());
    };

    let principal_id_expr = expr::traversal([
        "azurerm_user_assigned_identity",
        management_label,
        "principal_id",
    ]);

    for permission_set_ref in management_permission_refs(ctx) {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        else {
            continue;
        };
        if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none() {
            continue;
        }

        let generator = AzureRuntimePermissionsGenerator::new();
        let permission_context = permission_context(vault_label)
            .with_resource_name(format!("${{local.resource_prefix}}-{}", ctx.resource_id));
        let grant_plan = generator
            .generate_grant_plan(
                &permission_set,
                BindingTarget::Resource,
                &permission_context,
            )
            .context(alien_core::ErrorData::GenericError {
                message: format!(
                    "failed to generate Azure vault management grants for '{}'",
                    permission_set.id
                ),
            })?;

        for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
            let role_definition_id = match &binding.role_definition {
                AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                    expr::template(role_definition_id.clone())
                }
                AzureRoleDefinitionRef::Custom { key } => {
                    let index = grant_plan
                        .custom_roles
                        .iter()
                        .position(|role| role.key == *key)
                        .unwrap_or(0);
                    let role_label = setup_management_role_label(&binding.role_name, index);
                    expr::traversal([
                        "azurerm_role_definition",
                        role_label.as_str(),
                        "role_definition_resource_id",
                    ])
                }
            };
            emit_role_assignment(
                fragment,
                vault,
                "management",
                binding_index,
                &binding.role_name,
                role_definition_id,
                principal_id_expr.clone(),
            )?;
        }
    }

    Ok(())
}

/// The vault a role assignment attaches to. The label and the gate travel
/// together because every reference to the vault has to agree on whether the
/// `[0]` is there.
#[derive(Clone, Copy)]
struct VaultTarget<'a> {
    label: &'a str,
    enabled_when: Option<&'a str>,
}

impl VaultTarget<'_> {
    /// Address of the vault block, indexed when it is counted.
    fn address(&self) -> String {
        match self.enabled_when {
            Some(_) => format!("azurerm_key_vault.{}[0]", self.label),
            None => format!("azurerm_key_vault.{}", self.label),
        }
    }
}

fn emit_role_assignment(
    fragment: &mut TfFragment,
    vault: VaultTarget<'_>,
    principal_label: &str,
    binding_index: usize,
    role_name: &str,
    role_definition_id: Expression,
    principal_id_expr: Expression,
) -> Result<()> {
    let role_label = sanitize_role_label(role_name);
    let vault_label = vault.label;
    // The assignment names the vault twice — once as its scope, once inside the
    // uuidv5 seed — so both read the same address.
    let vault_address = vault.address();
    let mut assignment = resource_block(
        "azurerm_role_assignment",
        &format!("{vault_label}_{role_label}_{principal_label}_assignment_{binding_index}"),
        [
            attr(
                "name",
                expr::raw(&format!(
                    "uuidv5(\"oid\", \"deployment:azure:vault-role-assign:${{{vault_address}.id}}:{role_label}:{principal_label}:{binding_index}\")"
                )),
            ),
            attr(
                "scope",
                enabled::attribute(vault.enabled_when, "azurerm_key_vault", vault_label, "id"),
            ),
            attr("role_definition_id", role_definition_id),
            attr("principal_id", principal_id_expr),
        ],
    );
    // The grant targets this vault, so it only exists while the vault does.
    enabled::gate(&mut assignment, vault.enabled_when)?;
    fragment.resource_blocks.push(assignment);
    Ok(())
}

fn sanitize_role_label(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}

fn vault_permission_owners<'a>(
    ctx: &'a EmitContext<'_>,
) -> Vec<(String, Vec<&'a PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        owners.push((profile_name.clone(), refs));
    }
    owners
}

fn management_permission_refs<'a>(ctx: &'a EmitContext<'_>) -> Vec<&'a PermissionSetReference> {
    let Some(profile) = ctx.stack.management().profile() else {
        return Vec::new();
    };
    resource_permission_refs(profile, ctx.resource_id)
}

fn resource_permission_refs<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Vec<&'a PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("vault/"))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    refs
}
