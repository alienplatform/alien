output "role_assignment_id" {
  description = "Azure role assignment ID."
  value       = azurerm_role_assignment.observe.id
}

output "scope" {
  description = "Azure scope where observe read access was granted."
  value       = var.scope
}

output "principal_id" {
  description = "Principal granted observe read access."
  value       = var.principal_id
}

output "role_definition_name" {
  description = "Azure role assigned at the observe scope."
  value       = var.role_definition_name
}
