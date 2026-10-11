variable "name" {
  description = "Name prefix for every resource"
  type        = string
  default     = "alien-manager"
}

variable "vpc_id" {
  description = "VPC to run the manager in"
  type        = string
}

variable "load_balancer_subnet_ids" {
  description = "Subnets for the load balancer (public subnets for an internet-facing manager)"
  type        = list(string)
}

variable "task_subnet_ids" {
  description = "Subnets for the manager task and its file system. Private subnets need a NAT gateway (or VPC endpoints) to pull the image."
  type        = list(string)
}

variable "assign_public_ip" {
  description = "Give the task a public IP (for public task subnets without NAT)"
  type        = bool
  default     = false
}

variable "internal" {
  description = "Make the load balancer internal instead of internet-facing"
  type        = bool
  default     = false
}

variable "certificate_arn" {
  description = "ACM certificate for HTTPS. Without it the manager is served over plain HTTP, which is only suitable for trying it out."
  type        = string
  default     = null
}

variable "domain_name" {
  description = "Public host name of the manager (matching certificate_arn). Defaults to the load balancer's DNS name."
  type        = string
  default     = null
}

variable "allowed_cidr_blocks" {
  description = "Networks that may reach the manager (deployments, your CLI and backend)"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "image" {
  description = "Manager image"
  type        = string
  default     = "ghcr.io/alienplatform/alien-manager:v3.3.32"
}

variable "cpu" {
  description = "Task CPU units"
  type        = number
  default     = 1024
}

variable "memory" {
  description = "Task memory (MiB)"
  type        = number
  default     = 2048
}

variable "config" {
  description = "Extra alien-manager.toml content (TOML), e.g. [telemetry] or [artifact-registry] sections"
  type        = string
  default     = ""
}

variable "environment" {
  description = "Extra environment variables for the manager"
  type        = map(string)
  default     = {}
}

variable "create_artifact_repository" {
  description = "Store release images in an ECR repository (instead of the manager's file system)"
  type        = bool
  default     = true
}

variable "log_retention_days" {
  description = "Days to keep manager logs in CloudWatch"
  type        = number
  default     = 30
}

variable "tags" {
  description = "Tags for every resource"
  type        = map(string)
  default     = {}
}
