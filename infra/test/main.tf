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
  ci_principal_set      = module.gcp.ci_principal_set

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
  ci_principal_set      = module.gcp.ci_principal_set

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
    azuread.target     = azuread.target
  }

  management_subscription_id = var.azure_management_subscription_id
  management_tenant_id       = var.azure_management_tenant_id
  management_client_id       = var.azure_management_client_id
  management_region          = var.azure_management_region
  target_subscription_id     = var.azure_target_subscription_id
  target_tenant_id           = var.azure_target_tenant_id
  target_client_id           = var.azure_target_client_id

  e2e_aks_cluster_name       = var.e2e_aks_cluster_name
  e2e_aks_kubernetes_version = var.e2e_aks_kubernetes_version
}

data "aws_caller_identity" "management" {
  provider = aws.management
}

# CI identities may not change the workload identity pool or provider that
# lets them sign in. A deny policy wins over every role they hold, including
# project IAM admin. The pool is changed by a person applying this directory.
resource "google_iam_deny_policy" "ci_identity_guard" {
  provider     = google.management
  parent       = urlencode("cloudresourcemanager.googleapis.com/projects/${var.google_management_project_id}")
  name         = "alien-test-ci-identity-guard"
  display_name = "CI identities cannot change workload identity federation"

  rules {
    deny_rule {
      denied_principals = concat(
        [module.gcp.ci_principal_set],
        [for e in [
          module.gcp.manager_service_account_email,
          module.gcp.target_service_account_email,
          module.gcp_target_1.target_service_account_email,
          module.gcp_target_3.target_service_account_email,
        ] : "principal://iam.googleapis.com/projects/-/serviceAccounts/${e}"],
      )
      denied_permissions = [
        "iam.googleapis.com/workloadIdentityPools.create",
        "iam.googleapis.com/workloadIdentityPools.update",
        "iam.googleapis.com/workloadIdentityPools.delete",
        "iam.googleapis.com/workloadIdentityPools.undelete",
        "iam.googleapis.com/workloadIdentityPoolProviders.create",
        "iam.googleapis.com/workloadIdentityPoolProviders.update",
        "iam.googleapis.com/workloadIdentityPoolProviders.delete",
        "iam.googleapis.com/workloadIdentityPoolProviders.undelete",
        "iam.googleapis.com/serviceAccountKeys.create",
        "iam.googleapis.com/serviceAccountKeys.upload",
      ]
    }
  }

  # The CI identity may mint tokens only for the manager service account. The
  # workload identity user binding already limits it; this keeps it that way if
  # a test grants more. The resource name of a service account carries its
  # unique id, so the exception matches on that. Only the CI identity is
  # covered, not the service accounts it impersonates, because tests create and
  # impersonate their own service accounts.
  rules {
    deny_rule {
      denied_principals = [module.gcp.ci_principal_set]
      denied_permissions = [
        "iam.googleapis.com/serviceAccounts.getAccessToken",
        "iam.googleapis.com/serviceAccounts.getOpenIdToken",
        "iam.googleapis.com/serviceAccounts.signBlob",
        "iam.googleapis.com/serviceAccounts.signJwt",
        "iam.googleapis.com/serviceAccounts.implicitDelegation",
      ]
      denial_condition {
        title      = "other than the manager service account"
        expression = "!resource.name.endsWith('/serviceAccounts/${module.gcp.manager_service_account_unique_id}')"
      }
    }
  }
}
