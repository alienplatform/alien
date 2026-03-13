output "management_service_account_key" {
  value     = local.manager_key_json
  sensitive = true
}

output "target_service_account_key" {
  value     = base64decode(google_service_account_key.target.private_key)
  sensitive = true
}

output "gcs_bucket" {
  value = google_storage_bucket.test.name
}

output "cloudrun_image_uri" {
  value = "${local.image_repository}:latest"
}
