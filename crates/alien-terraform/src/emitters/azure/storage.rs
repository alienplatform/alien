//! Azure Storage — `azurerm_storage_container` inside the stack's
//! `azurerm_storage_account`.
//!
//! Mirrors `AzureStorageController`:
//!
//! * Container name = `lower(replace("${local.resource_prefix}-{id}", "_", "-"))`
//!   — the runtime's `get_azure_container_name` helper, reproduced in HCL
//!   so push/pull converge byte-identical.
//! * Public access maps `storage.public_read` to the provider's
//!   `container_access_type` of `blob` (anonymous read on objects only)
//!   versus `private`.
//! * Storage-account-level `versioning_enabled` is set by the auxiliary
//!   [`super::storage_account::AzureStorageAccountEmitter`] when any
//!   container opted in (Azure scopes the toggle to the account, not
//!   the container — see that file's doc comment).
//! * Lifecycle rules belong to the shared account emitter, which owns the
//!   single account-wide management policy.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{
        downcast, emit_remote_bindings_role_definitions, permission_context,
        remote_bindings_role_label, remote_stack_management_label, required_label,
        service_account_principal_id, setup_execution_role_label, setup_management_role_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, AzureStorageAccount, ErrorData, PermissionProfile, PermissionSet,
    PermissionSetReference, Result, Storage,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureCustomRole, AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::{
    expr::{Expression, FuncCall},
    Identifier,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureStorageEmitter;

impl TfEmitter for AzureStorageEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let storage = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let parent_label = parent_storage_account_label(ctx)?.to_string();

        let mut fragment = TfFragment::default();

        let access_type = if storage.public_read {
            "blob"
        } else {
            "private"
        };

        let container = resource_block(
            "azurerm_storage_container",
            label,
            [
                attr("name", container_name_expr(storage.id())),
                attr(
                    "storage_account_name",
                    expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
                ),
                attr(
                    "container_access_type",
                    Expression::String(access_type.to_string()),
                ),
            ],
        );
        fragment.resource_blocks.push(container);

        emit_storage_permissions(ctx, label, &parent_label, &mut fragment)?;
        emit_notifications(ctx, label, &parent_label, storage, &mut fragment)?;

        fragment.retain_resources(|kind, _| kind == "azurerm_storage_container");
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let parent_label = parent_storage_account_label(ctx)?.to_string();
        Ok(expr::object([
            ("subscriptionId", expr::raw("var.azure_subscription_id")),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "storageAccountName",
                expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
            ),
            (
                "containerName",
                expr::traversal(["azurerm_storage_container", label, "name"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let _ = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let parent_label = parent_storage_account_label(ctx)?.to_string();
        Ok(Some(expr::object([
            ("service", Expression::String("blob".to_string())),
            (
                "accountName",
                expr::traversal(["azurerm_storage_account", &parent_label, "name"]),
            ),
            (
                "containerName",
                expr::traversal(["azurerm_storage_container", label, "name"]),
            ),
        ])))
    }
}

fn emit_notifications(
    ctx: &EmitContext<'_>,
    label: &str,
    parent_label: &str,
    storage: &Storage,
    fragment: &mut TfFragment,
) -> Result<()> {
    for (index, notification) in storage.notifications.iter().enumerate() {
        let queue_label = ctx.name_for(&notification.queue.id).ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Storage '{}' notification {index} queue '{}' has no Terraform resource",
                    storage.id(),
                    notification.queue.id
                ),
            })
        })?;
        let container = format!("azurerm_storage_container.{label}.name");
        // Match the controller's deterministic name, including long container names.
        let name = expr::raw(format!(
            "format(\"%s-sn-%s-{index}\", substr({container}, 0, 40), substr(sha256({container}), 0, 12))"
        ));
        let mut filter = vec![
            attr("case_sensitive", Expression::Bool(true)),
            attr(
                "subject_begins_with",
                Expression::FuncCall(Box::new(
                    FuncCall::builder(Identifier::sanitized("format"))
                        .arg(Expression::String(
                            "/blobServices/default/containers/%s/blobs/%s".into(),
                        ))
                        .arg(expr::traversal([
                            "azurerm_storage_container",
                            label,
                            "name",
                        ]))
                        .arg(Expression::String(
                            notification.prefix.clone().unwrap_or_default(),
                        ))
                        .build(),
                )),
            ),
        ];
        if let Some(suffix) = &notification.suffix {
            filter.push(attr(
                "subject_ends_with",
                Expression::String(suffix.clone()),
            ));
        }
        // Account-scoped subscriptions reuse an existing system topic (including
        // one used by Worker storage triggers). Azure creates it if absent; this
        // Storage must never take ownership of the shared topic.
        fragment.resource_blocks.push(resource_block(
            "azurerm_eventgrid_event_subscription",
            &format!("{label}_notification_{index}"),
            [
                attr("name", name),
                attr(
                    "scope",
                    expr::traversal(["azurerm_storage_account", parent_label, "id"]),
                ),
                attr(
                    "service_bus_queue_endpoint_id",
                    expr::traversal(["azurerm_servicebus_queue", queue_label, "id"]),
                ),
                attr(
                    "event_delivery_schema",
                    Expression::String("CloudEventSchemaV1_0".into()),
                ),
                attr(
                    "included_event_types",
                    Expression::Array(vec![Expression::String(
                        "Microsoft.Storage.BlobCreated".into(),
                    )]),
                ),
                nested(block("subject_filter", filter)),
            ],
        ));
    }
    Ok(())
}

