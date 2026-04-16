# AWS - Management
variable "aws_management_access_key_id" {
  description = "AWS management account access key ID"
  type        = string
  sensitive   = true
}

variable "aws_management_secret_access_key" {
  description = "AWS management account secret access key"
  type        = string
  sensitive   = true
}

variable "aws_management_region" {
  description = "AWS management account region"
  type        = string
  default     = "us-east-1"
}

# AWS - Target
variable "aws_target_access_key_id" {
  description = "AWS target account access key ID"
  type        = string
  sensitive   = true
}

variable "aws_target_secret_access_key" {
  description = "AWS target account secret access key"
  type        = string
  sensitive   = true
}

variable "aws_target_region" {
  description = "AWS target account region"
  type        = string
  default     = "us-east-2"
}

# GCP - Management
variable "google_management_service_account_key" {
  description = "GCP management project service account key JSON"
  type        = string
  sensitive   = true
}

variable "google_management_project_id" {
  description = "GCP management project ID"
  type        = string
}

variable "google_management_region" {
  description = "GCP management project region"
  type        = string
  default     = "us-central1"
}

# GCP - Target
variable "google_target_service_account_key" {
  description = "GCP target project service account key JSON"
  type        = string
  sensitive   = true
}

variable "google_target_project_id" {
  description = "GCP target project ID"
  type        = string
}

variable "google_target_region" {
  description = "GCP target project region"
  type        = string
  default     = "us-east4"
}

# Azure - Management
variable "azure_management_subscription_id" {
  description = "Azure management subscription ID"
  type        = string
  sensitive   = true
}

variable "azure_management_tenant_id" {
  description = "Azure management tenant ID"
  type        = string
}

variable "azure_management_client_id" {
  description = "Azure management service principal client ID"
  type        = string
  sensitive   = true
}

variable "azure_management_client_secret" {
  description = "Azure management service principal client secret"
  type        = string
  sensitive   = true
}

variable "azure_management_region" {
  description = "Azure management region"
  type        = string
  default     = "eastus"
}

# Azure - Target
variable "azure_target_subscription_id" {
  description = "Azure target subscription ID"
  type        = string
  sensitive   = true
}

variable "azure_target_tenant_id" {
  description = "Azure target tenant ID"
  type        = string
}

variable "azure_target_client_id" {
  description = "Azure target service principal client ID"
  type        = string
  sensitive   = true
}

variable "azure_target_client_secret" {
  description = "Azure target service principal client secret"
  type        = string
  sensitive   = true
}

