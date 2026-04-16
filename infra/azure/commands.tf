# -----------------------------------------------------------------------------
# Table Storage + Blob Storage for Commands Store
# (conditional on enable_commands_store)
#
# Reuses the existing storage account; adds a table and blob container.
# -----------------------------------------------------------------------------

resource "azurerm_storage_table" "commands" {
  count = var.enable_commands_store ? 1 : 0

  name                 = "commands"
  storage_account_name = azurerm_storage_account.manager.name
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
