terraform {
  required_providers {
    aws     = { source = "hashicorp/aws",      version = "~> 5.0" }
    google  = { source = "hashicorp/google",   version = "~> 5.0" }
    azurerm = { source = "hashicorp/azurerm",  version = "~> 3.0" }
    docker  = { source = "kreuzwerker/docker", version = "~> 3.0" }
    random  = { source = "hashicorp/random",   version = "~> 3.0" }
  }
}

provider "aws" {
  alias      = "management"
  access_key = var.aws_management_access_key_id
  secret_key = var.aws_management_secret_access_key
  region     = var.aws_management_region
}

provider "aws" {
  alias      = "target"
  access_key = var.aws_target_access_key_id
  secret_key = var.aws_target_secret_access_key
  region     = var.aws_target_region
}

provider "google" {
  alias       = "management"
  credentials = var.google_management_service_account_key
  project     = var.google_management_project_id
  region      = var.google_management_region
}

provider "google" {
  alias       = "target"
  credentials = var.google_target_service_account_key
  project     = var.google_target_project_id
  region      = var.google_target_region
}

provider "azurerm" {
  alias           = "management"
  subscription_id = var.azure_management_subscription_id
  tenant_id       = var.azure_management_tenant_id
  client_id       = var.azure_management_client_id
  client_secret   = var.azure_management_client_secret
  features {}
}

provider "azurerm" {
  alias           = "target"
  subscription_id = var.azure_target_subscription_id
  tenant_id       = var.azure_target_tenant_id
  client_id       = var.azure_target_client_id
  client_secret   = var.azure_target_client_secret
  features {}
}

provider "docker" {}
