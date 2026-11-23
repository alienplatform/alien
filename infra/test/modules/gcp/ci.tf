# ── CI identity: GitHub Actions workload identity federation ─────────────────
# Cloud tests sign in with a short-lived GitHub OIDC token and impersonate the
# manager and target service accounts. There are no service account keys.
# Only jobs of this repository running in the `cloud-tests` environment match
# the provider condition. The pool lives in the management project; the deny
# policy in the root module keeps CI identities from changing it.

resource "google_iam_workload_identity_pool" "github" {
  provider                  = google.management
  workload_identity_pool_id = "alien-test-ci"
  display_name              = "Alien test CI"

  depends_on = [google_project_service.management_apis]
}

resource "google_iam_workload_identity_pool_provider" "github" {
  provider                           = google.management
  workload_identity_pool_id          = google_iam_workload_identity_pool.github.workload_identity_pool_id
  workload_identity_pool_provider_id = "github-cloud-tests"
  display_name                       = "GitHub cloud-tests"

  attribute_condition = "attribute.repository == \"alienplatform/alien\" && attribute.environment == \"cloud-tests\""

  attribute_mapping = {
    "google.subject"        = "assertion.sub"
    "attribute.repository"  = "assertion.repository"
    "attribute.environment" = "assertion.environment"
  }

  oidc {
    issuer_uri = "https://token.actions.githubusercontent.com"
  }
}

locals {
  ci_principal_set = "principalSet://iam.googleapis.com/${google_iam_workload_identity_pool.github.name}/attribute.repository/alienplatform/alien"
}

# The management identity of the tests is the manager service account.
resource "google_service_account_iam_member" "ci_impersonates_manager" {
  provider           = google.management
  service_account_id = google_service_account.manager.name
  role               = "roles/iam.workloadIdentityUser"
  member             = local.ci_principal_set
}

resource "google_service_account_iam_member" "ci_impersonates_target" {
  provider           = google.target
  service_account_id = google_service_account.target.name
  role               = "roles/iam.workloadIdentityUser"
  member             = local.ci_principal_set
}
