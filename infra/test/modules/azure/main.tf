terraform {
  required_providers {
    azurerm = {
      source                = "hashicorp/azurerm"
      version               = "~> 3.0"
      configuration_aliases = [azurerm.management, azurerm.target]
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
# admin_enabled = false matches production. Use separate ACR pull/push SPs instead.

resource "azurerm_container_registry" "test" {
  provider            = azurerm.management
  name                = "alientest${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.test.name
  location            = azurerm_resource_group.test.location
  sku                 = "Basic"
  admin_enabled       = false
}

# ── Management: Container Apps environment ────────────────────────────────────

resource "azurerm_container_app_environment" "test" {
  provider            = azurerm.management
  name                = "alien-test-${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.test.name
  location            = azurerm_resource_group.test.location
}

# ── Target: Shared Container Apps Environment ─────────────────────────────────
# Pre-provisioned in the target subscription so e2e tests can share a single
# environment instead of each test creating its own (Azure limits to 20 per
# region per subscription). Tests inject this as an external binding.

resource "azurerm_resource_group" "shared_target" {
  provider = azurerm.target
  name     = "alien-e2e-shared-${random_id.suffix.hex}"
  location = var.management_region
}

resource "azurerm_container_app_environment" "shared_target" {
  provider            = azurerm.target
  name                = "alien-e2e-shared-${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.shared_target.name
  location            = azurerm_resource_group.shared_target.location
}

# Custom role that allows joining the shared environment. Created once here;
# the test harness assigns it to each deployment's management UAMI after
# InitialSetup (when the UAMI exists but before Provisioning needs it).
resource "azurerm_role_definition" "shared_env_join" {
  provider = azurerm.target
  name     = "alien-e2e-env-join-${random_id.suffix.hex}"
  scope    = azurerm_container_app_environment.shared_target.id

  permissions {
    actions = [
      "Microsoft.App/managedEnvironments/read",
      "Microsoft.App/managedEnvironments/join/action",
    ]
  }

  assignable_scopes = [
    azurerm_container_app_environment.shared_target.id,
  ]
}

# ── Management: RBAC for Terraform execution identity ─────────────────────────
# The Terraform SP (current authenticated principal) needs blob + ACR push
# access for building and pushing images during test setup.

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
# Multi-tenant SP used as a local-dev fallback for cross-tenant client_credentials.
# In production/CI, OIDC token exchange replaces this SP entirely.

resource "azuread_application" "manager" {
  display_name     = "alien-test-manager"
  sign_in_audience = "AzureADMultipleOrgs"
  owners           = [data.azurerm_client_config.management.object_id]
}

resource "azuread_service_principal" "manager" {
  client_id = azuread_application.manager.client_id
  owners    = [data.azurerm_client_config.management.object_id]
}

resource "azuread_application_password" "manager" {
  application_id = azuread_application.manager.id
  display_name   = "alien-test"
}

# GitHub Actions OIDC federation — lets the Terraform execution SP
# (alien-terraform-bootstrap) authenticate using GitHub OIDC tokens in CI.
# Required for ACR scope map creation and other management-side operations.
# The FIC must be on the app that AZURE_MANAGEMENT_CLIENT_ID points to.
data "azuread_application" "terraform_bootstrap" {
  client_id = var.management_client_id
}

resource "azuread_application_federated_identity_credential" "github_main" {
  application_id = data.azuread_application.terraform_bootstrap.id
  display_name   = "github-actions-main"
  issuer         = "https://token.actions.githubusercontent.com"
  subject        = "repo:alienplatform/alien:ref:refs/heads/main"
  audiences      = ["api://AzureADTokenExchange"]
}

resource "azuread_application_federated_identity_credential" "github_pr" {
  application_id = data.azuread_application.terraform_bootstrap.id
  display_name   = "github-actions-pr"
  issuer         = "https://token.actions.githubusercontent.com"
  subject        = "repo:alienplatform/alien:pull_request"
  audiences      = ["api://AzureADTokenExchange"]
}

# Scoped to the test resource group instead of subscription-wide Contributor
resource "azurerm_role_assignment" "mgmt_sp_contributor" {
  provider             = azurerm.management
  scope                = azurerm_resource_group.test.id
  role_definition_name = "Contributor"
  principal_id         = azuread_service_principal.manager.object_id
}

resource "azurerm_role_assignment" "mgmt_sp_user_access_admin" {
  provider             = azurerm.management
  scope                = azurerm_resource_group.test.id
  role_definition_name = "User Access Administrator"
  principal_id         = azuread_service_principal.manager.object_id
}

# ── ACR Pull/Push SPs (matching production) ──────────────────────────────────
# Separate service principals for ACR operations, scoped to the ACR resource.

resource "azuread_application" "acr_pull" {
  display_name = "alien-test-acr-pull"
  owners       = [data.azurerm_client_config.management.object_id]
}

resource "azuread_service_principal" "acr_pull" {
  client_id = azuread_application.acr_pull.client_id
  owners    = [data.azurerm_client_config.management.object_id]
}

resource "azuread_application_password" "acr_pull" {
  application_id = azuread_application.acr_pull.id
  display_name   = "alien-test"
}

resource "azurerm_role_assignment" "acr_pull" {
  provider             = azurerm.management
  scope                = azurerm_container_registry.test.id
  role_definition_name = "AcrPull"
  principal_id         = azuread_service_principal.acr_pull.object_id
}

resource "azuread_application" "acr_push" {
  display_name = "alien-test-acr-push"
  owners       = [data.azurerm_client_config.management.object_id]
}

resource "azuread_service_principal" "acr_push" {
  client_id = azuread_application.acr_push.client_id
  owners    = [data.azurerm_client_config.management.object_id]
}

resource "azuread_application_password" "acr_push" {
  application_id = azuread_application.acr_push.id
  display_name   = "alien-test"
}

resource "azurerm_role_assignment" "acr_push" {
  provider             = azurerm.management
  scope                = azurerm_container_registry.test.id
  role_definition_name = "AcrPush"
  principal_id         = azuread_service_principal.acr_push.object_id
}
