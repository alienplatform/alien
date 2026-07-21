use std::collections::BTreeSet;

use alien_azure_clients::authorization::Scope;
use alien_azure_clients::models::authorization_role_definitions::{
    Permission, RoleDefinition, RoleDefinitionProperties,
};
use alien_core::{BindingValue, KubernetesCluster, ResourceLifecycle, Storage, StorageBinding};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_permissions::{
    generators::{
        dedupe_azure_role_bindings, AzureCustomRole, AzureGrantPlan, AzureRoleDefinitionRef,
        AzureRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget, PermissionContext,
};
use uuid::Uuid;

use super::azure::{
    generate_stack_management_grant_plan, resource_role_definition_key,
    AzureRemoteStackManagementController,
};
use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils;

pub(super) const REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID: &str = "storage/remote-data-write";

pub(super) fn desired_remote_storage_scopes(
    ctx: &ResourceControllerContext<'_>,
) -> Result<Vec<String>> {
    let mut scopes = generate_management_grant_plan(ctx)?
        .bindings
        .into_iter()
        .filter(|binding| binding.permission_set_id == REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID)
        .map(|binding| binding.scope)
        .collect::<Vec<_>>();
    scopes.sort_unstable();
    scopes.dedup();
    Ok(scopes)
}

pub(super) fn custom_roles_for_combined_management_role(
    grant_plan: AzureGrantPlan,
) -> Vec<AzureCustomRole> {
    let resource_scoped_role_keys: BTreeSet<_> = grant_plan
        .bindings
        .iter()
        .filter(|binding| binding.permission_set_id == REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID)
        .filter_map(|binding| match &binding.role_definition {
            AzureRoleDefinitionRef::Custom { key } => Some(key.clone()),
            AzureRoleDefinitionRef::Predefined { .. } => None,
        })
        .collect();

    grant_plan
        .custom_roles
        .into_iter()
        .filter(|custom_role| !resource_scoped_role_keys.contains(&custom_role.key))
        .collect()
}

pub(super) fn generate_management_grant_plan(
    ctx: &ResourceControllerContext<'_>,
) -> Result<AzureGrantPlan> {
    let management_permissions = ctx.desired_stack.management();
    let management_profile = management_permissions.profile().ok_or_else(|| {
        AlienError::new(ErrorData::InfrastructureError {
            message: "Management permissions not configured. Required for remote stack management."
                .to_string(),
            operation: Some("generate_management_role_definition".to_string()),
            resource_id: Some("management".to_string()),
        })
    })?;

    let azure_config = ctx.get_azure_config()?;
    let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
    let mut custom_roles = Vec::new();
    let mut bindings = Vec::new();

    let permission_context = PermissionContext::new()
        .with_subscription_id(azure_config.subscription_id.clone())
        .with_resource_group(resource_group_name.clone())
        .with_stack_prefix(ctx.resource_prefix.to_string())
        .with_managing_subscription_id(azure_config.subscription_id.clone())
        .with_managing_resource_group(resource_group_name.clone());
    let permission_context = match ctx.deployment_name_for_metadata() {
        Some(deployment_name) => {
            permission_context.with_deployment_name(deployment_name.to_string())
        }
        None => permission_context,
    };

    let generator = AzureRuntimePermissionsGenerator::new();
    let grant_plan = generate_stack_management_grant_plan(management_profile, &permission_context)?;
    custom_roles.extend(grant_plan.custom_roles);
    bindings.extend(grant_plan.bindings);

    for (resource_id, permission_set_refs) in management_profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
    {
        let Some(resource_entry) = ctx.desired_stack.resources.get(resource_id) else {
            continue;
        };
        let remote_storage = is_remote_frozen_storage(resource_entry);
        let permission_context = if let Some(cluster) =
            resource_entry.config.downcast_ref::<KubernetesCluster>()
        {
            ResourcePermissionsHelper::azure_kubernetes_cluster_permission_context(ctx, cluster)?
        } else if remote_storage {
            remote_storage_permission_context(ctx, resource_id)?
        } else {
            continue;
        };

        for permission_set_ref in permission_set_refs {
            if remote_storage && permission_set_ref.id().ends_with("/provision") {
                continue;
            }
            let Some(permission_set) =
                permission_set_ref.resolve(|name| get_permission_set(name).cloned())
            else {
                tracing::warn!(
                    permission_set_id = %permission_set_ref.id(),
                    "Management permission set not found, skipping"
                );
                continue;
            };
            if permission_set.platforms.azure.is_none() {
                continue;
            }

            let grant_plan = generator
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate Azure resource-scoped role definition for permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("generate_management_grant_plan".to_string()),
                    resource_id: Some(resource_id.clone()),
                })?;

            custom_roles.extend(grant_plan.custom_roles);
            bindings.extend(grant_plan.bindings);
        }
    }

    Ok(AzureGrantPlan {
        custom_roles,
        bindings: dedupe_azure_role_bindings(bindings),
    })
}

