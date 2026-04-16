variable "name" {
  description = "Name prefix for all resources"
  type        = string
}

variable "project_id" {
  description = "GCP project ID"
  type        = string
}

variable "region" {
  description = "GCP region for all resources"
  type        = string
}

variable "enable_artifact_registry" {
  description = "Create Artifact Registry repository for deployment container images"
  type        = bool
  default     = true
}

variable "enable_commands_store" {
  description = "Create Firestore database and GCS bucket for commands storage"
  type        = bool
  default     = false
}

variable "enable_impersonation" {
  description = "Create service account for impersonation"
  type        = bool
  default     = false
}

variable "impersonation_members" {
  description = "IAM members allowed to impersonate the service account (e.g. serviceAccount:x@y.iam.gserviceaccount.com)"
  type        = list(string)
  default     = []
}

variable "labels" {
  description = "Labels applied to all resources"
  type        = map(string)
  default     = {}
}