/// Find the auxiliary `azure_storage_account` resource in the stack and
/// return its Terraform label. The preflight pipeline always injects
/// exactly one of these per stack as `default-storage-account`; we
/// surface a typed error rather than panicking if it's missing.
fn parent_storage_account_label<'a>(ctx: &EmitContext<'a>) -> Result<&'a str> {
    for (id, entry) in ctx.stack.resources() {
        if entry.config.downcast_ref::<AzureStorageAccount>().is_some() {
            if let Some(label) = ctx.name_for(id) {
                return Ok(label);
            }
        }
    }
    Err(AlienError::new(ErrorData::GenericError {
        message:
            "Azure Storage resource requires a sibling `azure_storage_account` resource in the \
             stack (preflight-injected as `default-storage-account`)"
                .to_string(),
    }))
}

pub(super) fn container_name_expr(storage_id: &str) -> Expression {
    // `replace(lower("${local.resource_prefix}-{id}"), "_", "-")` — match
    // runtime's `get_azure_container_name` so push and pull resolve to
    // the same physical container.
    expr::raw(format!(
        "replace(lower(\"${{local.resource_prefix}}-{}\"), \"_\", \"-\")",
        storage_id
    ))
}

fn emit_storage_permissions(
    ctx: &EmitContext<'_>,
    storage_label: &str,
    parent_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    for (profile_name, permission_set_refs) in storage_permission_owners(ctx) {
        let Some(principal_id_expr) = service_account_principal_id(ctx, &profile_name) else {
            continue;
        };

        for permission_set_ref in permission_set_refs {
            let permission_set = resolve_permission_set(permission_set_ref, ctx.resource_id)?;
            if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none()
            {
                continue;
            }

            let generator = AzureRuntimePermissionsGenerator::new();
            let permission_context = permission_context(storage_label)
                .with_resource_name(container_name_expr_string(ctx.resource_id))
                .with_storage_account_name(storage_account_name_expr_string(parent_label));
            let grant_plan = generator
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate Azure storage grants for '{}'",
                        permission_set.id
                    ),
                })?;

            for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
                let role_definition_id = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        expr::template(role_definition_id.clone())
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        let index =
                            custom_role_index(&grant_plan.custom_roles, key, &permission_set.id)?;
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
                    ctx.resource_id,
                    storage_label,
                    &profile_name,
                    binding_index,
                    &binding.role_name,
                    expr::template(binding.scope.clone()),
                    role_definition_id,
                    principal_id_expr.clone(),
                )?;
            }
        }
    }

    if let Some(management_label) = remote_stack_management_label(ctx) {
        let management_principal_id_expr = expr::traversal([
            "azurerm_user_assigned_identity",
            management_label,
            "principal_id",
        ]);

        for permission_set_ref in management_permission_refs(ctx) {
            let permission_set = resolve_permission_set(permission_set_ref, ctx.resource_id)?;
            if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none()
            {
                continue;
            }

            let generator = AzureRuntimePermissionsGenerator::new();
            let permission_context = permission_context(storage_label)
                .with_resource_name(container_name_expr_string(ctx.resource_id))
                .with_storage_account_name(storage_account_name_expr_string(parent_label));
            let grant_plan = generator
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate Azure storage management grants for '{}'",
                        permission_set.id
                    ),
                })?;

            for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
                let role_definition_id = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        expr::template(role_definition_id.clone())
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        let index =
                            custom_role_index(&grant_plan.custom_roles, key, &permission_set.id)?;
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
                    ctx.resource_id,
                    storage_label,
                    "management",
                    binding_index,
                    &binding.role_name,
                    expr::template(binding.scope.clone()),
                    role_definition_id,
                    management_principal_id_expr.clone(),
                )?;
            }
        }
    }

    if let (Some(definition), Some(bindings_label)) = (
        alien_core::remote_bindings::remote_binding_for_entry(ctx.resource),
        remote_bindings_label(ctx),
    ) {
        let principal_id_expr = expr::traversal([
            "azurerm_user_assigned_identity",
            bindings_label,
            "principal_id",
        ]);
        let remote_permission = PermissionSetReference::from_name(definition.permission_set);
        let permission_set_ref = &remote_permission;
        let permission_set = resolve_permission_set(permission_set_ref, ctx.resource_id)?;
        if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none() {
            return Ok(());
        }

        let generator = AzureRuntimePermissionsGenerator::new();
        let permission_context = permission_context(storage_label)
            .with_resource_name(container_name_expr_string(ctx.resource_id))
            .with_storage_account_name(storage_account_name_expr_string(parent_label));
        let grant_plan = generator
            .generate_grant_plan(
                &permission_set,
                BindingTarget::Resource,
                &permission_context,
            )
            .context(ErrorData::GenericError {
                message: format!(
                    "failed to generate Azure storage management grants for '{}'",
                    permission_set.id
                ),
            })?;

        emit_remote_bindings_role_definitions(fragment, &permission_set)?;

        for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
            let role_definition_id = match &binding.role_definition {
                AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                    expr::template(role_definition_id.clone())
                }
                AzureRoleDefinitionRef::Custom { key } => {
                    let index =
                        custom_role_index(&grant_plan.custom_roles, key, &permission_set.id)?;
                    let role_label = remote_bindings_role_label(&binding.role_name, index);
                    expr::traversal([
                        "azurerm_role_definition",
                        role_label.as_str(),
                        "role_definition_resource_id",
                    ])
                }
            };
            emit_role_assignment(
                fragment,
                ctx.resource_id,
                storage_label,
                "access",
                binding_index,
                &binding.role_name,
                expr::template(binding.scope.clone()),
                role_definition_id,
                principal_id_expr.clone(),
            )?;
        }
    }

    Ok(())
}

