module "aws" {
  source = "./modules/aws"

  providers = {
    aws.management = aws.management
    aws.target     = aws.target
    docker         = docker
  }

  management_region = var.aws_management_region
  target_region     = var.aws_target_region
}

module "gcp" {
  source = "./modules/gcp"

  providers = {
    google.management = google.management
    google.target     = google.target
    docker            = docker
  }

  management_project_id = var.google_management_project_id
  management_region     = var.google_management_region
  target_project_id     = var.google_target_project_id
  target_region         = var.google_target_region
}

module "azure" {
  source = "./modules/azure"

  providers = {
    azurerm.management = azurerm.management
    azurerm.target     = azurerm.target
    docker             = docker
  }

  management_subscription_id = var.azure_management_subscription_id
  management_tenant_id       = var.azure_management_tenant_id
  management_client_id       = var.azure_management_client_id
  management_client_secret   = var.azure_management_client_secret
  management_region          = var.azure_management_region
  target_subscription_id     = var.azure_target_subscription_id
  target_tenant_id           = var.azure_target_tenant_id
  target_client_id           = var.azure_target_client_id
  target_client_secret       = var.azure_target_client_secret
}

data "aws_caller_identity" "management" {
  provider = aws.management
}
