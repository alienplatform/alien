output "target_client_id" {
  value     = var.target_client_id
  sensitive = true
}

output "target_client_secret" {
  value     = var.target_client_secret
  sensitive = true
}

output "resource_group" {
  value     = azurerm_resource_group.test.name
  sensitive = true
}

output "storage_account" {
  value     = azurerm_storage_account.test.name
  sensitive = true
}

output "blob_container" {
  value     = azurerm_storage_container.test.name
  sensitive = true
}

output "container_app_image_uri" {
  value     = "${azurerm_container_registry.test.login_server}/http-server:latest"
  sensitive = true
}

output "managed_environment" {
  value     = azurerm_container_app_environment.test.name
  sensitive = true
}

output "shared_container_env_name" {
  value     = azurerm_container_app_environment.shared_target.name
  sensitive = true
}

output "shared_container_env_resource_id" {
  value     = azurerm_container_app_environment.shared_target.id
  sensitive = true
}

output "shared_container_env_resource_group" {
  value     = azurerm_resource_group.shared_target.name
  sensitive = true
}

output "shared_container_env_default_domain" {
  value     = azurerm_container_app_environment.shared_target.default_domain
  sensitive = true
}

output "shared_container_env_static_ip" {
  value     = azurerm_container_app_environment.shared_target.static_ip_address
  sensitive = true
}

output "shared_container_env_join_role_id" {
  value     = azurerm_role_definition.shared_env_join.role_definition_resource_id
  sensitive = true
}

output "acr_name" {
  value     = azurerm_container_registry.test.name
  sensitive = true
}

output "management_sp_client_id" {
  value     = azuread_service_principal.manager.client_id
  sensitive = true
}

output "management_sp_client_secret" {
  value     = azuread_application_password.manager.value
  sensitive = true
}

output "management_sp_object_id" {
  value     = azuread_service_principal.manager.object_id
  sensitive = true
}

# E2E artifact registry — uses the same ACR but with a separate image
# path prefix. ACR supports multiple image repos within one registry.
output "e2e_acr_repository" {
  value     = "${azurerm_container_registry.test.login_server}/alien-e2e"
  sensitive = true
}
