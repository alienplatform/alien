variable "scope" {
  description = "Azure scope to observe, such as a resource group ID or subscription ID."
  type        = string
}

variable "principal_id" {
  description = "Object/principal ID of the managed identity or service principal granted observe read access."
  type        = string
}

variable "role_definition_name" {
  description = "Azure role assigned at the observe scope."
  type        = string
  default     = "Reader"
}
