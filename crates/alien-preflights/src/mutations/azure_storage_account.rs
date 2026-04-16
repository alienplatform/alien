//! Azure Storage Account mutation that adds the required storage account for storage/kv resources.

use crate::error::Result;
use crate::StackMutation;
use alien_core::{
    AzureStorageAccount, DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle, ResourceRef,
    Stack, StackState,
};
use async_trait::async_trait;
use tracing::{debug, info};

/// Mutation that adds AzureStorageAccount resource for storage and kv resources.
///
/// Storage resources on Azure use blob storage, and KV resources use table storage,
/// both of which require a storage account to be created first.
pub struct AzureStorageAccountMutation;

#[async_trait]
impl StackMutation for AzureStorageAccountMutation {
    fn description(&self) -> &'static str {
        "Add Azure Storage Account required by Storage resources"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Only add for Azure platform
        if stack_state.platform != Platform::Azure {
            return false;
        }

        // Check if we have storage or kv resources that need Storage Account
        let has_storage_resources = stack.resources.iter().any(|(_, entry)| {
            let resource_type = entry.config.resource_type();
            matches!(resource_type.as_ref(), "storage" | "kv")
        });

        if !has_storage_resources {
            return false;
        }

        // Check if AzureStorageAccount already exists
        let account_id = "default-storage-account";
        !stack.resources.iter().any(|(id, _)| id == account_id)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding AzureStorageAccount resource for Azure storage/kv resources");

        let account_id = "default-storage-account";

        // Create the AzureStorageAccount resource
        let storage_account = AzureStorageAccount::new(account_id.to_string()).build();

        // Dependencies: resource group and Microsoft.Storage service activation
        let dependencies = vec![
            ResourceRef::new(
                alien_core::AzureResourceGroup::RESOURCE_TYPE,
                "default-resource-group",
            ),
            ResourceRef::new(
                alien_core::ServiceActivation::RESOURCE_TYPE,
                "enable-storage",
            ),
        ];

        // Add it to the stack as a frozen resource
        let account_entry = ResourceEntry {
            config: alien_core::Resource::new(storage_account),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies,
            remote_access: false,
        };

        stack
            .resources
            .insert(account_id.to_string(), account_entry);

        debug!("Added AzureStorageAccount resource '{}'", account_id);

        Ok(stack)
    }
}
