terraform {
  required_version = ">= 1.5"

  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = ">= 3.80"
    }
    random = {
      source  = "hashicorp/random"
      version = ">= 3.5"
    }
  }
}

# Short random suffix to avoid naming collisions on globally-unique resources
resource "random_id" "suffix" {
  byte_length = 4
}

locals {
  rg_name = var.resource_group_name != "" ? var.resource_group_name : "${var.name}-manager-rg"
  # Azure storage account names: lowercase alphanumeric, 3-24 chars
  storage_account_name = substr(replace(lower("${var.name}mgr${random_id.suffix.hex}"), "/[^a-z0-9]/", ""), 0, 24)

  common_tags = merge(var.tags, {
    "alien-managed-by" = "terraform"
    "alien-component"  = "alien-manager"
    "alien-name"       = var.name
  })
}

# -----------------------------------------------------------------------------
# Resource Group
# -----------------------------------------------------------------------------

resource "azurerm_resource_group" "manager" {
  name     = local.rg_name
  location = var.location
  tags     = local.common_tags
}

# -----------------------------------------------------------------------------
# Managed Identity
#
# Used by artifact_registry.tf, commands.tf, and impersonation.tf for RBAC
# assignments. Attach this identity to whatever compute runs your manager.
# -----------------------------------------------------------------------------

resource "azurerm_user_assigned_identity" "manager" {
  name                = "${var.name}-manager"
  location            = azurerm_resource_group.manager.location
  resource_group_name = azurerm_resource_group.manager.name
  tags                = local.common_tags
}

# -----------------------------------------------------------------------------
# Storage Account
#
# Used by commands.tf for Table Storage and Blob Storage.
# -----------------------------------------------------------------------------

resource "azurerm_storage_account" "manager" {
  name                     = local.storage_account_name
  resource_group_name      = azurerm_resource_group.manager.name
  location                 = azurerm_resource_group.manager.location
  account_tier             = "Standard"
  account_replication_type = "LRS"
  min_tls_version          = "TLS1_2"
  tags                     = local.common_tags
}
