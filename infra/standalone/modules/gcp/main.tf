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

# ── Manager SA: Execution identity ────────────────────────────────────────────
# Runs the alien-manager process. Needs permissions for all resource types it
# provisions, but NOT roles/owner. Uses scoped predefined roles instead.

resource "google_service_account" "manager" {
  provider     = google.management
  account_id   = "alien-test-manager"
  display_name = "Alien Test Manager"
}

resource "google_service_account_key" "manager" {
  provider           = google.management
  service_account_id = google_service_account.manager.name
}

# Scoped roles for the manager SA on the management project.
# These cover all GCP resource types the manager provisions.
locals {
  manager_roles = [
    "roles/run.admin",                       # Cloud Run services
    "roles/cloudfunctions.admin",            # Cloud Functions
    "roles/pubsub.admin",                    # Pub/Sub topics & subscriptions
    "roles/storage.admin",                   # GCS buckets
    "roles/iam.serviceAccountAdmin",         # Service accounts (create/delete)
    "roles/iam.serviceAccountUser",          # Bind SAs to resources (actAs)
    "roles/iam.roleAdmin",                   # Custom IAM roles
    "roles/resourcemanager.projectIamAdmin", # Project-level IAM bindings
    "roles/secretmanager.admin",             # Secret Manager
    "roles/cloudbuild.builds.editor",        # Cloud Build
    "roles/datastore.owner",                 # Firestore
    "roles/serviceusage.serviceUsageAdmin",  # Enable/disable APIs
    "roles/compute.networkAdmin",            # VPC, subnets, firewalls
    "roles/compute.loadBalancerAdmin",       # Load balancers
    "roles/artifactregistry.admin",          # Artifact Registry repositories
  ]
}

resource "google_project_iam_member" "manager_roles" {
  provider = google.management
  for_each = toset(local.manager_roles)
  project  = var.management_project_id
  role     = each.value
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
# cross-project operations). It only needs serviceAccountTokenCreator for
# impersonation — matching the production IAM model exactly.

resource "google_service_account" "management" {
  provider     = google.management
  account_id   = "alien-test-management"
  display_name = "Alien Test Management Identity"
}

resource "google_project_iam_member" "management_token_creator" {
  provider = google.management
  project  = var.management_project_id
  role     = "roles/iam.serviceAccountTokenCreator"
  member   = "serviceAccount:${google_service_account.management.email}"
}

resource "google_service_account_iam_member" "execution_impersonates_management" {
  provider           = google.management
  service_account_id = google_service_account.management.name
  role               = "roles/iam.serviceAccountTokenCreator"
  member             = "serviceAccount:${google_service_account.manager.email}"
}

# ── AR Pull/Push SAs (matching production) ───────────────────────────────────
# Separate service accounts for artifact registry pull/push, with the management
# SA able to impersonate them.

resource "google_service_account" "ar_pull" {
  provider     = google.management
  account_id   = "alien-test-ar-pull"
  display_name = "Alien Test AR Pull"
}

resource "google_project_iam_custom_role" "ar_pull" {
  provider = google.management
  role_id  = "alien_test_ar_pull"
  title    = "Alien Test AR Pull"
  project  = var.management_project_id
  permissions = [
    "artifactregistry.repositories.downloadArtifacts",
    "artifactregistry.files.get",
    "artifactregistry.repositories.get",
    "artifactregistry.repositories.list",
  ]
}

resource "google_project_iam_member" "ar_pull" {
  provider = google.management
  project  = var.management_project_id
  role     = google_project_iam_custom_role.ar_pull.id
  member   = "serviceAccount:${google_service_account.ar_pull.email}"
}

resource "google_service_account" "ar_push" {
  provider     = google.management
  account_id   = "alien-test-ar-push"
  display_name = "Alien Test AR Push"
}

resource "google_project_iam_custom_role" "ar_push" {
  provider = google.management
  role_id  = "alien_test_ar_push"
  title    = "Alien Test AR Push"
  project  = var.management_project_id
  permissions = [
    "artifactregistry.repositories.downloadArtifacts",
    "artifactregistry.repositories.uploadArtifacts",
    "artifactregistry.files.get",
    "artifactregistry.repositories.get",
    "artifactregistry.repositories.list",
    "artifactregistry.repositories.createOnPush",
  ]
}

resource "google_project_iam_member" "ar_push" {
  provider = google.management
  project  = var.management_project_id
  role     = google_project_iam_custom_role.ar_push.id
  member   = "serviceAccount:${google_service_account.ar_push.email}"
}

# Management SA can impersonate AR pull/push SAs
resource "google_service_account_iam_member" "management_impersonates_ar_pull" {
  provider           = google.management
  service_account_id = google_service_account.ar_pull.name
  role               = "roles/iam.serviceAccountTokenCreator"
  member             = "serviceAccount:${google_service_account.management.email}"
}

resource "google_service_account_iam_member" "management_impersonates_ar_push" {
  provider           = google.management
  service_account_id = google_service_account.ar_push.name
  role               = "roles/iam.serviceAccountTokenCreator"
  member             = "serviceAccount:${google_service_account.management.email}"
}

# ── Target: Service account + key ─────────────────────────────────────────────
# The target SA runs deployment steps (push_initial_setup). It needs the same
# provisioning permissions as the manager SA on the target project.

resource "google_service_account" "target" {
  provider     = google.target
  account_id   = "alien-test-target"
  display_name = "Alien Test Target"
}

resource "google_service_account_key" "target" {
  provider           = google.target
  service_account_id = google_service_account.target.name
}

resource "google_project_iam_member" "target_roles" {
  provider = google.target
  for_each = toset(local.manager_roles)
  project  = var.target_project_id
  role     = each.value
  member   = "serviceAccount:${google_service_account.target.email}"
}

# The target SA needs limited access to the management project for
# push_initial_setup() — service activation and cross-project IAM operations.
locals {
  target_mgmt_roles = [
    "roles/serviceusage.serviceUsageAdmin",  # Enable APIs
    "roles/iam.serviceAccountAdmin",         # Manage SAs
    "roles/iam.roleAdmin",                   # Custom roles
    "roles/resourcemanager.projectIamAdmin", # IAM bindings
  ]
}

resource "google_project_iam_member" "target_management_access" {
  provider = google.management
  for_each = toset(local.target_mgmt_roles)
  project  = var.management_project_id
  role     = each.value
  member   = "serviceAccount:${google_service_account.target.email}"
}

locals {
  registry_host    = "${var.management_region}-docker.pkg.dev"
  image_repository = "${local.registry_host}/${var.management_project_id}/alien-test/http-server"
  manager_key_json = base64decode(google_service_account_key.manager.private_key)
}
