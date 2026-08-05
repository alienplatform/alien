use crate::{
    block::{attr, data_block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, emit_remote_bindings_role_definitions_at_scope, permission_context,
        remote_bindings_role_label, required_label, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, Key, RemoteBindings, RemoteStackManagement, Result,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureKeyEmitter;

impl TfEmitter for AzureKeyEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<Key>(ctx, Key::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let client_label = format!("{label}_current");
        let suffix_label = format!("{label}_vault_suffix");
        let resource_group_label = format!("{label}_key");
        let mut fragment = TfFragment::default();

        fragment
            .data_blocks
            .push(data_block("azurerm_client_config", &client_label, []));
        fragment.resource_blocks.push(resource_block(
            "random_id",
            &suffix_label,
            [attr(
                "byte_length",
                Expression::Number(hcl::Number::from(3i64)),
            )],
        ));
        // A retained Key Vault cannot live in the deployment's ordinary
        // resource group: detaching the vault and then destroying that group
        // would either fail or delete unrelated retained data. Give each Key
        // a small ownership boundary that is retained with it.
        fragment.resource_blocks.push(resource_block(
            "azurerm_resource_group",
            &resource_group_label,
            [
                attr(
                    "name",
                    expr::raw(format!(
                        "substr(lower(replace(\"${{local.resource_prefix}}-{}-key\", \"_\", \"-\")), 0, 90)",
                        ctx.resource_id
                    )),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("tags", tags(ctx, "key-resource-group")),
                nested(crate::block::block(
                    "lifecycle",
                    [attr("prevent_destroy", Expression::Bool(true))],
                )),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "azurerm_key_vault",
            label,
            [
                attr(
                    "name",
                    expr::raw(format!(
                        "format(\"%s-%s\", trim(substr(lower(replace(\"${{local.resource_prefix}}-{}\", \"_\", \"-\")), 0, 17), \"-\"), random_id.{suffix_label}.hex)",
                        ctx.resource_id
                    )),
                ),
                attr(
                    "resource_group_name",
                    expr::traversal([
                        "azurerm_resource_group",
                        resource_group_label.as_str(),
                        "name",
                    ]),
                ),
                attr(
                    "location",
                    expr::traversal([
                        "azurerm_resource_group",
                        resource_group_label.as_str(),
                        "location",
                    ]),
                ),
                attr(
                    "tenant_id",
                    expr::traversal([
                        "data",
                        "azurerm_client_config",
                        client_label.as_str(),
                        "tenant_id",
                    ]),
                ),
                attr("sku_name", Expression::String("standard".to_string())),
                attr("rbac_authorization_enabled", Expression::Bool(true)),
                attr("purge_protection_enabled", Expression::Bool(true)),
                attr(
                    "soft_delete_retention_days",
                    Expression::Number(hcl::Number::from(90i64)),
                ),
                attr("public_network_access_enabled", Expression::Bool(true)),
                attr("tags", tags(ctx, "key")),
                nested(crate::block::block(
                    "lifecycle",
                    [attr("prevent_destroy", Expression::Bool(true))],
                )),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "azurerm_key_vault_key",
            label,
            [
                attr("name", Expression::String("key".to_string())),
                attr(
                    "key_vault_id",
                    expr::traversal(["azurerm_key_vault", label, "id"]),
                ),
                attr("key_type", Expression::String("RSA".to_string())),
                attr("key_size", Expression::Number(hcl::Number::from(2048i64))),
                attr(
                    "key_opts",
                    Expression::Array(vec![
                        Expression::String("decrypt".to_string()),
                        Expression::String("encrypt".to_string()),
                        Expression::String("unwrapKey".to_string()),
                        Expression::String("wrapKey".to_string()),
                    ]),
                ),
                attr(
                    "depends_on",
                    Expression::Array(vec![expr::traversal([
                        "time_sleep",
                        &format!("{label}_installer_rbac"),
                    ])]),
                ),
                nested(crate::block::block(
                    "lifecycle",
                    [attr("prevent_destroy", Expression::Bool(true))],
                )),
            ],
        ));

        let installer_assignment_label = format!("{label}_installer_key_admin");
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &installer_assignment_label,
            [
                attr(
                    "name",
                    expr::raw(format!(
                        "uuidv5(\"oid\", \"deployment:azure:key-installer:${{local.resource_prefix}}:{label}\")"
                    )),
                ),
                attr(
                    "scope",
                    expr::traversal(["azurerm_key_vault", label, "id"]),
                ),
                attr(
                    "role_definition_id",
                    expr::template("/subscriptions/${var.azure_subscription_id}/providers/Microsoft.Authorization/roleDefinitions/14b46e9e-c2b7-41b4-b07b-48a6ebf60603".to_string()),
                ),
                attr(
                    "principal_id",
                    expr::traversal([
                        "data",
                        "azurerm_client_config",
                        client_label.as_str(),
                        "object_id",
                    ]),
                ),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "time_sleep",
            &format!("{label}_installer_rbac"),
            [
                attr("create_duration", Expression::String("60s".to_string())),
                attr(
                    "depends_on",
                    Expression::Array(vec![expr::traversal([
                        "azurerm_role_assignment",
                        installer_assignment_label.as_str(),
                    ])]),
                ),
            ],
        ));

        emit_remote_access(ctx, label, &mut fragment)?;
        emit_management_access(ctx, label, &mut fragment)?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "vaultResourceId",
                expr::traversal(["azurerm_key_vault", label, "id"]),
            ),
            (
                "keyName",
                expr::traversal(["azurerm_key_vault_key", label, "name"]),
            ),
            (
                "lineageVersionId",
                expr::traversal(["azurerm_key_vault_key", label, "version"]),
            ),
            (
                "keyId",
                expr::traversal(["azurerm_key_vault_key", label, "id"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("key-vault-key".to_string())),
            (
                "keyId",
                expr::traversal(["azurerm_key_vault_key", label, "id"]),
            ),
        ])))
    }
}

