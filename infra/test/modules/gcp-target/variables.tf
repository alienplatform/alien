variable "management_project_id" {
  type = string
}

variable "target_project_id" {
  type = string
}

variable "target_region" {
  type = string
}

variable "ci_principal_set" {
  description = "Workload identity principal set of the CI pool; may impersonate the target service account."
  type        = string
}

variable "e2e_gke_cluster_name" {
  type    = string
  default = ""
}

variable "e2e_gke_release_channel" {
  type = string
}