pub(super) async fn delete_resource_role_definitions(
    controller: &mut AzureRemoteStackManagementController,
    client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
    resource_group_name: &str,
    config_id: &str,
) -> Result<()> {
    for role_definition_id in controller.resource_role_definition_ids.values() {
        let role_definition_uuid = role_definition_id
            .split('/')
            .next_back()
            .unwrap_or(role_definition_id);
        let scope =
            super::azure::role_definition_scope_from_id(role_definition_id, resource_group_name);
        match client
            .delete_role_definition(&scope, role_definition_uuid.to_string())
            .await
        {
            Ok(_) => {
                tracing::info!(role_definition_id = %role_definition_id, "Exact-scope management role definition deleted");
            }
            Err(error)
                if matches!(
                    &error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                tracing::info!(role_definition_id = %role_definition_id, "Exact-scope management role definition already absent");
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to delete exact-scope management role definition '{}'",
                        role_definition_id
                    ),
                    resource_id: Some(config_id.to_string()),
                }));
            }
        }
    }
    controller.resource_role_definition_ids.clear();
    Ok(())
}

pub(super) async fn create_remote_storage_role_definitions(
    controller: &mut AzureRemoteStackManagementController,
    ctx: &ResourceControllerContext<'_>,
    client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
    azure_cfg: &alien_azure_clients::AzureClientConfig,
    resource_group_name: &str,
    config_id: &str,
) -> Result<()> {
    let grant_plan = generate_management_grant_plan(ctx)?;
    controller.resource_role_definition_ids.clear();
    let definition_scope = Scope::ResourceGroup {
        resource_group_name: resource_group_name.to_string(),
    };
    let assignable_scope = definition_scope.to_resource_id_string(azure_cfg);

    for binding in grant_plan
        .bindings
        .iter()
        .filter(|binding| binding.permission_set_id == REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID)
    {
        let AzureRoleDefinitionRef::Custom { key } = &binding.role_definition else {
            continue;
        };
        let state_key = resource_role_definition_key(key, &binding.scope);
        if controller
            .resource_role_definition_ids
            .contains_key(&state_key)
        {
            continue;
        }
        let custom_role = grant_plan
            .custom_roles
            .iter()
            .find(|custom_role| {
                custom_role.key == *key
                    && custom_role
                        .role_definition
                        .assignable_scopes
                        .contains(&binding.scope)
            })
            .ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: format!(
                        "Missing exact-scope custom role for '{}' at '{}'",
                        binding.permission_set_id, binding.scope
                    ),
                    operation: Some("create_management_role_definition".to_string()),
                    resource_id: Some(config_id.to_string()),
                })
            })?;
        let role_definition_uuid = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!(
                "deployment:azure:mgmt-resource-role-def:{}:{}",
                ctx.resource_prefix, state_key
            )
            .as_bytes(),
        )
        .to_string();
        let short_id = role_definition_uuid.split('-').next().unwrap_or("remote");
        let role = &custom_role.role_definition;
        let role_definition = RoleDefinition {
            properties: Some(RoleDefinitionProperties {
                role_name: Some(format!(
                    "{}-remote-storage-data-write-{}",
                    ctx.resource_prefix, short_id
                )),
                description: Some(role.description.clone()),
                type_: Some("CustomRole".to_string()),
                permissions: vec![Permission {
                    actions: role.actions.clone(),
                    not_actions: role.not_actions.clone(),
                    data_actions: role.data_actions.clone(),
                    not_data_actions: role.not_data_actions.clone(),
                }],
                assignable_scopes: vec![assignable_scope.clone()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let created = client
            .create_or_update_role_definition(
                &definition_scope,
                role_definition_uuid,
                &role_definition,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create exact-scope role definition for remote Storage at '{}'",
                    binding.scope
                ),
                resource_id: Some(config_id.to_string()),
            })?;
        let role_definition_id = created.id.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Created remote Storage role definition missing ID".to_string(),
                operation: Some("create_management_role_definition".to_string()),
                resource_id: Some(config_id.to_string()),
            })
        })?;
        controller
            .resource_role_definition_ids
            .insert(state_key, role_definition_id);
    }

    Ok(())
}

