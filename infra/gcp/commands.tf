# -----------------------------------------------------------------------------
# Firestore Database + GCS Bucket for Commands Store
# (conditional on enable_commands_store)
# -----------------------------------------------------------------------------

resource "google_firestore_database" "commands" {
  count = var.enable_commands_store ? 1 : 0

  name        = "${var.name}-commands"
  location_id = var.region
  project     = var.project_id
  type        = "FIRESTORE_NATIVE"

  # Firestore databases cannot be easily recreated
  lifecycle {
    prevent_destroy = true
  }
}

resource "google_storage_bucket" "commands" {
  count = var.enable_commands_store ? 1 : 0

  name     = "${var.name}-commands-store"
  location = var.region
  project  = var.project_id
  labels   = local.common_labels

  uniform_bucket_level_access = true

  versioning {
    enabled = true
  }

  lifecycle_rule {
    condition {
      age = 90
    }
    action {
      type = "Delete"
    }
  }

  lifecycle_rule {
    condition {
      num_newer_versions = 3
    }
    action {
      type = "Delete"
    }
  }
}

# IAM: allow manager to read/write Firestore
resource "google_project_iam_member" "manager_firestore" {
  count = var.enable_commands_store ? 1 : 0

  project = var.project_id
  role    = "roles/datastore.user"
  member  = "serviceAccount:${google_service_account.manager.email}"
}

# IAM: allow manager to read/write commands GCS bucket
resource "google_storage_bucket_iam_member" "manager_commands" {
  count = var.enable_commands_store ? 1 : 0

  bucket = google_storage_bucket.commands[0].name
  role   = "roles/storage.objectAdmin"
  member = "serviceAccount:${google_service_account.manager.email}"
}
