output "target_client_id" {
  value     = var.target_client_id
  sensitive = true
}

output "target_client_secret" {
  value     = var.target_client_secret
  sensitive = true
}

output "resource_group" {
  value = azurerm_resource_group.test.name
}

output "storage_account" {
  value = azurerm_storage_account.test.name
}

output "blob_container" {
  value = azurerm_storage_container.test.name
}

output "container_app_image_uri" {
  value = "${azurerm_container_registry.test.login_server}/http-server:latest"
}

output "managed_environment" {
  value = azurerm_container_app_environment.test.name
}

output "acr_name" {
  value = azurerm_container_registry.test.name
}

output "management_sp_client_id" {
  value = azuread_service_principal.manager.application_id
}

output "management_sp_client_secret" {
  value     = azuread_application_password.manager.value
  sensitive = true
}

output "management_sp_object_id" {
  value = azuread_service_principal.manager.object_id
}
