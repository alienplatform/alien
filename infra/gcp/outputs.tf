# -----------------------------------------------------------------------------
# Structured config values for alien-manager.toml
# -----------------------------------------------------------------------------

output "config_values" {
  description = "Values for alien-manager.toml sections (artifact-registry, commands, impersonation)"
  value = {
    artifact_registry = var.enable_artifact_registry ? {
      service        = "gar"
      repositoryName = "projects/${var.project_id}/locations/${var.region}/repositories/${google_artifact_registry_repository.artifacts[0].name}"
    } : null

    commands = var.enable_commands_store ? {
      kv = {
        service        = "firestore"
        projectId      = var.project_id
        databaseId     = google_firestore_database.commands[0].name
        collectionName = "alien-commands"
      }
      storage = {
        service    = "gcs"
        bucketName = google_storage_bucket.commands[0].name
      }
    } : null

    impersonation = var.enable_impersonation ? {
      service = "gcpserviceaccount"
      email   = google_service_account.impersonation[0].email
    } : null
  }
}

# -----------------------------------------------------------------------------
# Individual resource outputs
# -----------------------------------------------------------------------------

output "service_account_email" {
  description = "Email of the manager service account (attach to your compute)"
  value       = google_service_account.manager.email
}

output "gar_repository_name" {
  description = "Name of the Artifact Registry repository (if enabled)"
  value       = var.enable_artifact_registry ? google_artifact_registry_repository.artifacts[0].name : ""
}

output "gar_repository_url" {
  description = "URL of the Artifact Registry repository (if enabled)"
  value       = var.enable_artifact_registry ? "${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.artifacts[0].name}" : ""
}
