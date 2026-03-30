terraform {
  required_providers {
    google = {
      source                = "hashicorp/google"
      version               = "~> 5.0"
      configuration_aliases = [google.management, google.target]
    }
    random = { source = "hashicorp/random", version = "~> 3.0" }
  }
}

resource "random_id" "suffix" {
  byte_length = 4
}

# ── Required APIs ─────────────────────────────────────────────────────────────

resource "google_project_service" "management_apis" {
  provider = google.management
  for_each = toset([
    "storage.googleapis.com",
    "artifactregistry.googleapis.com",
    "run.googleapis.com",
    "iam.googleapis.com",
    "cloudbuild.googleapis.com",
    "compute.googleapis.com",
    "secretmanager.googleapis.com",
    "pubsub.googleapis.com",
    "firestore.googleapis.com",
    "serviceusage.googleapis.com",
    "cloudresourcemanager.googleapis.com",
  ])
  service            = each.key
  disable_on_destroy = false
}

resource "google_project_service" "target_apis" {
  provider = google.target
  for_each = toset([
    "serviceusage.googleapis.com",
    "cloudresourcemanager.googleapis.com",
    "storage.googleapis.com",
    "artifactregistry.googleapis.com",
    "run.googleapis.com",
    "iam.googleapis.com",
    "compute.googleapis.com",
    "cloudbuild.googleapis.com",
    "secretmanager.googleapis.com",
    "pubsub.googleapis.com",
    "firestore.googleapis.com",
  ])
  service            = each.key
  disable_on_destroy = false
}

# ── Management: Service account + key ─────────────────────────────────────────
# Dedicated test project — roles/owner is safe here.

resource "google_service_account" "manager" {
  provider     = google.management
  account_id   = "alien-test-manager"
  display_name = "Alien Test Manager"
}

resource "google_service_account_key" "manager" {
  provider           = google.management
  service_account_id = google_service_account.manager.name
}

resource "google_project_iam_member" "manager_owner" {
  provider = google.management
  project  = var.management_project_id
  role     = "roles/owner"
  member   = "serviceAccount:${google_service_account.manager.email}"
}

# ── Management: GCS bucket ────────────────────────────────────────────────────

resource "google_storage_bucket" "test" {
  provider                    = google.management
  name                        = "alien-test-${random_id.suffix.hex}"
  location                    = var.management_region
  force_destroy               = true
  uniform_bucket_level_access = true
}

# ── Management: Artifact Registry ─────────────────────────────────────────────

resource "google_artifact_registry_repository" "test" {
  provider      = google.management
  location      = var.management_region
  repository_id = "alien-test"
  format        = "DOCKER"

  depends_on = [google_project_service.management_apis]
}

# ── Management: Separate management identity ──────────────────────────────
# The management SA is the identity that customers trust (impersonated during
# cross-project operations). It must NOT have artifact registry access.
# The execution SA (manager) can impersonate it via serviceAccountTokenCreator.

resource "google_service_account" "management" {
  provider     = google.management
  account_id   = "alien-test-management"
  display_name = "Alien Test Management Identity"
}

resource "google_project_iam_member" "management_owner" {
  provider = google.management
  project  = var.management_project_id
  role     = "roles/owner"
  member   = "serviceAccount:${google_service_account.management.email}"
}

resource "google_service_account_iam_member" "execution_impersonates_management" {
  provider           = google.management
  service_account_id = google_service_account.management.name
  role               = "roles/iam.serviceAccountTokenCreator"
  member             = "serviceAccount:${google_service_account.manager.email}"
}

# ── Target: Service account + key ─────────────────────────────────────────────
# Dedicated test project — roles/owner is safe here.

resource "google_service_account" "target" {
  provider     = google.target
  account_id   = "alien-test-target"
  display_name = "Alien Test Target"
}

resource "google_service_account_key" "target" {
  provider           = google.target
  service_account_id = google_service_account.target.name
}

resource "google_project_iam_member" "target_owner" {
  provider = google.target
  project  = var.target_project_id
  role     = "roles/owner"
  member   = "serviceAccount:${google_service_account.target.email}"
}

# The target SA needs access to the management project because push_initial_setup()
# runs the deployment step loop with target credentials. Steps like service activation
# operate on both projects, so the target SA must be able to call serviceusage APIs
# (and other resource-management APIs) on the management project.
# Dedicated test project — roles/owner is safe here.
resource "google_project_iam_member" "target_management_access" {
  provider = google.management
  project  = var.management_project_id
  role     = "roles/owner"
  member   = "serviceAccount:${google_service_account.target.email}"
}

locals {
  registry_host    = "${var.management_region}-docker.pkg.dev"
  image_repository = "${local.registry_host}/${var.management_project_id}/alien-test/http-server"
  manager_key_json = base64decode(google_service_account_key.manager.private_key)
}
