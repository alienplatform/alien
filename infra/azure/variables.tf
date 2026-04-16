variable "name" {
  description = "Name prefix for all resources"
  type        = string
}

variable "resource_group_name" {
  description = "Name of the Azure resource group (will be created)"
  type        = string
  default     = ""
}

variable "location" {
  description = "Azure region for all resources"
  type        = string
}

variable "enable_artifact_registry" {
  description = "Create Azure Container Registry for deployment container images"
  type        = bool
  default     = true
}

variable "enable_commands_store" {
  description = "Create Table Storage and Blob Storage for commands"
  type        = bool
  default     = false
}

variable "enable_impersonation" {
  description = "Create managed identity for impersonation"
  type        = bool
  default     = false
}

variable "tags" {
  description = "Tags applied to all resources"
  type        = map(string)
  default     = {}
}
