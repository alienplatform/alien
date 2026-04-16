# -----------------------------------------------------------------------------
# Service Account for Impersonation
# (conditional on enable_impersonation)
# -----------------------------------------------------------------------------

resource "google_service_account" "impersonation" {
  count = var.enable_impersonation ? 1 : 0

  account_id   = "${var.name}-impersonation"
  display_name = "${var.name} impersonation identity"
  project      = var.project_id
}

# Allow the manager service account to impersonate this identity
resource "google_service_account_iam_member" "manager_impersonates" {
  count = var.enable_impersonation ? 1 : 0

  service_account_id = google_service_account.impersonation[0].name
  role               = "roles/iam.serviceAccountTokenCreator"
  member             = "serviceAccount:${google_service_account.manager.email}"
}

# Allow additional members to impersonate (e.g. for cross-project access)
resource "google_service_account_iam_member" "external_impersonates" {
  for_each = var.enable_impersonation ? toset(var.impersonation_members) : toset([])

  service_account_id = google_service_account.impersonation[0].name
  role               = "roles/iam.serviceAccountTokenCreator"
  member             = each.value
}