fn resolve_permission_set(
    permission_set_ref: &PermissionSetReference,
    resource_id: &str,
) -> Result<PermissionSet> {
    permission_set_ref
        .resolve(|name| alien_permissions::get_permission_set(name).cloned())
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "permission set '{}' referenced by Azure storage resource '{}' was not found",
                    permission_set_ref.id(),
                    resource_id
                ),
            })
        })
}

fn custom_role_index(
    custom_roles: &[AzureCustomRole],
    key: &str,
    permission_set_id: &str,
) -> Result<usize> {
    custom_roles
        .iter()
        .position(|role| role.key == key)
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Azure storage permission set '{}' generated a binding for missing custom role '{}'",
                    permission_set_id, key
                ),
            })
        })
}

fn emit_role_assignment(
    fragment: &mut TfFragment,
    storage_id: &str,
    storage_label: &str,
    principal_label: &str,
    binding_index: usize,
    role_name: &str,
    scope: Expression,
    role_definition_id: Expression,
    principal_id_expr: Expression,
) -> Result<()> {
    let role_label = sanitize_role_label(role_name);
    let assignment = resource_block(
        "azurerm_role_assignment",
        &format!("{storage_label}_{role_label}_{principal_label}_assignment_{binding_index}"),
        [
            attr(
                "name",
                expr::raw(&format!(
                    "uuidv5(\"oid\", \"deployment:azure:storage-role-assign:${{local.resource_prefix}}:{storage_id}:{role_label}:{principal_label}:{binding_index}\")"
                )),
            ),
            attr("scope", scope),
            attr("role_definition_id", role_definition_id),
            attr("principal_id", principal_id_expr),
        ],
    );
    fragment.resource_blocks.push(assignment);
    Ok(())
}

fn storage_permission_owners<'a>(
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
            .filter(|permission_ref| permission_ref.id().starts_with("storage/"))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    refs
}

fn remote_bindings_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == alien_core::RemoteBindings::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    })
}

fn storage_account_name_expr_string(parent_label: &str) -> String {
    format!("${{azurerm_storage_account.{parent_label}.name}}")
}

