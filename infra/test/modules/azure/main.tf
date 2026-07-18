terraform {
  required_providers {
    azurerm = {
      source                = "hashicorp/azurerm"
      version               = ">= 4.46.0, < 5.0.0"
      configuration_aliases = [azurerm.management, azurerm.target]
    }
    azapi = {
      source                = "Azure/azapi"
      version               = "~> 2.4"
      configuration_aliases = [azapi.target]
    }
    azuread = {
      source                = "hashicorp/azuread"
      version               = "~> 3.0"
      configuration_aliases = [azuread.management]
    }
    random = { source = "hashicorp/random", version = "~> 3.0" }
    time   = { source = "hashicorp/time", version = "~> 0.13" }
  }
}

resource "random_id" "suffix" {
  byte_length = 4
}

locals {
  e2e_aks_cluster_name = var.e2e_aks_cluster_name != "" ? var.e2e_aks_cluster_name : "alien-e2e-${random_id.suffix.hex}"
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

resource "azurerm_virtual_network" "e2e" {
  provider            = azurerm.target
  name                = "alien-e2e-${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.shared_target.name
  location            = azurerm_resource_group.shared_target.location
  address_space       = ["10.253.0.0/16"]
}

resource "azurerm_subnet" "e2e_public" {
  provider             = azurerm.target
  name                 = "public"
  resource_group_name  = azurerm_resource_group.shared_target.name
  virtual_network_name = azurerm_virtual_network.e2e.name
  address_prefixes     = ["10.253.0.0/24"]
}

resource "azurerm_subnet" "e2e_private" {
  provider             = azurerm.target
  name                 = "private"
  resource_group_name  = azurerm_resource_group.shared_target.name
  virtual_network_name = azurerm_virtual_network.e2e.name
  address_prefixes     = ["10.253.1.0/24"]
}

resource "azurerm_public_ip" "e2e_nat" {
  provider            = azurerm.target
  name                = "alien-e2e-nat-${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.shared_target.name
  location            = azurerm_resource_group.shared_target.location
  allocation_method   = "Static"
  sku                 = "Standard"
}

resource "azurerm_nat_gateway" "e2e" {
  provider            = azurerm.target
  name                = "alien-e2e-${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.shared_target.name
  location            = azurerm_resource_group.shared_target.location
  sku_name            = "Standard"
}

resource "azurerm_nat_gateway_public_ip_association" "e2e" {
  provider             = azurerm.target
  nat_gateway_id       = azurerm_nat_gateway.e2e.id
  public_ip_address_id = azurerm_public_ip.e2e_nat.id
}

resource "azurerm_subnet_nat_gateway_association" "e2e_private" {
  provider       = azurerm.target
  subnet_id      = azurerm_subnet.e2e_private.id
  nat_gateway_id = azurerm_nat_gateway.e2e.id
}

resource "azurerm_container_app_environment" "shared_target" {
  provider            = azurerm.target
  name                = "alien-e2e-shared-${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.shared_target.name
  location            = azurerm_resource_group.shared_target.location
}

# ── Target: shared AKS cluster for Terraform -> Helm E2Es ────────────────────

resource "azurerm_public_ip" "e2e_ingress" {
  provider            = azurerm.target
  name                = "alien-e2e-ingress-${random_id.suffix.hex}"
  resource_group_name = azurerm_resource_group.shared_target.name
  location            = azurerm_resource_group.shared_target.location
  allocation_method   = "Static"
  sku                 = "Standard"
}

data "azurerm_client_config" "target" {
  provider = azurerm.target
}

module "e2e_aks" {
  source  = "Azure/avm-res-containerservice-managedcluster/azurerm"
  version = "0.5.6"

  providers = {
    azapi   = azapi.target
    azurerm = azurerm.target
  }

  name      = local.e2e_aks_cluster_name
  location  = azurerm_resource_group.shared_target.location
  parent_id = azurerm_resource_group.shared_target.id

  dns_prefix         = "alien-e2e-${random_id.suffix.hex}"
  enable_telemetry   = false
  kubernetes_version = var.e2e_aks_kubernetes_version

  aad_profile = {
    enable_azure_rbac = true
    managed           = true
    tenant_id         = var.target_tenant_id
  }

  default_agent_pool = {
    count_of       = 3
    vm_size        = "Standard_D2s_v3"
    vnet_subnet_id = azurerm_subnet.e2e_private.id

    upgrade_settings = {
      max_surge = "10%"
    }
  }

  sku = {
    name = "Base"
    tier = "Standard"
  }

  network_profile = {
    load_balancer_sku   = "standard"
    network_dataplane   = "cilium"
    network_plugin      = "azure"
    network_plugin_mode = "overlay"
    network_policy      = "cilium"
  }

  ingress_profile = {
    web_app_routing = {
      enabled = true
      nginx = {
        default_ingress_controller_type = "AnnotationControlled"
      }
    }
  }

  role_assignments = {
    target_admin = {
      principal_id               = data.azurerm_client_config.target.object_id
      role_definition_id_or_name = "Azure Kubernetes Service RBAC Cluster Admin"
    }
  }
}

resource "azurerm_role_assignment" "e2e_aks_network" {
  provider             = azurerm.target
  scope                = azurerm_resource_group.shared_target.id
  role_definition_name = "Network Contributor"
  principal_id         = module.e2e_aks.identity_principal_id
}

resource "time_sleep" "e2e_aks_rbac_propagation" {
  create_duration = "90s"

  depends_on = [
    azurerm_role_assignment.e2e_aks_network,
    module.e2e_aks,
  ]
}

locals {
  e2e_aks_kubeconfig = yamldecode(module.e2e_aks.kube_config)
}

# Custom role that allows using the shared environment. Created once here; the
# test harness assigns it to each deployment's management UAMI after InitialSetup
# (when the UAMI exists but before Provisioning needs it).
resource "azurerm_role_definition" "shared_env_join" {
  provider = azurerm.target
  name     = "alien-e2e-env-join-${random_id.suffix.hex}"
  scope    = azurerm_container_app_environment.shared_target.id

  permissions {
    actions = [
      "Microsoft.App/managedEnvironments/read",
      "Microsoft.App/managedEnvironments/join/action",
      "Microsoft.App/managedEnvironments/daprComponents/delete",
      "Microsoft.App/managedEnvironments/daprComponents/read",
      "Microsoft.App/managedEnvironments/daprComponents/write",
    ]
  }

  assignable_scopes = [
    azurerm_container_app_environment.shared_target.id,
  ]
}

# ── Management: RBAC for Terraform execution identity ─────────────────────────
# The Terraform SP (current authenticated principal) needs blob/table + ACR
# access for exercising the storage clients and pushing images during setup.

resource "azurerm_role_assignment" "manager_storage_blob" {
  provider             = azurerm.management
  scope                = azurerm_storage_account.test.id
  role_definition_name = "Storage Blob Data Contributor"
  principal_id         = data.azurerm_client_config.management.object_id
}

resource "azurerm_role_assignment" "manager_storage_table" {
  provider             = azurerm.management
  scope                = azurerm_storage_account.test.id
  role_definition_name = "Storage Table Data Contributor"
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

# Drop historical managed cloud-pull identities from state without requiring
# directory delete privileges. The current e2e flows do not use these resources.
removed {
  from = azuread_application.agent

  lifecycle {
    destroy = false
  }
}

removed {
  from = azuread_application_password.agent

  lifecycle {
    destroy = false
  }
}

removed {
  from = azuread_service_principal.agent

  lifecycle {
    destroy = false
  }
}

removed {
  from = azuread_service_principal.target_agent

  lifecycle {
    destroy = false
  }
}

# GitHub Actions OIDC federation — lets the Terraform execution SP
# (alien-terraform-bootstrap) authenticate using GitHub OIDC tokens in CI.
# Required for ACR scope map creation and other management-side operations.
# The FIC must be on the app that AZURE_MANAGEMENT_CLIENT_ID points to.
#
# We use a single environment-scoped FIC: when a workflow job declares
# `environment: e2e-tests`, GitHub emits OIDC tokens whose `sub` is
# `repo:OWNER/REPO:environment:e2e-tests` regardless of branch or event
# (push to main, pull_request, workflow_dispatch on a feature branch all
# match). This avoids per-branch FIC churn or hardcoding branch names.
data "azuread_application" "terraform_bootstrap" {
  provider  = azuread.management
  client_id = var.management_client_id
}

resource "azuread_application_federated_identity_credential" "github_environment" {
  provider       = azuread.management
  application_id = data.azuread_application.terraform_bootstrap.id
  display_name   = "github-actions-e2e-tests"
  issuer         = "https://token.actions.githubusercontent.com"
  subject        = "repo:alienplatform/alien:environment:e2e-tests"
  audiences      = ["api://AzureADTokenExchange"]
}

# ── ACR Pull/Push SPs (matching production) ──────────────────────────────────
# Separate service principals for ACR operations, scoped to the ACR resource.

resource "azuread_application" "acr_pull" {
  provider     = azuread.management
  display_name = "alien-test-acr-pull"
  owners       = [data.azurerm_client_config.management.object_id]
}

resource "azuread_service_principal" "acr_pull" {
  provider  = azuread.management
  client_id = azuread_application.acr_pull.client_id
  owners    = [data.azurerm_client_config.management.object_id]
}

resource "azuread_application_password" "acr_pull" {
  provider       = azuread.management
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
  provider     = azuread.management
  display_name = "alien-test-acr-push"
  owners       = [data.azurerm_client_config.management.object_id]
}

resource "azuread_service_principal" "acr_push" {
  provider  = azuread.management
  client_id = azuread_application.acr_push.client_id
  owners    = [data.azurerm_client_config.management.object_id]
}

resource "azuread_application_password" "acr_push" {
  provider       = azuread.management
  application_id = azuread_application.acr_push.id
  display_name   = "alien-test"
}

resource "azurerm_role_assignment" "acr_push" {
  provider             = azurerm.management
  scope                = azurerm_container_registry.test.id
  role_definition_name = "AcrPush"
  principal_id         = azuread_service_principal.acr_push.object_id
}
