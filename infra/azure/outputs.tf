# -----------------------------------------------------------------------------
# Structured config values for alien-manager.toml
# -----------------------------------------------------------------------------

output "config_values" {
  description = "Values for alien-manager.toml sections (artifact-registry, commands, impersonation)"
  value = {
    artifact_registry = var.enable_artifact_registry ? {
      service           = "acr"
      registryName      = azurerm_container_registry.artifacts[0].name
      resourceGroupName = azurerm_resource_group.manager.name
    } : null

    commands = var.enable_commands_store ? {
      kv = {
        service           = "tablestorage"
        resourceGroupName = azurerm_resource_group.manager.name
        accountName       = azurerm_storage_account.manager.name
        tableName         = azurerm_storage_table.commands[0].name
      }
      storage = {
        service       = "blob"
        accountName   = azurerm_storage_account.manager.name
        containerName = azurerm_storage_container.commands[0].name
      }
    } : null

    impersonation = var.enable_impersonation ? {
      service    = "azuremanagedidentity"
      clientId   = azurerm_user_assigned_identity.impersonation[0].client_id
      resourceId = azurerm_user_assigned_identity.impersonation[0].id
    } : null
  }
}

# -----------------------------------------------------------------------------
# Individual resource outputs
# -----------------------------------------------------------------------------

output "resource_group_name" {
  description = "Name of the resource group"
  value       = azurerm_resource_group.manager.name
}

output "managed_identity_id" {
  description = "ID of the manager managed identity (attach to your compute)"
  value       = azurerm_user_assigned_identity.manager.id
}

output "managed_identity_client_id" {
  description = "Client ID of the manager managed identity"
  value       = azurerm_user_assigned_identity.manager.client_id
}

output "acr_login_server" {
  description = "Login server URL of the ACR (if artifact registry is enabled)"
  value       = var.enable_artifact_registry ? azurerm_container_registry.artifacts[0].login_server : ""
}

output "storage_account_name" {
  description = "Name of the storage account"
  value       = azurerm_storage_account.manager.name
}
