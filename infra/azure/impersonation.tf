# -----------------------------------------------------------------------------
# Managed Identity for Impersonation
# (conditional on enable_impersonation)
# -----------------------------------------------------------------------------

resource "azurerm_user_assigned_identity" "impersonation" {
  count = var.enable_impersonation ? 1 : 0

  name                = "${var.name}-impersonation"
  location            = azurerm_resource_group.manager.location
  resource_group_name = azurerm_resource_group.manager.name
  tags                = local.common_tags
}

# Allow the manager identity to create tokens for the impersonation identity
# via federated credential or Managed Identity Operator role.
resource "azurerm_role_assignment" "impersonation_operator" {
  count = var.enable_impersonation ? 1 : 0

  scope                = azurerm_user_assigned_identity.impersonation[0].id
  role_definition_name = "Managed Identity Operator"
  principal_id         = azurerm_user_assigned_identity.manager.principal_id
}