fn emit_management_access(
    ctx: &EmitContext<'_>,
    label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let Some(management_label) = management_label(ctx) else {
        return Ok(());
    };
    let permission_set =
        alien_permissions::get_permission_set("key/management").ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "key/management permission set is not registered".to_string(),
            })
        })?;
    let context = permission_context(label).with_resource_name(format!(
        "${{azurerm_key_vault_key.{label}.resource_versionless_id}}"
    ));
    let plan = AzureRuntimePermissionsGenerator::new()
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: "failed to generate Azure Key management permissions".to_string(),
        })?;

    for (index, binding) in plan.bindings.iter().enumerate() {
        let AzureRoleDefinitionRef::Predefined { role_definition_id } = &binding.role_definition
        else {
            return Err(AlienError::new(ErrorData::GenericError {
                message: "key/management must use an Azure predefined role".to_string(),
            }));
        };
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_management_{index}"),
            [
                attr(
                    "name",
                    expr::raw(format!(
                        "uuidv5(\"oid\", \"deployment:azure:key-management:${{local.resource_prefix}}:{label}:{index}\")"
                    )),
                ),
                attr("scope", expr::template(binding.scope.clone())),
                attr(
                    "role_definition_id",
                    expr::template(role_definition_id.clone()),
                ),
                attr(
                    "principal_id",
                    expr::traversal([
                        "azurerm_user_assigned_identity",
                        management_label,
                        "principal_id",
                    ]),
                ),
            ],
        ));
    }

    Ok(())
}

fn emit_remote_access(ctx: &EmitContext<'_>, label: &str, fragment: &mut TfFragment) -> Result<()> {
    if alien_core::remote_bindings::remote_binding_for_entry(ctx.resource).is_none() {
        return Ok(());
    }
    let access_label = remote_bindings_label(ctx).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: "remote Key has no Remote Bindings identity".to_string(),
        })
    })?;
    let permission_set = alien_permissions::get_permission_set("key/remote-cryptography")
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "key/remote-cryptography permission set is not registered".to_string(),
            })
        })?;
    let context = permission_context(label).with_resource_name(format!(
        "${{azurerm_key_vault_key.{label}.resource_versionless_id}}"
    ));
    let plan = AzureRuntimePermissionsGenerator::new()
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .context(ErrorData::GenericError {
            message: "failed to generate Azure remote Key permissions".to_string(),
        })?;

    let resource_group_label = format!("{label}_key");
    emit_remote_bindings_role_definitions_at_scope(
        fragment,
        permission_set,
        expr::traversal([
            "azurerm_resource_group",
            resource_group_label.as_str(),
            "id",
        ]),
        format!("${{azurerm_resource_group.{resource_group_label}.id}}"),
    )?;
    for (index, binding) in plan.bindings.iter().enumerate() {
        let role_definition_id = match &binding.role_definition {
            AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                expr::template(role_definition_id.clone())
            }
            AzureRoleDefinitionRef::Custom { key } => {
                let custom_index = plan
                    .custom_roles
                    .iter()
                    .position(|role| &role.key == key)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::GenericError {
                            message: format!("missing generated Azure role '{key}'"),
                        })
                    })?;
                let role_label = remote_bindings_role_label(&binding.role_name, custom_index);
                expr::traversal([
                    "azurerm_role_definition",
                    role_label.as_str(),
                    "role_definition_resource_id",
                ])
            }
        };
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_access_{index}"),
            [
                attr(
                    "name",
                    expr::raw(format!(
                        "uuidv5(\"oid\", \"deployment:azure:key-access:${{local.resource_prefix}}:{label}:{index}\")"
                    )),
                ),
                attr("scope", expr::template(binding.scope.clone())),
                attr("role_definition_id", role_definition_id),
                attr(
                    "principal_id",
                    expr::traversal([
                        "azurerm_user_assigned_identity",
                        access_label,
                        "principal_id",
                    ]),
                ),
            ],
        ));
    }

    Ok(())
}

fn remote_bindings_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    })
}

fn management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    let has_permission = ctx
        .stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(ctx.resource_id))
        .is_some_and(|refs| {
            refs.iter()
                .any(|reference| reference.id() == "key/management")
        });
    has_permission.then(|| {
        ctx.stack.resources().find_map(|(id, entry)| {
            (entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
                .then(|| ctx.name_for(id))
                .flatten()
        })
    })?
}
