variable "resource_prefix" {
  type = string
  description = "Optional stable physical resource prefix. Leave empty to generate one."
  default = ""

  validation {
    condition = var.resource_prefix == "" || (can(regex("^[a-z][a-z0-9-]{1,38}[a-z0-9]$", var.resource_prefix)) && length(regexall("--", var.resource_prefix)) == 0)
    error_message = "resource_prefix must be 3-40 characters: lowercase letters, numbers, and hyphens; start with a letter; end with a letter or number; and not contain consecutive hyphens."
  }
}

variable "name" {
  type = string
  description = "Human-readable application name shown in setup and cloud IAM review metadata."
}

variable "token" {
  type = string
  description = "Install token from the application setup page. This is the same token used by the deploy CLI --token flag."
  sensitive = true
}

variable "management_url" {
  type = string
  description = "Optional management endpoint used by pull-style runtimes."
  default = ""
}

variable "deployment_model" {
  type = string
  description = "How runtime updates are delivered after setup."
  default = "push"

  validation {
    condition = contains(["push", "pull"], var.deployment_model)
    error_message = "deployment_model must be one of: push, pull."
  }
}

variable "advanced_settings_json" {
  type = string
  description = "Advanced JSON-encoded deployment settings. Most installations should use the typed variables in this module instead."
  default = "{}"
  sensitive = true
}

variable "advanced_settings_overlay_json" {
  type = string
  description = "JSON-encoded deployment settings merged over the package defaults. Use this for partial advanced-setting overrides that must preserve generated defaults such as compute selections."
  default = "{}"
  sensitive = true
}

variable "endpoint_access" {
  type = string
  description = "Who can reach this deployment's endpoints. Private access requires AWS managed containers and cannot change after setup."
  default = "internet"

  validation {
    condition = contains(["internet", "private"], var.endpoint_access)
    error_message = "endpoint_access must be one of: internet, private."
  }
}

variable "updates_mode" {
  type = string
  description = "How application updates are delivered after setup."
  default = "auto"

  validation {
    condition = contains(["auto", "approval-required"], var.updates_mode)
    error_message = "updates_mode must be one of: auto, approval-required."
  }
}

variable "telemetry_mode" {
  type = string
  description = "How logs, metrics, and traces are collected."
  default = "off"

  validation {
    condition = contains(["off", "auto", "approval-required"], var.telemetry_mode)
    error_message = "telemetry_mode must be one of: off, auto, approval-required."
  }
}

variable "heartbeats_mode" {
  type = string
  description = "Whether runtime health checks are enabled."
  default = "off"

  validation {
    condition = contains(["off", "on"], var.heartbeats_mode)
    error_message = "heartbeats_mode must be one of: off, on."
  }
}

variable "aws_region" {
  type = string
  description = "AWS region used by the AWS provider."
  default = "us-east-1"
}

variable "managing_role_arn" {
  type = string
  description = "ARN of the management identity allowed to assume setup-created roles."
  default = ""
}

variable "managing_account_id" {
  type = string
  description = "AWS account ID that hosts application container images. Empty disables scoped cross-account image-pull grants."
  default = ""
}

variable "network_mode" {
  type = string
  description = "Choose whether this setup creates a new network, uses an existing network, or uses the default network. Values: create-new, use-existing, use-default."
  default = "create-new"
}

variable "vpc_cidr" {
  type = string
  description = "CIDR for a newly-created network. Empty uses 10.42.0.0/16."
  default = ""
}

variable "availability_zones" {
  type = number
  description = "Number of availability zones to use when creating a new network."
  default = 2
}

variable "vpc_id" {
  type = string
  description = "Existing VPC ID. Required when network is use-existing."
  default = ""
}

variable "public_subnet_ids" {
  type = list(string)
  description = "Existing public subnet IDs. Required when network is use-existing."
  default = []
}

variable "private_subnet_ids" {
  type = list(string)
  description = "Existing private subnet IDs. Required when network is use-existing."
  default = []
}

variable "security_group_ids" {
  type = list(string)
  description = "Existing security group IDs. Required when network is use-existing."
  default = []
}

variable "unsupported_availability_zone_ids" {
  type = list(string)
  description = "Availability Zone IDs to exclude when selecting EKS control-plane subnets. AWS publishes EKS-disallowed zones by ID, not by name, because AZ names are account-local — the same physical zone can be `us-east-1e` in one account and `us-east-1c` in another. The defaults cover the AZs AWS documents as not supporting EKS control plane today (see https://docs.aws.amazon.com/eks/latest/userguide/network-reqs.html); override per region when AWS deprecates more."
  default = [
    "use1-az3",
    "usw1-az2",
    "cac1-az3"
  ]
}
