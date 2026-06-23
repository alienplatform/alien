terraform {
  required_version = ">= 1.5"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 5.0"
    }
  }
}

locals {
  observe_roles = toset(var.roles)
}

resource "google_project_iam_member" "observe" {
  for_each = local.observe_roles

  project = var.project_id
  role    = each.value
  member  = var.member
}
