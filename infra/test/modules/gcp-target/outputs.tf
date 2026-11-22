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

output "target_service_account_email" {
  value     = google_service_account.target.email
  sensitive = true
}
