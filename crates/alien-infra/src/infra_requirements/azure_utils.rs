use crate::error::{ErrorData, Result};
use alien_core::{
    AzureContainerAppsEnvironment, AzureContainerAppsEnvironmentOutputs, AzureResourceGroup,
    AzureResourceGroupOutputs, AzureStorageAccount, AzureStorageAccountOutputs, ResourceRef,
    StackState,
};
use alien_error::Context;

/// Helper to extract the Azure Resource Group name from the dependency outputs.
/// This is used by all Azure resource controllers.
pub fn get_resource_group_name(state: &StackState) -> Result<String> {
    let rg_ref = ResourceRef::new(AzureResourceGroup::RESOURCE_TYPE, "default-resource-group");
    let rg_outputs = state
        .get_resource_outputs::<AzureResourceGroupOutputs>(rg_ref.id())
        .context(ErrorData::DependencyNotReady {
            resource_id: "default-resource-group".to_string(),
            dependency_id: rg_ref.id().to_string(),
        })?;
    Ok(rg_outputs.name.clone())
}

/// Helper to extract the Azure Storage Account name from the dependency outputs.
/// This is used by Azure resource controllers that depend on a storage account.
pub fn get_storage_account_name(state: &StackState) -> Result<String> {
    let sa_ref = ResourceRef::new(
        AzureStorageAccount::RESOURCE_TYPE,
        "default-storage-account",
    );
    let sa_outputs = state
        .get_resource_outputs::<AzureStorageAccountOutputs>(sa_ref.id())
        .context(ErrorData::DependencyNotReady {
            resource_id: "default-storage-account".to_string(),
            dependency_id: sa_ref.id().to_string(),
        })?;
    Ok(sa_outputs.account_name.clone())
}

/// Helper to extract Azure Container Apps Environment outputs from the dependency outputs.
pub fn get_container_apps_environment_outputs(
    state: &StackState,
) -> Result<&AzureContainerAppsEnvironmentOutputs> {
    let env_ref = ResourceRef::new(
        AzureContainerAppsEnvironment::RESOURCE_TYPE,
        "default-container-env",
    );
    state
        .get_resource_outputs::<AzureContainerAppsEnvironmentOutputs>(env_ref.id())
        .context(ErrorData::DependencyNotReady {
            resource_id: "default-container-env".to_string(),
            dependency_id: env_ref.id().to_string(),
        })
}

/// Helper to extract the Azure Container Apps Environment name from the dependency outputs.
/// This is used by Azure resource controllers that depend on a container apps environment.
pub fn get_container_apps_environment_name(state: &StackState) -> Result<String> {
    Ok(get_container_apps_environment_outputs(state)?
        .environment_name
        .clone())
}

/// Helper to extract the full Azure resource ID of the Container Apps Environment.
/// This should be used instead of constructing the resource ID manually, since the
/// environment may be in a different resource group than the stack (e.g., when using
/// external bindings for a pre-provisioned shared environment).
pub fn get_container_apps_environment_resource_id(state: &StackState) -> Result<String> {
    Ok(get_container_apps_environment_outputs(state)?
        .resource_id
        .clone())
}

/// Helper to extract the resource group name of the Container Apps Environment.
/// This may differ from the stack's default resource group when the environment
/// is externally provisioned in a separate resource group.
pub fn get_container_apps_environment_resource_group(state: &StackState) -> Result<String> {
    Ok(get_container_apps_environment_outputs(state)?
        .resource_group_name
        .clone())
}

pub(crate) fn azure_resource_group_resource_id(
    subscription_id: &str,
    resource_group: &str,
) -> String {
    format!("/subscriptions/{subscription_id}/resourceGroups/{resource_group}")
}

pub(crate) fn azure_storage_account_resource_id(
    subscription_id: &str,
    resource_group: &str,
    account_name: &str,
) -> String {
    format!(
        "{}/providers/Microsoft.Storage/storageAccounts/{account_name}",
        azure_resource_group_resource_id(subscription_id, resource_group)
    )
}

pub(crate) fn azure_container_apps_environment_resource_id(
    subscription_id: &str,
    resource_group: &str,
    environment_name: &str,
) -> String {
    format!(
        "{}/providers/Microsoft.App/managedEnvironments/{environment_name}",
        azure_resource_group_resource_id(subscription_id, resource_group)
    )
}

pub(crate) fn azure_service_bus_namespace_resource_id(
    subscription_id: &str,
    resource_group: &str,
    namespace_name: &str,
) -> String {
    format!(
        "{}/providers/Microsoft.ServiceBus/namespaces/{namespace_name}",
        azure_resource_group_resource_id(subscription_id, resource_group)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azure_resource_ids_match_arm_resource_id_format() {
        let subscription_id = "sub-123";
        let resource_group = "alien-e2e-rg";

        assert_eq!(
            azure_resource_group_resource_id(subscription_id, resource_group),
            "/subscriptions/sub-123/resourceGroups/alien-e2e-rg"
        );
        assert_eq!(
            azure_storage_account_resource_id(subscription_id, resource_group, "alienstorage"),
            "/subscriptions/sub-123/resourceGroups/alien-e2e-rg/providers/Microsoft.Storage/storageAccounts/alienstorage"
        );
        assert_eq!(
            azure_container_apps_environment_resource_id(
                subscription_id,
                resource_group,
                "alien-env"
            ),
            "/subscriptions/sub-123/resourceGroups/alien-e2e-rg/providers/Microsoft.App/managedEnvironments/alien-env"
        );
        assert_eq!(
            azure_service_bus_namespace_resource_id(subscription_id, resource_group, "alien-bus"),
            "/subscriptions/sub-123/resourceGroups/alien-e2e-rg/providers/Microsoft.ServiceBus/namespaces/alien-bus"
        );
    }
}
