output "target_client_id" {
  value     = var.target_client_id
  sensitive = true
}

output "target_principal_id" {
  value     = data.azuread_service_principal.target.object_id
  sensitive = true
}

output "management_principal_id" {
  value     = data.azuread_service_principal.management.object_id
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

output "e2e_vnet_resource_id" {
  value     = azurerm_virtual_network.e2e.id
  sensitive = true
}

output "e2e_public_subnet_name" {
  value     = azurerm_subnet.e2e_public.name
  sensitive = true
}

output "e2e_private_subnet_name" {
  value     = azurerm_subnet.e2e_private.name
  sensitive = true
}

output "acr_name" {
  value     = azurerm_container_registry.test.name
  sensitive = true
}

# E2E artifact registry — uses the same ACR but with a separate image
# path prefix. ACR supports multiple image repos within one registry.
output "e2e_acr_repository" {
  value     = "${azurerm_container_registry.test.login_server}/alien-e2e"
  sensitive = true
}

output "e2e_aks_cluster_name" {
  value     = module.e2e_aks.name
  sensitive = true
}

output "e2e_aks_cluster_resource_group" {
  value     = azurerm_resource_group.shared_target.name
  sensitive = true
}

output "e2e_aks_kube_context" {
  value     = module.e2e_aks.name
  sensitive = true
}

output "e2e_k8s_public_host_suffix" {
  value     = "${azurerm_public_ip.e2e_ingress.ip_address}.sslip.io"
  sensitive = true
}

output "e2e_ingress_public_ip_name" {
  value     = azurerm_public_ip.e2e_ingress.name
  sensitive = true
}
