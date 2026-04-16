output "management_service_account_key" {
  value     = local.manager_key_json
  sensitive = true
}

output "target_service_account_key" {
  value     = base64decode(google_service_account_key.target.private_key)
  sensitive = true
}

output "gcs_bucket" {
  value     = google_storage_bucket.test.name
  sensitive = true
}

output "cloudrun_image_uri" {
  value     = "${local.image_repository}:latest"
  sensitive = true
}

output "management_identity_email" {
  value     = google_service_account.management.email
  sensitive = true
}

output "management_identity_unique_id" {
  value     = google_service_account.management.unique_id
  sensitive = true
}

# E2E artifact registry (matches alien-infra controller pattern)
output "e2e_gar_repository" {
  value     = "${var.management_region}-docker.pkg.dev/${var.management_project_id}/${google_artifact_registry_repository.e2e.repository_id}"
  sensitive = true
}

output "e2e_ar_pull_sa_email" {
  value     = google_service_account.e2e_ar_pull.email
  sensitive = true
}

output "e2e_ar_push_sa_email" {
  value     = google_service_account.e2e_ar_push.email
  sensitive = true
}
