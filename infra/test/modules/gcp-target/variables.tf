variable "management_project_id" {
  type = string
}

variable "target_project_id" {
  type = string
}

variable "target_region" {
  type = string
}

variable "target_provider_email" {
  type = string
}

variable "e2e_gke_cluster_name" {
  type    = string
  default = ""
}

variable "e2e_gke_release_channel" {
  type = string
}
