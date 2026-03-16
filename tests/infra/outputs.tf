# AWS - Management
output "management_aws_region" {
  value = var.aws_management_region
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
  value = data.aws_caller_identity.management.account_id
}

# AWS - Target
output "target_aws_region" {
  value = var.aws_target_region
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
  value = module.aws.target_account_id
}

# AWS resources
output "aws_s3_bucket" {
  value = module.aws.s3_bucket
}

output "aws_lambda_image_uri" {
  value = module.aws.lambda_image_uri
}

output "aws_lambda_execution_role_arn" {
  value = module.aws.lambda_execution_role_arn
}

# GCP - Management
output "management_gcp_service_account_key" {
  value     = module.gcp.management_service_account_key
  sensitive = true
}

output "management_gcp_project_id" {
  value = var.google_management_project_id
}

output "management_gcp_region" {
  value = var.google_management_region
}

# GCP - Target
output "target_gcp_service_account_key" {
  value     = module.gcp.target_service_account_key
  sensitive = true
}

output "target_gcp_project_id" {
  value = var.google_target_project_id
}

output "target_gcp_region" {
  value = var.google_target_region
}

# GCP resources
output "gcp_gcs_bucket" {
  value = module.gcp.gcs_bucket
}

output "gcp_cloudrun_image_uri" {
  value = module.gcp.cloudrun_image_uri
}

# Azure - Management
output "management_azure_subscription_id" {
  value     = var.azure_management_subscription_id
  sensitive = true
}

output "management_azure_tenant_id" {
  value = var.azure_management_tenant_id
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
  value = var.azure_management_region
}

# Azure - Target
output "target_azure_subscription_id" {
  value     = var.azure_target_subscription_id
  sensitive = true
}

output "target_azure_tenant_id" {
  value = var.azure_target_tenant_id
}

output "target_azure_client_id" {
  value     = module.azure.target_client_id
  sensitive = true
}

output "target_azure_client_secret" {
  value     = module.azure.target_client_secret
  sensitive = true
}

# Azure resources
output "azure_resource_group" {
  value = module.azure.resource_group
}

output "azure_storage_account" {
  value = module.azure.storage_account
}

output "azure_blob_container" {
  value = module.azure.blob_container
}

output "azure_container_app_image_uri" {
  value = module.azure.container_app_image_uri
}

output "azure_managed_environment" {
  value = module.azure.managed_environment
}

output "azure_acr_name" {
  value = module.azure.acr_name
}
