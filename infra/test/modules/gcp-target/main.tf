terraform {
  required_providers {
    google = {
      source                = "hashicorp/google"
      version               = "~> 5.0"
      configuration_aliases = [google.management, google.target]
    }
    helm   = { source = "hashicorp/helm", version = "~> 2.0" }
    random = { source = "hashicorp/random", version = "~> 3.0" }
  }
}

resource "random_id" "suffix" {
  byte_length = 4
}

locals {
  e2e_gke_cluster_name = var.e2e_gke_cluster_name != "" ? var.e2e_gke_cluster_name : "alien-e2e-${random_id.suffix.hex}"
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
    "cloudscheduler.googleapis.com",
    "container.googleapis.com",
  ])
  service            = each.key
  disable_on_destroy = false
}

resource "google_compute_network" "e2e" {
  provider                = google.target
  name                    = "alien-e2e-${random_id.suffix.hex}"
  auto_create_subnetworks = false

  depends_on = [google_project_service.target_apis]
}

resource "google_compute_subnetwork" "e2e" {
  provider      = google.target
  name          = "alien-e2e-${random_id.suffix.hex}"
  ip_cidr_range = "10.252.0.0/20"
  region        = var.target_region
  network       = google_compute_network.e2e.id
}

resource "google_compute_router" "e2e" {
  provider = google.target
  name     = "alien-e2e-${random_id.suffix.hex}"
  region   = var.target_region
  network  = google_compute_network.e2e.id
}

resource "google_compute_router_nat" "e2e" {
  provider                           = google.target
  name                               = "alien-e2e-${random_id.suffix.hex}"
  router                             = google_compute_router.e2e.name
  region                             = google_compute_router.e2e.region
  nat_ip_allocate_option             = "AUTO_ONLY"
  source_subnetwork_ip_ranges_to_nat = "ALL_SUBNETWORKS_ALL_IP_RANGES"
}

# ── Target: shared GKE cluster for Terraform -> Helm E2Es ────────────────────

resource "google_compute_address" "e2e_ingress" {
  provider = google.target
  name     = "alien-e2e-ingress-${random_id.suffix.hex}"
  region   = var.target_region

  depends_on = [google_project_service.target_apis]
}

resource "google_container_cluster" "e2e" {
  provider = google.target
  name     = local.e2e_gke_cluster_name
  location = var.target_region

  deletion_protection = false
  enable_autopilot    = true
  network             = google_compute_network.e2e.name
  subnetwork          = google_compute_subnetwork.e2e.name

  release_channel {
    channel = var.e2e_gke_release_channel
  }

  ip_allocation_policy {}

  workload_identity_config {
    workload_pool = "${var.target_project_id}.svc.id.goog"
  }

  master_auth {
    client_certificate_config {
      issue_client_certificate = true
    }
  }

  depends_on = [google_project_service.target_apis]
}

provider "helm" {
  kubernetes {
    host                   = "https://${google_container_cluster.e2e.endpoint}"
    cluster_ca_certificate = base64decode(google_container_cluster.e2e.master_auth[0].cluster_ca_certificate)
    client_certificate     = base64decode(google_container_cluster.e2e.master_auth[0].client_certificate)
    client_key             = base64decode(google_container_cluster.e2e.master_auth[0].client_key)
  }
}

resource "helm_release" "e2e_ingress_nginx" {
  name             = "ingress-nginx"
  repository       = "https://kubernetes.github.io/ingress-nginx"
  chart            = "ingress-nginx"
  version          = var.e2e_ingress_nginx_chart_version
  namespace        = "ingress-nginx"
  create_namespace = true
  wait             = true
  timeout          = 600

  values = [
    yamlencode({
      controller = {
        ingressClass = var.e2e_k8s_ingress_class
        ingressClassResource = {
          enabled = true
          name    = var.e2e_k8s_ingress_class
        }
        service = {
          loadBalancerIP = google_compute_address.e2e_ingress.address
        }
        resources = {
          requests = {
            cpu    = "100m"
            memory = "128Mi"
          }
          limits = {
            cpu    = "500m"
            memory = "512Mi"
          }
        }
        admissionWebhooks = {
          enabled = false
        }
      }
    })
  ]

  depends_on = [google_container_cluster.e2e]
}

resource "google_service_account" "target" {
  provider     = google.target
  account_id   = "alien-test-target-${random_id.suffix.hex}"
  display_name = "Alien Test Target ${random_id.suffix.hex}"
}

resource "google_service_account_key" "target" {
  provider           = google.target
  service_account_id = google_service_account.target.name
}

locals {
  target_roles = [
    "roles/run.admin",
    "roles/cloudfunctions.admin",
    "roles/pubsub.admin",
    "roles/storage.admin",
    "roles/iam.serviceAccountAdmin",
    "roles/iam.serviceAccountUser",
    "roles/iam.roleAdmin",
    "roles/resourcemanager.projectIamAdmin",
    "roles/secretmanager.admin",
    "roles/cloudbuild.builds.editor",
    "roles/datastore.owner",
    "roles/serviceusage.serviceUsageAdmin",
    "roles/compute.admin",
    "roles/artifactregistry.admin",
    "roles/cloudscheduler.admin",
  ]

  target_mgmt_roles = [
    "roles/serviceusage.serviceUsageAdmin",
    "roles/iam.serviceAccountAdmin",
    "roles/iam.roleAdmin",
    "roles/resourcemanager.projectIamAdmin",
  ]
}

resource "google_project_iam_member" "target_roles" {
  provider = google.target
  for_each = toset(local.target_roles)
  project  = var.target_project_id
  role     = each.value
  member   = "serviceAccount:${google_service_account.target.email}"
}

resource "google_project_iam_member" "target_management_access" {
  provider = google.management
  for_each = toset(local.target_mgmt_roles)
  project  = var.management_project_id
  role     = each.value
  member   = "serviceAccount:${google_service_account.target.email}"
}
