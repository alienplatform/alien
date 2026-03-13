terraform {
  required_providers {
    azurerm = { source = "hashicorp/azurerm",  version = "~> 3.0" }
    docker  = { source = "kreuzwerker/docker", version = "~> 3.0" }
    random  = { source = "hashicorp/random",   version = "~> 3.0" }
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

# ── Target: Service principal (via separate subscription) ─────────────────────

data "azurerm_client_config" "management" {
  provider = azurerm.management
}

data "azurerm_client_config" "target" {
  provider = azurerm.target
}

# Grant target SP access to Container Registry in management subscription
resource "azurerm_role_assignment" "target_acr_pull" {
  provider             = azurerm.management
  scope                = azurerm_container_registry.test.id
  role_definition_name = "AcrPull"
  principal_id         = data.azurerm_client_config.target.object_id
}

resource "azurerm_role_assignment" "target_containerapp" {
  provider             = azurerm.management
  scope                = azurerm_container_app_environment.test.id
  role_definition_name = "Contributor"
  principal_id         = data.azurerm_client_config.target.object_id
}

# ── Docker: build and push http-server image ──────────────────────────────────

resource "docker_registry_image" "http_server" {
  name          = "${azurerm_container_registry.test.login_server}/http-server:latest"
  keep_remotely = true

  build {
    context  = "${path.root}/images/http-server"
    platform = "linux/amd64"

    auth_config {
      host_name = azurerm_container_registry.test.login_server
      user_name = azurerm_container_registry.test.admin_username
      password  = azurerm_container_registry.test.admin_password
    }
  }

  depends_on = [azurerm_container_registry.test]
}
