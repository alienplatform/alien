variable "management_region" {
  type = string
}

variable "target_region" {
  type = string
}

variable "e2e_k8s_ingress_class" {
  type = string
}

variable "e2e_eks_cluster_name" {
  type    = string
  default = ""
}

variable "e2e_eks_kubernetes_version" {
  type = string
}

variable "e2e_ingress_nginx_chart_version" {
  type = string
}
