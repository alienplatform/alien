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

/// Helper to extract the Azure Container Apps Environment name from the dependency outputs.
/// This is used by Azure resource controllers that depend on a container apps environment.
pub fn get_container_apps_environment_name(state: &StackState) -> Result<String> {
    let env_ref = ResourceRef::new(
        AzureContainerAppsEnvironment::RESOURCE_TYPE,
        "default-container-env",
    );
    let env_outputs = state
        .get_resource_outputs::<AzureContainerAppsEnvironmentOutputs>(env_ref.id())
        .context(ErrorData::DependencyNotReady {
            resource_id: "default-container-env".to_string(),
            dependency_id: env_ref.id().to_string(),
        })?;
    Ok(env_outputs.environment_name.clone())
}
