# AWS - Management
output "management_aws_region" {
  value     = var.aws_management_region
  sensitive = true
}

output "management_aws_access_key_id" {
  value     = module.aws.management_access_key_id
  sensitive = true
}

output "management_aws_secret_access_key" {
  value     = module.aws.management_secret_access_key
  sensitive = true
}

output "management_aws_account_id" {
  value     = data.aws_caller_identity.management.account_id
  sensitive = true
}

output "management_aws_role_arn" {
  value     = module.aws.management_role_arn
  sensitive = true
}

output "management_aws_role_name" {
  value     = module.aws.management_role_name
  sensitive = true
}

# AWS - Target
output "target_aws_region" {
  value     = var.aws_target_region
  sensitive = true
}

output "target_aws_access_key_id" {
  value     = module.aws.target_access_key_id
  sensitive = true
}

output "target_aws_secret_access_key" {
  value     = module.aws.target_secret_access_key
  sensitive = true
}

output "target_aws_account_id" {
  value     = module.aws.target_account_id
  sensitive = true
}

# AWS resources
output "aws_s3_bucket" {
  value     = module.aws.s3_bucket
  sensitive = true
}

output "aws_lambda_image_uri" {
  value     = module.aws.lambda_image_uri
  sensitive = true
}

output "aws_lambda_execution_role_arn" {
  value     = module.aws.lambda_execution_role_arn
  sensitive = true
}

output "aws_ecr_push_role_arn" {
  value     = module.aws.ecr_push_role_arn
  sensitive = true
}

output "aws_ecr_pull_role_arn" {
  value     = module.aws.ecr_pull_role_arn
  sensitive = true
}

# E2E artifact registry (matches alien-infra controller pattern)
output "e2e_aws_ar_push_role_arn" {
  value     = module.aws.e2e_ar_push_role_arn
  sensitive = true
}

output "e2e_aws_ar_pull_role_arn" {
  value     = module.aws.e2e_ar_pull_role_arn
  sensitive = true
}

# GCP - Management
output "management_gcp_service_account_key" {
  value     = module.gcp.management_service_account_key
  sensitive = true
}

output "management_gcp_project_id" {
  value     = var.google_management_project_id
  sensitive = true
}

output "management_gcp_region" {
  value     = var.google_management_region
  sensitive = true
}

# GCP - Target
output "target_gcp_service_account_key" {
  value     = module.gcp.target_service_account_key
  sensitive = true
}

output "target_gcp_project_id" {
  value     = var.google_target_project_id
  sensitive = true
}

output "target_gcp_region" {
  value     = var.google_target_region
  sensitive = true
}

output "gcp_management_identity_email" {
  value     = module.gcp.management_identity_email
  sensitive = true
}

output "gcp_management_identity_unique_id" {
  value     = module.gcp.management_identity_unique_id
  sensitive = true
}

# GCP resources
output "gcp_gcs_bucket" {
  value     = module.gcp.gcs_bucket
  sensitive = true
}

output "gcp_cloudrun_image_uri" {
  value     = module.gcp.cloudrun_image_uri
  sensitive = true
}

# E2E artifact registry (matches alien-infra controller pattern)
output "e2e_gcp_gar_repository" {
  value     = module.gcp.e2e_gar_repository
  sensitive = true
}

output "e2e_gcp_ar_pull_sa_email" {
  value     = module.gcp.e2e_ar_pull_sa_email
  sensitive = true
}

output "e2e_gcp_ar_push_sa_email" {
  value     = module.gcp.e2e_ar_push_sa_email
  sensitive = true
}

# Azure - Management
output "management_azure_subscription_id" {
  value     = var.azure_management_subscription_id
  sensitive = true
}

output "management_azure_tenant_id" {
  value     = var.azure_management_tenant_id
  sensitive = true
}

output "management_azure_client_id" {
  value     = var.azure_management_client_id
  sensitive = true
}

output "management_azure_client_secret" {
  value     = var.azure_management_client_secret
  sensitive = true
}

output "management_azure_region" {
  value     = var.azure_management_region
  sensitive = true
}

# Azure - Target
output "target_azure_subscription_id" {
  value     = var.azure_target_subscription_id
  sensitive = true
}

output "target_azure_tenant_id" {
  value     = var.azure_target_tenant_id
  sensitive = true
}

output "target_azure_client_id" {
  value     = module.azure.target_client_id
  sensitive = true
}

output "target_azure_client_secret" {
  value     = module.azure.target_client_secret
  sensitive = true
}

# Azure - Management Service Principal
output "management_azure_sp_client_id" {
  value     = module.azure.management_sp_client_id
  sensitive = true
}

output "management_azure_sp_client_secret" {
  value     = module.azure.management_sp_client_secret
  sensitive = true
}

output "management_azure_sp_object_id" {
  value     = module.azure.management_sp_object_id
  sensitive = true
}

# Azure resources
output "azure_resource_group" {
  value     = module.azure.resource_group
  sensitive = true
}

output "azure_storage_account" {
  value     = module.azure.storage_account
  sensitive = true
}

output "azure_blob_container" {
  value     = module.azure.blob_container
  sensitive = true
}

output "azure_container_app_image_uri" {
  value     = module.azure.container_app_image_uri
  sensitive = true
}

output "azure_managed_environment" {
  value     = module.azure.managed_environment
  sensitive = true
}

output "azure_acr_name" {
  value     = module.azure.acr_name
  sensitive = true
}

# E2E artifact registry
output "e2e_azure_acr_repository" {
  value     = module.azure.e2e_acr_repository
  sensitive = true
}

# AWS commands store (DynamoDB table name for alien-manager.toml generation)
output "aws_command_kv_table_name" {
  value     = module.aws.command_kv_table_name
  sensitive = true
}

# Azure shared Container Apps Environment (in target subscription)
output "azure_shared_container_env_name" {
  value     = module.azure.shared_container_env_name
  sensitive = true
}

output "azure_shared_container_env_resource_id" {
  value     = module.azure.shared_container_env_resource_id
  sensitive = true
}

output "azure_shared_container_env_resource_group" {
  value     = module.azure.shared_container_env_resource_group
  sensitive = true
}

output "azure_shared_container_env_default_domain" {
  value     = module.azure.shared_container_env_default_domain
  sensitive = true
}

output "azure_shared_container_env_static_ip" {
  value     = module.azure.shared_container_env_static_ip
  sensitive = true
}

output "azure_shared_container_env_join_role_id" {
  value     = module.azure.shared_container_env_join_role_id
  sensitive = true
}
