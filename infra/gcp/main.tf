terraform {
  required_version = ">= 1.5"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 5.0"
    }
  }
}

# -----------------------------------------------------------------------------
# Service Account
#
# Used by artifact_registry.tf, commands.tf, and impersonation.tf for IAM
# bindings. Attach this SA to whatever compute runs your manager.
# -----------------------------------------------------------------------------

resource "google_service_account" "manager" {
  account_id   = "${var.name}-manager"
  display_name = "${var.name} alien-manager"
  project      = var.project_id
}

locals {
  common_labels = merge(var.labels, {
    "alien-managed-by" = "terraform"
    "alien-component"  = "alien-manager"
    "alien-name"       = var.name
  })
}
