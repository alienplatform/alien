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
  description = "GCP target-2 project service account key JSON"
  type        = string
  sensitive   = true
}

variable "google_target_project_id" {
  description = "GCP target-2 project ID"
  type        = string
}

variable "google_target_region" {
  description = "GCP target-2 project region"
  type        = string
  default     = "us-east4"
}

variable "google_target_1_service_account_key" {
  description = "GCP target-1 project service account key JSON"
  type        = string
  sensitive   = true
}

variable "google_target_1_project_id" {
  description = "GCP target-1 project ID"
  type        = string
}

variable "google_target_1_region" {
  description = "GCP target-1 project region"
  type        = string
  default     = "us-east4"
}

variable "google_target_3_service_account_key" {
  description = "GCP target-3 project service account key JSON"
  type        = string
  sensitive   = true
}

variable "google_target_3_project_id" {
  description = "GCP target-3 project ID"
  type        = string
}

variable "google_target_3_region" {
  description = "GCP target-3 project region"
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

# Shared Kubernetes clusters used by Terraform -> Helm distribution E2Es.
# These values describe long-lived test clusters. Individual test runs create
# random namespaces inside them; tests do not create or destroy clusters.
variable "e2e_k8s_namespace_prefix" {
  description = "Namespace prefix for Kubernetes distribution E2E runs."
  type        = string
  default     = "alien-test"
}

variable "e2e_k8s_ingress_class" {
  description = "IngressClass name used by Kubernetes distribution E2Es."
  type        = string
  default     = ""
}

variable "e2e_k8s_public_host_suffix" {
  description = "Optional DNS suffix override for per-test Kubernetes public hosts. If empty, infra/test emits a static-IP sslip.io suffix per cluster."
  type        = string
  default     = ""
}

variable "e2e_k8s_tls_secret_name" {
  description = "Optional TLS secret present in each test namespace."
  type        = string
  default     = ""
}

variable "e2e_eks_cluster_name" {
  description = "Shared EKS cluster name for Terraform EKS Helm E2Es."
  type        = string
  default     = ""
}

variable "e2e_eks_kube_context" {
  description = "Optional kubeconfig context for the shared EKS cluster."
  type        = string
  default     = ""
}

variable "e2e_eks_kubernetes_version" {
  description = "Kubernetes version for the shared EKS cluster."
  type        = string
  default     = "1.35"
}

variable "e2e_gke_cluster_name" {
  description = "Shared GKE cluster name for Terraform GKE Helm E2Es."
  type        = string
  default     = ""
}

variable "e2e_gke_cluster_location" {
  description = "Shared GKE cluster region or zone for Terraform GKE Helm E2Es."
  type        = string
  default     = ""
}

variable "e2e_gke_kube_context" {
  description = "Optional kubeconfig context for the shared GKE cluster."
  type        = string
  default     = ""
}

variable "e2e_gke_release_channel" {
  description = "GKE release channel for the shared GKE cluster."
  type        = string
  default     = "RAPID"
}

variable "e2e_aks_cluster_name" {
  description = "Shared AKS cluster name for Terraform AKS Helm E2Es."
  type        = string
  default     = ""
}

variable "e2e_aks_cluster_resource_group" {
  description = "Resource group containing the shared AKS cluster."
  type        = string
  default     = ""
}

variable "e2e_aks_kube_context" {
  description = "Optional kubeconfig context for the shared AKS cluster."
  type        = string
  default     = ""
}

variable "e2e_aks_kubernetes_version" {
  description = "Kubernetes version for the shared AKS cluster."
  type        = string
  default     = "1.35"
}