fn container_name_expr_string(storage_id: &str) -> String {
    format!("${{replace(lower(\"${{local.resource_prefix}}-{storage_id}\"), \"_\", \"-\")}}")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};
    use alien_core::{
        AzureResourceGroup, ManagementPermissions, RemoteBindings, RemoteStackManagement,
        ResourceLifecycle, ServiceAccount, Stack, StackSettings,
    };

    const STORAGE_BLOB_DATA_CONTRIBUTOR_ROLE_ID: &str = "ba92f5b4-2d11-453d-a403-e96b0029c9fe";
    const RESOURCE_GROUP_SCOPE: &str = "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\"";
    const STORAGE_CONTAINER_SCOPE: &str = "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}/providers/Microsoft.Storage/storageAccounts/${azurerm_storage_account.default_storage_account.name}/blobServices/default/containers/${replace(lower(\"${local.resource_prefix}-files\"), \"_\", \"-\")}\"";

    #[test]
    fn generated_storage_assignments_preserve_each_permission_binding_scope() {
        let permissions = PermissionProfile::new().resource(
            "files",
            ["storage/data-write", "storage/trigger-management"],
        );
        let stack = Stack::new("azure-storage-scopes".to_string())
            .permission("app", permissions.clone())
            .management(ManagementPermissions::override_(permissions))
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureStorageAccount::new("default-storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("files".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceAccount::new("app-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let registry = TfRegistry::built_in();
        let module = generate_terraform_module(
            &stack,
            TerraformTarget::Azure,
            TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings: StackSettings::default(),
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("Azure Terraform module should render");
        let storage_module = module.get("files.tf").expect("storage resource module");

        assert_principal_assignment_scopes(
            storage_module,
            "azurerm_user_assigned_identity.app_sa.principal_id",
        );
        assert_principal_assignment_scopes(
            storage_module,
            "azurerm_user_assigned_identity.management.principal_id",
        );
    }

    #[test]
    fn setup_grants_remote_storage_only_to_remote_bindings_identity() {
        let stack = Stack::new("azure-remote-storage-scopes".to_string())
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureStorageAccount::new("default-storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_remote_access(
                Storage::new("files".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteBindings::new("access".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let registry = TfRegistry::built_in();
        let module = generate_terraform_module(
            &stack,
            TerraformTarget::Azure,
            TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings: StackSettings::default(),
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("Azure Terraform module should render");
        let storage_module = module.get("files.tf").expect("storage resource module");
        let assignments = storage_module
            .split("resource \"azurerm_role_assignment\"")
            .skip(1)
            .filter_map(|chunk| chunk.split_once("\n}\n").map(|(block, _)| block))
            .filter(|block| block.contains("azurerm_user_assigned_identity.access.principal_id"))
            .collect::<Vec<_>>();

        assert_eq!(assignments.len(), 1, "expected one container-scoped grant");
        assert!(assignments.iter().any(|block| block.contains(
            "blobServices/default/containers/${replace(lower(\"${local.resource_prefix}-files\"), \"_\", \"-\")}",
        )));
        assert!(!storage_module.contains("azurerm_user_assigned_identity.management.principal_id"));
    }

    fn assert_principal_assignment_scopes(rendered: &str, principal_id: &str) {
        let assignments = rendered
            .split("resource \"azurerm_role_assignment\"")
            .skip(1)
            .filter_map(|chunk| chunk.split_once("\n}\n").map(|(block, _)| block))
            .filter(|block| block.contains(principal_id))
            .collect::<Vec<_>>();
        assert_eq!(assignments.len(), 2, "expected both storage grants");

        let data_write = assignments
            .iter()
            .find(|block| block.contains(STORAGE_BLOB_DATA_CONTRIBUTOR_ROLE_ID))
            .expect("storage/data-write assignment");
        assert!(
            data_write.contains(STORAGE_CONTAINER_SCOPE),
            "storage/data-write must retain its generated container scope:\n{data_write}"
        );

        let trigger_management = assignments
            .iter()
            .find(|block| !block.contains(STORAGE_BLOB_DATA_CONTRIBUTOR_ROLE_ID))
            .expect("storage/trigger-management assignment");
        assert!(
            trigger_management.contains(RESOURCE_GROUP_SCOPE),
            "storage/trigger-management must retain its generated resource-group scope:\n{trigger_management}"
        );
        assert!(
            !trigger_management.contains("blobServices/default/containers"),
            "storage/trigger-management must not be narrowed to the blob container"
        );
    }
}