fn is_remote_frozen_storage(resource_entry: &alien_core::ResourceEntry) -> bool {
    resource_entry.lifecycle == ResourceLifecycle::Frozen
        && resource_entry.remote_access
        && resource_entry.config.downcast_ref::<Storage>().is_some()
}

fn remote_storage_permission_context(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<PermissionContext> {
    let (storage_account_name, container_name) = match remote_storage_binding(ctx, resource_id)? {
        Some(StorageBinding::Blob(binding)) => (
            concrete_storage_binding_value(
                binding.account_name,
                resource_id,
                "accountName",
                "Azure Blob Storage",
            )?,
            concrete_storage_binding_value(
                binding.container_name,
                resource_id,
                "containerName",
                "Azure Blob Storage",
            )?,
        ),
        Some(other) => {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Remote Storage resource '{resource_id}' must use a Blob binding on Azure, got {other:?}"
                ),
                resource_id: Some(resource_id.to_string()),
            }));
        }
        None => (
            azure_utils::get_storage_account_name(ctx.state)?,
            format!("{}-{}", ctx.resource_prefix, resource_id)
                .to_lowercase()
                .replace('_', "-"),
        ),
    };

    Ok(
        ResourcePermissionsHelper::build_azure_permission_context(ctx, &container_name)?
            .with_resource_id(resource_id.to_string())
            .with_storage_account_name(storage_account_name),
    )
}

