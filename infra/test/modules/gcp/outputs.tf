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

output "e2e_gke_cluster_name" {
  value     = google_container_cluster.e2e.name
  sensitive = true
}

output "e2e_gke_cluster_location" {
  value     = google_container_cluster.e2e.location
  sensitive = true
}

output "e2e_gke_kube_context" {
  value     = google_container_cluster.e2e.name
  sensitive = true
}

output "e2e_k8s_public_host_suffix" {
  value     = "${google_compute_global_address.e2e_ingress.address}.sslip.io"
  sensitive = true
}

output "e2e_ingress_ip_address" {
  value     = google_compute_global_address.e2e_ingress.address
  sensitive = true
}

output "e2e_ingress_ip_name" {
  value     = google_compute_global_address.e2e_ingress.name
  sensitive = true
}

output "manager_service_account_email" {
  value     = google_service_account.manager.email
  sensitive = true
}

output "target_service_account_email" {
  value     = google_service_account.target.email
  sensitive = true
}

output "ci_principal_set" {
  value = local.ci_principal_set
}

output "ci_workload_identity_provider" {
  description = "Full resource name of the GitHub OIDC provider, for google-github-actions/auth."
  value       = google_iam_workload_identity_pool_provider.github.name
}
