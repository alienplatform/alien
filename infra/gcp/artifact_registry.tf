# -----------------------------------------------------------------------------
# Artifact Registry Repository (conditional on enable_artifact_registry)
# -----------------------------------------------------------------------------

resource "google_artifact_registry_repository" "artifacts" {
  count = var.enable_artifact_registry ? 1 : 0

  repository_id = "${var.name}-artifacts"
  location      = var.region
  project       = var.project_id
  format        = "DOCKER"
  labels        = local.common_labels

  cleanup_policies {
    id     = "keep-recent"
    action = "KEEP"

    most_recent_versions {
      keep_count = 50
    }
  }
}

# Allow the manager service account to push and pull images
resource "google_artifact_registry_repository_iam_member" "manager_writer" {
  count = var.enable_artifact_registry ? 1 : 0

  repository = google_artifact_registry_repository.artifacts[0].name
  location   = var.region
  project    = var.project_id
  role       = "roles/artifactregistry.writer"
  member     = "serviceAccount:${google_service_account.manager.email}"
}

resource "google_artifact_registry_repository_iam_member" "manager_reader" {
  count = var.enable_artifact_registry ? 1 : 0

  repository = google_artifact_registry_repository.artifacts[0].name
  location   = var.region
  project    = var.project_id
  role       = "roles/artifactregistry.reader"
  member     = "serviceAccount:${google_service_account.manager.email}"
}
