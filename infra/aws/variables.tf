variable "name" {
  description = "Name prefix for all resources"
  type        = string
}

variable "principal_arn" {
  description = "ARN of the IAM role/user running the manager (receives permissions to access these resources)"
  type        = string
}

variable "enable_artifact_registry" {
  description = "Create ECR repository for deployment container images"
  type        = bool
  default     = true
}

variable "enable_commands_store" {
  description = "Create DynamoDB table and S3 bucket for commands storage"
  type        = bool
  default     = false
}

variable "enable_impersonation" {
  description = "Create IAM role for cross-account impersonation"
  type        = bool
  default     = false
}

variable "impersonation_trusted_accounts" {
  description = "AWS account IDs allowed to assume the impersonation role"
  type        = list(string)
  default     = []
}

variable "tags" {
  description = "Tags applied to all resources"
  type        = map(string)
  default     = {}
}
