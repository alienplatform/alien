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
  ])
  service            = each.key
  disable_on_destroy = false
}

resource "google_project_service" "target_apis" {
  provider = google.target
  for_each = toset([
    "run.googleapis.com",
    "iam.googleapis.com",
  ])
  service            = each.key
  disable_on_destroy = false
}

# ── Management: Service account + key ─────────────────────────────────────────

resource "google_service_account" "manager" {
  provider     = google.management
  account_id   = "alien-test-manager"
  display_name = "Alien Test Manager"
}

resource "google_service_account_key" "manager" {
  provider           = google.management
  service_account_id = google_service_account.manager.name
}

resource "google_project_iam_member" "manager_storage" {
  provider = google.management
  project  = var.management_project_id
  role     = "roles/storage.admin"
  member   = "serviceAccount:${google_service_account.manager.email}"
}

resource "google_project_iam_member" "manager_artifact_registry" {
  provider = google.management
  project  = var.management_project_id
  role     = "roles/artifactregistry.admin"
  member   = "serviceAccount:${google_service_account.manager.email}"
}

resource "google_project_iam_member" "manager_run" {
  provider = google.management
  project  = var.management_project_id
  role     = "roles/run.admin"
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

# ── Target: Service account + key ─────────────────────────────────────────────

resource "google_service_account" "target" {
  provider     = google.target
  account_id   = "alien-test-target"
  display_name = "Alien Test Target"
}

resource "google_service_account_key" "target" {
  provider           = google.target
  service_account_id = google_service_account.target.name
}

resource "google_project_iam_member" "target_run" {
  provider = google.target
  project  = var.target_project_id
  role     = "roles/run.admin"
  member   = "serviceAccount:${google_service_account.target.email}"
}

resource "google_project_iam_member" "target_iam_sa_user" {
  provider = google.target
  project  = var.target_project_id
  role     = "roles/iam.serviceAccountUser"
  member   = "serviceAccount:${google_service_account.target.email}"
}

locals {
  registry_host    = "${var.management_region}-docker.pkg.dev"
  image_repository = "${local.registry_host}/${var.management_project_id}/alien-test/http-server"
  manager_key_json = base64decode(google_service_account_key.manager.private_key)
}
