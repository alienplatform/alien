variable "region" {
  description = "AWS region for the network and the internal API."
  type        = string
  default     = "us-east-1"
}

variable "name_prefix" {
  description = "Prefix for every resource name."
  type        = string
  default     = "agent-sandboxes"
}

variable "vpc_cidr" {
  description = "CIDR of the VPC."
  type        = string
  default     = "10.40.0.0/16"
}

variable "availability_zones" {
  description = "Two availability zones for the subnets. Empty picks the region's first two. Set it when an instance type is not offered in them."
  type        = list(string)
  default     = []
}

variable "instance_type" {
  description = "Instance type of the internal API host (arm64)."
  type        = string
  default     = "t4g.nano"
}

variable "scanner_api_key" {
  description = "The key the mock scanner requires. Stored as a SecureString in this account (it is also in Terraform state); write the same value into the stack's secret store."
  type        = string
  sensitive   = true
}
