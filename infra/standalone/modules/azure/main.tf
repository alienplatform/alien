terraform {
  required_providers {
    azurerm = {
      source                = "hashicorp/azurerm"
      version               = "~> 3.0"
      configuration_aliases = [azurerm.management]
    }
    azuread = {
      source  = "hashicorp/azuread"
      version = "~> 3.0"
    }
    random = { source = "hashicorp/random", version = "~> 3.0" }
  }
}

resource "random_id" "suffix" {
  byte_length = 4
}

# ── Management: Resource group ────────────────────────────────────────────────

resource "azurerm_resource_group" "test" {
  provider = azurerm.management
  name     = "alien-test-${random_id.suffix.hex}"
  location = var.management_region
}

# ── Management: Storage account + blob container ─────────────────────────────

resource "azurerm_storage_account" "test" {
  provider                 = azurerm.management
  name                     = "alientest${random_id.suffix.hex}"
  resource_group_name      = azurerm_resource_group.test.name
  location                 = azurerm_resource_group.test.location
  account_tier             = "Standard"
  account_replication_type = "LRS"
}

resource "azurerm_storage_container" "test" {
  provider              = azurerm.management
  name                  = "alien-test"
  storage_account_name  = azurerm_storage_account.test.name
  container_access_type = "private"
}

# ── Management: Container Registry ───────────────────────────────────────────

resource "azurerm_container_registry" "test" {
  provider            = azurerm.management
  name                = "alientest${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.test.name
  location            = azurerm_resource_group.test.location
  sku                 = "Basic"
  admin_enabled       = true
}

# ── Management: Container Apps environment ────────────────────────────────────

resource "azurerm_container_app_environment" "test" {
  provider            = azurerm.management
  name                = "alien-test-${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.test.name
  location            = azurerm_resource_group.test.location
}

# ── Management: RBAC for management service principal ────────────────────────

resource "azurerm_role_assignment" "manager_storage_blob" {
  provider             = azurerm.management
  scope                = azurerm_storage_account.test.id
  role_definition_name = "Storage Blob Data Contributor"
  principal_id         = data.azurerm_client_config.management.object_id
}

resource "azurerm_role_assignment" "manager_acr_push" {
  provider             = azurerm.management
  scope                = azurerm_container_registry.test.id
  role_definition_name = "AcrPush"
  principal_id         = data.azurerm_client_config.management.object_id
}

data "azurerm_client_config" "management" {
  provider = azurerm.management
}

# ── Management: Service Principal ─────────────────────────────────────────
# The management SP is the identity that customers trust (used for Lighthouse
# cross-subscription access). It must NOT have AcrPush/AcrPull — those belong
# to the execution identity (the Terraform SP above).

resource "azuread_application" "manager" {
  display_name = "alien-test-manager"
  owners       = [data.azurerm_client_config.management.object_id]
}

resource "azuread_service_principal" "manager" {
  client_id = azuread_application.manager.client_id
  owners    = [data.azurerm_client_config.management.object_id]
}

resource "azuread_application_password" "manager" {
  application_id = azuread_application.manager.id
  display_name   = "alien-test"
}

resource "azurerm_role_assignment" "mgmt_sp_contributor" {
  provider             = azurerm.management
  scope                = "/subscriptions/${var.management_subscription_id}"
  role_definition_name = "Contributor"
  principal_id         = azuread_service_principal.manager.object_id
}

resource "azurerm_role_assignment" "mgmt_sp_user_access_admin" {
  provider             = azurerm.management
  scope                = "/subscriptions/${var.management_subscription_id}"
  role_definition_name = "User Access Administrator"
  principal_id         = azuread_service_principal.manager.object_id
}

