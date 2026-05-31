module "aws" {
  source = "./modules/aws"

  providers = {
    aws.management = aws.management
    aws.target     = aws.target
  }

  management_region = var.aws_management_region
  target_region     = var.aws_target_region

  e2e_eks_cluster_name       = var.e2e_eks_cluster_name
  e2e_eks_kubernetes_version = var.e2e_eks_kubernetes_version
}

module "gcp" {
  source = "./modules/gcp"

  providers = {
    google.management = google.management
    google.target     = google.target
  }

  management_project_id = var.google_management_project_id
  management_region     = var.google_management_region
  target_project_id     = var.google_target_project_id
  target_region         = var.google_target_region
  target_provider_email = nonsensitive(try(jsondecode(var.google_target_service_account_key).client_email, ""))

  e2e_gke_cluster_name    = var.e2e_gke_cluster_name
  e2e_gke_release_channel = var.e2e_gke_release_channel
}

module "gcp_target_1" {
  source = "./modules/gcp-target"

  providers = {
    google.management = google.management
    google.target     = google.target_1
  }

  management_project_id = var.google_management_project_id
  target_project_id     = var.google_target_1_project_id
  target_region         = var.google_target_1_region
  target_provider_email = nonsensitive(try(jsondecode(var.google_target_1_service_account_key).client_email, ""))

  e2e_gke_cluster_name    = var.e2e_gke_cluster_name
  e2e_gke_release_channel = var.e2e_gke_release_channel
}

module "gcp_target_3" {
  source = "./modules/gcp-target"

  providers = {
    google.management = google.management
    google.target     = google.target_3
  }

  management_project_id = var.google_management_project_id
  target_project_id     = var.google_target_3_project_id
  target_region         = var.google_target_3_region
  target_provider_email = nonsensitive(try(jsondecode(var.google_target_3_service_account_key).client_email, ""))

  e2e_gke_cluster_name    = var.e2e_gke_cluster_name
  e2e_gke_release_channel = var.e2e_gke_release_channel
}

module "azure" {
  source = "./modules/azure"

  providers = {
    azurerm.management = azurerm.management
    azurerm.target     = azurerm.target
    azapi.target       = azapi.target
    azuread.management = azuread
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

  e2e_aks_cluster_name       = var.e2e_aks_cluster_name
  e2e_aks_kubernetes_version = var.e2e_aks_kubernetes_version
}

data "aws_caller_identity" "management" {
  provider = aws.management
}
