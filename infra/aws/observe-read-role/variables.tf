variable "name" {
  description = "IAM role name to create."
  type        = string
}

variable "manager_managing_role_arn" {
  description = "ARN of the Alien manager role allowed to assume this observe role."
  type        = string
}

variable "external_id" {
  description = "Per-customer ExternalId required for sts:AssumeRole."
  type        = string
  sensitive   = true
}

variable "tags" {
  description = "Tags applied to created IAM resources."
  type        = map(string)
  default     = {}
}
