terraform {
  required_providers {
    aws     = { source = "hashicorp/aws", version = "~> 5.0" }
    google  = { source = "hashicorp/google", version = "~> 5.0" }
    azapi   = { source = "Azure/azapi", version = "~> 2.4" }
    azurerm = { source = "hashicorp/azurerm", version = ">= 4.46.0, < 5.0.0" }
    azuread = { source = "hashicorp/azuread", version = "~> 3.0" }
    random  = { source = "hashicorp/random", version = "~> 3.0" }
    time    = { source = "hashicorp/time", version = "~> 0.13" }
    tls     = { source = "hashicorp/tls", version = "~> 4.0" }
  }
}

# Terraform in this directory is applied by a person signed in with `az login`,
# `aws sso login` and `gcloud auth application-default login`; no provider
# carries a static credential.
provider "azuread" {
  tenant_id = var.azure_management_tenant_id
}

provider "azuread" {
  alias     = "target"
  tenant_id = var.azure_target_tenant_id
}

provider "aws" {
  alias   = "management"
  profile = "alien-test-mgmt"
  region  = var.aws_management_region
}

provider "aws" {
  alias   = "target"
  profile = "alien-test-target"
  region  = var.aws_target_region
}


provider "google" {
  alias   = "management"
  project = var.google_management_project_id
  region  = var.google_management_region
}

provider "google" {
  alias   = "target"
  project = var.google_target_project_id
  region  = var.google_target_region
}

provider "google" {
  alias   = "target_1"
  project = var.google_target_1_project_id
  region  = var.google_target_1_region
}

provider "google" {
  alias   = "target_3"
  project = var.google_target_3_project_id
  region  = var.google_target_3_region
}

provider "azurerm" {
  alias           = "management"
  subscription_id = var.azure_management_subscription_id
  tenant_id       = var.azure_management_tenant_id
  features {}
}

provider "azurerm" {
  alias           = "target"
  subscription_id = var.azure_target_subscription_id
  tenant_id       = var.azure_target_tenant_id
  features {}
}

provider "azapi" {
  alias           = "target"
  subscription_id = var.azure_target_subscription_id
  tenant_id       = var.azure_target_tenant_id
}
