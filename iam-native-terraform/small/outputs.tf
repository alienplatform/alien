output "deployment_target" {
  value = "aws"
  description = "Terraform module target."
}

output "deployment_resource_prefix" {
  value = local.resource_prefix
  description = "Physical resource prefix."
}

output "deployment_platform" {
  value = local.deployment_platform
  description = "Target platform."
}

output "deployment_region" {
  value = local.deployment_region
  description = "Target cloud region or location."
}

output "deployment_setup_target" {
  value = ""
  description = "Setup target."
}

output "deployment_setup_import_format_version" {
  value = 1
  description = "Setup registration payload format version."
}

output "deployment_setup_fingerprint" {
  value = ""
  description = "Setup compatibility fingerprint."
}

output "deployment_setup_fingerprint_version" {
  value = 0
  description = "Setup fingerprint algorithm version."
}

output "deployment_management_config" {
  value = jsonencode(local.deployment_management_config)
  description = "Deployment registration management configuration JSON."
}

output "deployment_stack_settings" {
  value = jsonencode(local.deployment_settings)
  description = "Deployment registration settings JSON."
  sensitive = true
}

output "deployment_resources" {
  value = jsonencode(local.deployment_resources)
  description = "Deployment registration resource metadata JSON."
}
