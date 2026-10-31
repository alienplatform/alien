# -----------------------------------------------------------------------------
# Table Storage + Blob Storage for Commands Store
# (conditional on enable_commands_store)
#
# Reuses the existing storage account; adds a table and blob container.
# -----------------------------------------------------------------------------

# Created through Azure Resource Manager: azurerm_storage_table also reads and
# writes the table ACL on the Table data plane, which Azure Table storage now
# rejects with 415 Unsupported Media Type, so it can neither create nor refresh
# the table.
resource "azapi_resource" "commands" {
  count = var.enable_commands_store ? 1 : 0

  type      = "Microsoft.Storage/storageAccounts/tableServices/tables@2024-01-01"
  name      = "commands"
  parent_id = "${azurerm_storage_account.manager.id}/tableServices/default"
  body = {
    properties = {}
  }
}

# Forget the azurerm_storage_table of earlier versions of this module without
# deleting the table. The README's upgrade steps import it as azapi_resource.
removed {
  from = azurerm_storage_table.commands

  lifecycle {
    destroy = false
  }
}

resource "azurerm_storage_container" "commands" {
  count = var.enable_commands_store ? 1 : 0

  name                  = "commands-store"
  storage_account_name  = azurerm_storage_account.manager.name
  container_access_type = "private"
}

# Grant the managed identity access to table and blob data
resource "azurerm_role_assignment" "commands_table" {
  count = var.enable_commands_store ? 1 : 0

  scope                = azurerm_storage_account.manager.id
  role_definition_name = "Storage Table Data Contributor"
  principal_id         = azurerm_user_assigned_identity.manager.principal_id
}

resource "azurerm_role_assignment" "commands_blob" {
  count = var.enable_commands_store ? 1 : 0

  scope                = azurerm_storage_account.manager.id
  role_definition_name = "Storage Blob Data Contributor"
  principal_id         = azurerm_user_assigned_identity.manager.principal_id
}