fn remote_storage_binding(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<Option<StorageBinding>> {
    super::ensure_setup_owned_remote_storage(ctx, resource_id)?;

    let Some(binding) = ctx
        .state
        .resource(resource_id)
        .and_then(|state| state.remote_binding_params.as_ref())
    else {
        return Ok(None);
    };

    serde_json::from_value(binding.clone())
        .into_alien_error()
        .context(ErrorData::ResourceConfigInvalid {
            message: format!(
                "Remote Storage resource '{resource_id}' has invalid binding parameters"
            ),
            resource_id: Some(resource_id.to_string()),
        })
        .map(Some)
}

fn concrete_storage_binding_value(
    value: BindingValue<String>,
    resource_id: &str,
    field_name: &str,
    provider: &str,
) -> Result<String> {
    match value {
        BindingValue::Value(value) => Ok(value),
        BindingValue::Expression(_) | BindingValue::SecretRef { .. } => {
            Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Remote Storage resource '{resource_id}' requires a concrete {provider} {field_name}"
                ),
                resource_id: Some(resource_id.to_string()),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use alien_core::{Resource, ResourceEntry};

    use super::*;

    fn permission_context() -> PermissionContext {
        PermissionContext::new()
            .with_subscription_id("sub-123".to_string())
            .with_resource_group("rg-123".to_string())
            .with_stack_prefix("e2e-01-azcr".to_string())
            .with_managing_subscription_id("sub-123".to_string())
            .with_managing_resource_group("rg-123".to_string())
    }

    #[test]
    fn remote_storage_management_is_limited_to_opted_in_frozen_storage() {
        let entry = |lifecycle, remote_access| ResourceEntry {
            config: Resource::new(Storage::new("archive".to_string()).build()),
            lifecycle,
            dependencies: Vec::new(),
            remote_access,
        };

        assert!(is_remote_frozen_storage(&entry(
            ResourceLifecycle::Frozen,
            true
        )));
        assert!(!is_remote_frozen_storage(&entry(
            ResourceLifecycle::Frozen,
            false
        )));
        assert!(!is_remote_frozen_storage(&entry(
            ResourceLifecycle::Live,
            true
        )));
    }

    #[test]
    fn remote_storage_management_grants_exact_container_data_and_account_delegation_key() {
        let context = permission_context()
            .with_resource_id("archive".to_string())
            .with_resource_name("setup-owned-archive-container".to_string())
            .with_storage_account_name("setupownedstorageaccount".to_string());
        let permission_set = get_permission_set(REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID)
            .expect("remote storage permission set");

        let grant_plan = AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
            .unwrap();

        let account_scope = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Storage/storageAccounts/setupownedstorageaccount";
        let container_scope = format!(
            "{account_scope}/blobServices/default/containers/setup-owned-archive-container"
        );
        assert_eq!(grant_plan.bindings.len(), 2);
        assert!(grant_plan
            .bindings
            .iter()
            .any(|binding| binding.scope == account_scope));
        assert!(grant_plan
            .bindings
            .iter()
            .any(|binding| binding.scope == container_scope));

        let delegation_role = grant_plan
            .custom_roles
            .iter()
            .find(|role| role.role_definition.assignable_scopes == [account_scope])
            .expect("account-scoped delegation-key role");
        assert_eq!(
            delegation_role.role_definition.actions,
            ["Microsoft.Storage/storageAccounts/blobServices/generateUserDelegationKey/action"]
        );
        assert!(delegation_role.role_definition.data_actions.is_empty());

        let data_role = grant_plan
            .custom_roles
            .iter()
            .find(|role| role.role_definition.assignable_scopes == [container_scope.as_str()])
            .expect("container-scoped blob data role");
        assert!(data_role.role_definition.actions.is_empty());
        assert!(data_role
            .role_definition
            .data_actions
            .iter()
            .all(|action| action.contains("/containers/blobs/")));
        assert!(
            custom_roles_for_combined_management_role(grant_plan).is_empty(),
            "remote Storage roles must not be merged into the RG-scoped management role"
        );
    }

    #[test]
    fn delegation_key_assignment_is_deduped_per_storage_account() {
        let permission_set = get_permission_set(REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID)
            .expect("remote storage permission set");
        let generator = AzureRuntimePermissionsGenerator::new();
        let mut bindings = Vec::new();
        for container in ["container-a", "container-b"] {
            let context = permission_context()
                .with_resource_id(container.to_string())
                .with_resource_name(container.to_string())
                .with_storage_account_name("sharedstorageaccount".to_string());
            bindings.extend(
                generator
                    .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
                    .unwrap()
                    .bindings,
            );
        }

        let bindings = dedupe_azure_role_bindings(bindings);
        let account_scope = "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Storage/storageAccounts/sharedstorageaccount";
        assert_eq!(
            bindings
                .iter()
                .filter(|binding| binding.scope == account_scope)
                .count(),
            1,
        );
        assert_eq!(
            bindings
                .iter()
                .filter(|binding| binding.scope.contains("/containers/"))
                .count(),
            2,
        );
    }
}
