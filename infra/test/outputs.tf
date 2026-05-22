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

output "e2e_aws_vpc_id" {
  value     = module.aws.e2e_vpc_id
  sensitive = true
}

output "e2e_aws_public_subnet_ids" {
  value     = module.aws.e2e_public_subnet_ids
  sensitive = true
}

output "e2e_aws_private_subnet_ids" {
  value     = module.aws.e2e_private_subnet_ids
  sensitive = true
}

output "e2e_aws_security_group_ids" {
  value     = module.aws.e2e_security_group_ids
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

output "aws_target_options" {
  value = {
    aws-target-1 = {
      AWS_TARGET_REGION                        = var.aws_target_region
      AWS_TARGET_ACCESS_KEY_ID                 = module.aws.target_access_key_id
      AWS_TARGET_SECRET_ACCESS_KEY             = module.aws.target_secret_access_key
      AWS_TARGET_ACCOUNT_ID                    = module.aws.target_account_id
      ALIEN_E2E_AWS_VPC_ID                     = module.aws.e2e_vpc_id
      ALIEN_E2E_AWS_PUBLIC_SUBNET_IDS          = module.aws.e2e_public_subnet_ids
      ALIEN_E2E_AWS_PRIVATE_SUBNET_IDS         = module.aws.e2e_private_subnet_ids
      ALIEN_E2E_AWS_SECURITY_GROUP_IDS         = module.aws.e2e_security_group_ids
      ALIEN_TEST_AWS_S3_BUCKET                 = module.aws.s3_bucket
      ALIEN_TEST_AWS_COMMAND_KV_TABLE          = module.aws.command_kv_table_name
      ALIEN_TEST_AWS_LAMBDA_IMAGE              = module.aws.lambda_image_uri
      ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN = module.aws.lambda_execution_role_arn
      ALIEN_TEST_AWS_ECR_PUSH_ROLE_ARN         = module.aws.ecr_push_role_arn
      ALIEN_TEST_AWS_ECR_PULL_ROLE_ARN         = module.aws.ecr_pull_role_arn
      ALIEN_TEST_AWS_ECR_REPOSITORY            = split(":", module.aws.lambda_image_uri)[0]
      E2E_AWS_AR_PUSH_ROLE_ARN                 = module.aws.e2e_ar_push_role_arn
      E2E_AWS_AR_PULL_ROLE_ARN                 = module.aws.e2e_ar_pull_role_arn
      ALIEN_TEST_K8S_NAMESPACE_PREFIX          = var.e2e_k8s_namespace_prefix
      ALIEN_TEST_K8S_INGRESS_CLASS             = var.e2e_k8s_ingress_class
      ALIEN_TEST_K8S_PUBLIC_HOST_SUFFIX        = var.e2e_k8s_public_host_suffix
      ALIEN_TEST_K8S_TLS_SECRET_NAME           = var.e2e_k8s_tls_secret_name
      ALIEN_TEST_EKS_CLUSTER_NAME              = var.e2e_eks_cluster_name
      ALIEN_TEST_EKS_KUBE_CONTEXT              = var.e2e_eks_kube_context
    }
  }
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

output "e2e_gcp_network_name" {
  value     = module.gcp.e2e_network_name
  sensitive = true
}

output "e2e_gcp_subnet_name" {
  value     = module.gcp.e2e_subnet_name
  sensitive = true
}

output "e2e_gcp_network_region" {
  value     = module.gcp.e2e_network_region
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

output "gcp_target_options" {
  value = merge(
    {
      gcp-target-1 = {
        GOOGLE_TARGET_SERVICE_ACCOUNT_KEY = module.gcp_target_1.target_service_account_key
        GOOGLE_TARGET_PROJECT_ID          = module.gcp_target_1.target_project_id
        GOOGLE_TARGET_REGION              = module.gcp_target_1.target_region
        ALIEN_E2E_GCP_NETWORK_NAME        = module.gcp_target_1.e2e_network_name
        ALIEN_E2E_GCP_SUBNET_NAME         = module.gcp_target_1.e2e_subnet_name
        ALIEN_E2E_GCP_REGION              = module.gcp_target_1.e2e_network_region
        ALIEN_TEST_K8S_NAMESPACE_PREFIX   = var.e2e_k8s_namespace_prefix
        ALIEN_TEST_K8S_INGRESS_CLASS      = var.e2e_k8s_ingress_class
        ALIEN_TEST_K8S_PUBLIC_HOST_SUFFIX = var.e2e_k8s_public_host_suffix
        ALIEN_TEST_K8S_TLS_SECRET_NAME    = var.e2e_k8s_tls_secret_name
        ALIEN_TEST_GKE_CLUSTER_NAME       = var.e2e_gke_cluster_name
        ALIEN_TEST_GKE_CLUSTER_LOCATION   = var.e2e_gke_cluster_location
        ALIEN_TEST_GKE_KUBE_CONTEXT       = var.e2e_gke_kube_context
      }
      gcp-target-2 = {
        GOOGLE_TARGET_SERVICE_ACCOUNT_KEY = module.gcp.target_service_account_key
        GOOGLE_TARGET_PROJECT_ID          = var.google_target_project_id
        GOOGLE_TARGET_REGION              = var.google_target_region
        ALIEN_E2E_GCP_NETWORK_NAME        = module.gcp.e2e_network_name
        ALIEN_E2E_GCP_SUBNET_NAME         = module.gcp.e2e_subnet_name
        ALIEN_E2E_GCP_REGION              = module.gcp.e2e_network_region
        ALIEN_TEST_K8S_NAMESPACE_PREFIX   = var.e2e_k8s_namespace_prefix
        ALIEN_TEST_K8S_INGRESS_CLASS      = var.e2e_k8s_ingress_class
        ALIEN_TEST_K8S_PUBLIC_HOST_SUFFIX = var.e2e_k8s_public_host_suffix
        ALIEN_TEST_K8S_TLS_SECRET_NAME    = var.e2e_k8s_tls_secret_name
        ALIEN_TEST_GKE_CLUSTER_NAME       = var.e2e_gke_cluster_name
        ALIEN_TEST_GKE_CLUSTER_LOCATION   = var.e2e_gke_cluster_location
        ALIEN_TEST_GKE_KUBE_CONTEXT       = var.e2e_gke_kube_context
      }
    },
    var.google_target_3_enabled ? {
      gcp-target-3 = {
        GOOGLE_TARGET_SERVICE_ACCOUNT_KEY = module.gcp_target_3[0].target_service_account_key
        GOOGLE_TARGET_PROJECT_ID          = module.gcp_target_3[0].target_project_id
        GOOGLE_TARGET_REGION              = module.gcp_target_3[0].target_region
        ALIEN_E2E_GCP_NETWORK_NAME        = module.gcp_target_3[0].e2e_network_name
        ALIEN_E2E_GCP_SUBNET_NAME         = module.gcp_target_3[0].e2e_subnet_name
        ALIEN_E2E_GCP_REGION              = module.gcp_target_3[0].e2e_network_region
        ALIEN_TEST_K8S_NAMESPACE_PREFIX   = var.e2e_k8s_namespace_prefix
        ALIEN_TEST_K8S_INGRESS_CLASS      = var.e2e_k8s_ingress_class
        ALIEN_TEST_K8S_PUBLIC_HOST_SUFFIX = var.e2e_k8s_public_host_suffix
        ALIEN_TEST_K8S_TLS_SECRET_NAME    = var.e2e_k8s_tls_secret_name
        ALIEN_TEST_GKE_CLUSTER_NAME       = var.e2e_gke_cluster_name
        ALIEN_TEST_GKE_CLUSTER_LOCATION   = var.e2e_gke_cluster_location
        ALIEN_TEST_GKE_KUBE_CONTEXT       = var.e2e_gke_kube_context
      }
    } : {}
  )
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

output "e2e_azure_vnet_resource_id" {
  value     = module.azure.e2e_vnet_resource_id
  sensitive = true
}

output "e2e_azure_public_subnet_name" {
  value     = module.azure.e2e_public_subnet_name
  sensitive = true
}

output "e2e_azure_private_subnet_name" {
  value     = module.azure.e2e_private_subnet_name
  sensitive = true
}

output "azure_target_options" {
  value = {
    azure-target-1 = {
      AZURE_TARGET_SUBSCRIPTION_ID              = var.azure_target_subscription_id
      AZURE_TARGET_TENANT_ID                    = var.azure_target_tenant_id
      AZURE_TARGET_CLIENT_ID                    = module.azure.target_client_id
      AZURE_TARGET_CLIENT_SECRET                = module.azure.target_client_secret
      AZURE_TARGET_REGION                       = var.azure_management_region
      AZURE_TARGET_RESOURCE_GROUP               = module.azure.shared_container_env_resource_group
      AZURE_REGION                              = var.azure_management_region
      ARM_SUBSCRIPTION_ID                       = var.azure_target_subscription_id
      ARM_TENANT_ID                             = var.azure_target_tenant_id
      ARM_CLIENT_ID                             = module.azure.target_client_id
      ARM_CLIENT_SECRET                         = module.azure.target_client_secret
      ALIEN_TEST_AZURE_RESOURCE_GROUP           = module.azure.resource_group
      ALIEN_TEST_AZURE_STORAGE_ACCOUNT          = module.azure.storage_account
      ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER      = module.azure.blob_container
      ALIEN_TEST_AZURE_CONTAINER_APP_IMAGE      = module.azure.container_app_image_uri
      ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME = module.azure.managed_environment
      ALIEN_TEST_AZURE_REGISTRY_NAME            = module.azure.acr_name
      ALIEN_TEST_AZURE_ACR_REPOSITORY           = split(":", module.azure.container_app_image_uri)[0]
      E2E_AZURE_ACR_REPOSITORY                  = module.azure.e2e_acr_repository
      AZURE_SHARED_CONTAINER_ENV_NAME           = module.azure.shared_container_env_name
      AZURE_SHARED_CONTAINER_ENV_RESOURCE_ID    = module.azure.shared_container_env_resource_id
      AZURE_SHARED_CONTAINER_ENV_RESOURCE_GROUP = module.azure.shared_container_env_resource_group
      AZURE_SHARED_CONTAINER_ENV_DEFAULT_DOMAIN = module.azure.shared_container_env_default_domain
      AZURE_SHARED_CONTAINER_ENV_STATIC_IP      = module.azure.shared_container_env_static_ip
      AZURE_SHARED_CONTAINER_ENV_JOIN_ROLE_ID   = module.azure.shared_container_env_join_role_id
      ALIEN_E2E_AZURE_VNET_RESOURCE_ID          = module.azure.e2e_vnet_resource_id
      ALIEN_E2E_AZURE_PUBLIC_SUBNET_NAME        = module.azure.e2e_public_subnet_name
      ALIEN_E2E_AZURE_PRIVATE_SUBNET_NAME       = module.azure.e2e_private_subnet_name
      ALIEN_TEST_K8S_NAMESPACE_PREFIX           = var.e2e_k8s_namespace_prefix
      ALIEN_TEST_K8S_INGRESS_CLASS              = var.e2e_k8s_ingress_class
      ALIEN_TEST_K8S_PUBLIC_HOST_SUFFIX         = var.e2e_k8s_public_host_suffix
      ALIEN_TEST_K8S_TLS_SECRET_NAME            = var.e2e_k8s_tls_secret_name
      ALIEN_TEST_AKS_CLUSTER_NAME               = var.e2e_aks_cluster_name
      ALIEN_TEST_AKS_CLUSTER_RESOURCE_GROUP     = var.e2e_aks_cluster_resource_group
      ALIEN_TEST_AKS_KUBE_CONTEXT               = var.e2e_aks_kube_context
    }
  }
  sensitive = true
}
