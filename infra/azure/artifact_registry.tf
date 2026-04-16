# -----------------------------------------------------------------------------
# Azure Container Registry (conditional on enable_artifact_registry)
# -----------------------------------------------------------------------------

resource "azurerm_container_registry" "artifacts" {
  count = var.enable_artifact_registry ? 1 : 0

  # ACR names: alphanumeric, 5-50 chars, globally unique
  name                = substr(replace("${var.name}artifacts${random_id.suffix.hex}", "/[^a-zA-Z0-9]/", ""), 0, 50)
  resource_group_name = azurerm_resource_group.manager.name
  location            = azurerm_resource_group.manager.location
  sku                 = "Basic"
  admin_enabled       = false
  tags                = local.common_tags
}

# Allow the managed identity to push images
resource "azurerm_role_assignment" "acr_push" {
  count = var.enable_artifact_registry ? 1 : 0

  scope                = azurerm_container_registry.artifacts[0].id
  role_definition_name = "AcrPush"
  principal_id         = azurerm_user_assigned_identity.manager.principal_id
}

# Allow the managed identity to pull images
resource "azurerm_role_assignment" "acr_pull" {
  count = var.enable_artifact_registry ? 1 : 0

  scope                = azurerm_container_registry.artifacts[0].id
  role_definition_name = "AcrPull"
  principal_id         = azurerm_user_assigned_identity.manager.principal_id
}
