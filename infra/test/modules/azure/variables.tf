variable "management_subscription_id" {
  type      = string
  sensitive = true
}

variable "management_tenant_id" {
  type = string
}

variable "management_client_id" {
  type      = string
  sensitive = true
}

variable "management_client_secret" {
  type      = string
  sensitive = true
}

variable "management_region" {
  type    = string
  default = "eastus"
}

variable "target_subscription_id" {
  type      = string
  sensitive = true
}

variable "target_tenant_id" {
  type = string
}

variable "target_client_id" {
  type      = string
  sensitive = true
}

variable "target_client_secret" {
  type      = string
  sensitive = true
}

variable "e2e_aks_cluster_name" {
  type    = string
  default = ""
}

variable "e2e_aks_kubernetes_version" {
  type = string
}
