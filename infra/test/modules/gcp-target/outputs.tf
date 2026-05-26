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

output "e2e_gke_kubeconfig" {
  value = yamlencode({
    apiVersion = "v1"
    kind       = "Config"
    clusters = [{
      name = google_container_cluster.e2e.name
      cluster = {
        server                       = "https://${google_container_cluster.e2e.endpoint}"
        "certificate-authority-data" = google_container_cluster.e2e.master_auth[0].cluster_ca_certificate
      }
    }]
    contexts = [{
      name = google_container_cluster.e2e.name
      context = {
        cluster = google_container_cluster.e2e.name
        user    = google_container_cluster.e2e.name
      }
    }]
    "current-context" = google_container_cluster.e2e.name
    users = [{
      name = google_container_cluster.e2e.name
      user = {
        "client-certificate-data" = google_container_cluster.e2e.master_auth[0].client_certificate
        "client-key-data"         = google_container_cluster.e2e.master_auth[0].client_key
      }
    }]
  })
  sensitive = true
}

output "e2e_k8s_public_host_suffix" {
  value     = "${google_compute_address.e2e_ingress.address}.sslip.io"
  sensitive = true
}

output "e2e_ingress_ip_address" {
  value     = google_compute_address.e2e_ingress.address
  sensitive = true
}
