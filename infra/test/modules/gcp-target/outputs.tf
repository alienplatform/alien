output "target_service_account_key" {
  value     = base64decode(google_service_account_key.target.private_key)
  sensitive = true
}

output "target_project_id" {
  value     = var.target_project_id
  sensitive = true
}

output "target_region" {
  value     = var.target_region
  sensitive = true
}

output "e2e_network_name" {
  value     = google_compute_network.e2e.name
  sensitive = true
}

output "e2e_subnet_name" {
  value     = google_compute_subnetwork.e2e.name
  sensitive = true
}

output "e2e_network_region" {
  value     = google_compute_subnetwork.e2e.region
  sensitive = true
}
