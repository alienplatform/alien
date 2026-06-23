variable "project_id" {
  description = "GCP project ID to observe."
  type        = string
}

variable "member" {
  description = "IAM member granted observe read access, such as serviceAccount:alien-manager@example.iam.gserviceaccount.com."
  type        = string
}

variable "roles" {
  description = "Project IAM roles granted to the observe identity."
  type        = list(string)
  default = [
    "roles/cloudasset.viewer",
    "roles/monitoring.viewer",
  ]
}
