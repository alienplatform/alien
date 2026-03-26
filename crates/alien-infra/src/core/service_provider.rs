use crate::error::Result;
use alien_aws_clients::{
    acm::{AcmApi, AcmClient},
    apigatewayv2::{ApiGatewayV2Api, ApiGatewayV2Client},
    autoscaling::{AutoScalingApi, AutoScalingClient},
    cloudformation::{CloudFormationApi, CloudFormationClient},
    codebuild::{CodeBuildApi, CodeBuildClient},
    dynamodb::{DynamoDbApi, DynamoDbClient},
    ec2::{Ec2Api, Ec2Client},
    ecr::{EcrApi, EcrClient},
    elbv2::{Elbv2Api, Elbv2Client},
    iam::{IamApi, IamClient},
    lambda::{LambdaApi, LambdaClient},
    s3::{S3Api, S3Client},
    secrets_manager::{SecretsManagerApi, SecretsManagerClient},
    sqs::{SqsApi, SqsClient},
    AwsClientConfig, AwsCredentialProvider,
};
use alien_azure_clients::{
    authorization::{AuthorizationApi, AzureAuthorizationClient},
    blob_containers::{AzureBlobContainerClient, BlobContainerApi},
    compute::{AzureVmssClient, VirtualMachineScaleSetsApi},
    container_apps::{AzureContainerAppsClient, ContainerAppsApi},
    containerregistry::{AzureContainerRegistryClient, ContainerRegistryApi},
    disks::{AzureManagedDisksClient, ManagedDisksApi},
    keyvault::{
        AzureKeyVaultCertificatesClient, AzureKeyVaultManagementClient, AzureKeyVaultSecretsClient,
        KeyVaultCertificatesApi, KeyVaultManagementApi, KeyVaultSecretsApi,
    },
    load_balancers::{AzureLoadBalancerClient, LoadBalancerApi},
    long_running_operation::{LongRunningOperationApi, LongRunningOperationClient},
    managed_identity::{AzureManagedIdentityClient, ManagedIdentityApi},
    managed_services::{AzureManagedServicesClient, ManagedServicesApi},
    network::{AzureNetworkClient, NetworkApi as AzureNetworkApi},
    resources::{AzureResourcesClient, ResourcesApi},
    service_bus::{
        AzureServiceBusDataPlaneClient, AzureServiceBusManagementClient, ServiceBusDataPlaneApi,
        ServiceBusManagementApi,
    },
    storage_accounts::{AzureStorageAccountsClient, StorageAccountsApi},
    tables::{AzureTableManagementClient, TableManagementApi},
    AzureClientConfig, AzureTokenCache,
};
use alien_error::Context;
use alien_gcp_clients::{
    artifactregistry::{ArtifactRegistryApi, ArtifactRegistryClient},
    cloudbuild::{CloudBuildApi, CloudBuildClient},
    cloudrun::{CloudRunApi, CloudRunClient},
    compute::{ComputeApi as GcpComputeApi, ComputeClient as GcpComputeClient},
    firestore::{FirestoreApi, FirestoreClient},
    gcs::{GcsApi, GcsClient},
    iam::{IamApi as GcpIamApi, IamClient as GcpIamClient},
    pubsub::{PubSubApi, PubSubClient},
    resource_manager::{ResourceManagerApi, ResourceManagerClient},
    secret_manager::{SecretManagerApi, SecretManagerClient},
    service_usage::{ServiceUsageApi, ServiceUsageClient},
    GcpClientConfig,
};
use alien_k8s_clients::{
    deployments::DeploymentApi, jobs::JobApi, kubernetes_client::KubernetesClient, pods::PodApi,
    secrets::SecretsApi, services::ServiceApi, KubernetesClientConfig,
};
use std::sync::Arc;

#[cfg(test)]
use mockall::automock;

/// Trait that provides methods to get platform service clients.
/// This enables dependency injection for testing by allowing mock clients to be provided.
///
/// For cloud platforms (AWS, GCP, Azure, Kubernetes), this provides API clients.
/// For local platform, this will provide local service managers (function manager, storage manager, etc).
#[cfg_attr(test, automock)]
#[async_trait::async_trait]
pub trait PlatformServiceProvider: Send + Sync {
    // AWS clients
    async fn get_aws_iam_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn IamApi>>;
    async fn get_aws_lambda_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn LambdaApi>>;
    async fn get_aws_s3_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn S3Api>>;
    async fn get_aws_cloudformation_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CloudFormationApi>>;
    async fn get_aws_codebuild_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CodeBuildApi>>;
    async fn get_aws_ecr_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn EcrApi>>;
    async fn get_aws_secrets_manager_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn SecretsManagerApi>>;
    async fn get_aws_dynamodb_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn DynamoDbApi>>;
    async fn get_aws_sqs_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SqsApi>>;
    async fn get_aws_ec2_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn Ec2Api>>;
    async fn get_aws_autoscaling_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn AutoScalingApi>>;
    async fn get_aws_elbv2_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn Elbv2Api>>;
    async fn get_aws_acm_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn AcmApi>>;
    async fn get_aws_apigatewayv2_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn ApiGatewayV2Api>>;

    // GCP clients
    fn get_gcp_iam_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpIamApi>>;
    fn get_gcp_cloudbuild_client(&self, config: &GcpClientConfig)
        -> Result<Arc<dyn CloudBuildApi>>;
    fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn CloudRunApi>>;
    fn get_gcp_resource_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ResourceManagerApi>>;
    fn get_gcp_service_usage_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ServiceUsageApi>>;
    fn get_gcp_gcs_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcsApi>>;
    fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ArtifactRegistryApi>>;
    fn get_gcp_secret_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn SecretManagerApi>>;
    fn get_gcp_firestore_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn FirestoreApi>>;
    fn get_gcp_pubsub_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn PubSubApi>>;
    fn get_gcp_compute_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpComputeApi>>;

    // Azure clients
    fn get_azure_authorization_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AuthorizationApi>>;
    fn get_azure_blob_container_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn BlobContainerApi>>;
    fn get_azure_compute_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn VirtualMachineScaleSetsApi>>;
    fn get_azure_container_apps_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ContainerAppsApi>>;
    fn get_azure_container_registry_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ContainerRegistryApi>>;
    fn get_azure_long_running_operation_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn LongRunningOperationApi>>;
    fn get_azure_load_balancer_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn LoadBalancerApi>>;
    fn get_azure_managed_disks_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedDisksApi>>;
    fn get_azure_managed_identity_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedIdentityApi>>;
    fn get_azure_managed_services_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedServicesApi>>;
    fn get_azure_resources_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ResourcesApi>>;
    fn get_azure_storage_accounts_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn StorageAccountsApi>>;
    fn get_azure_key_vault_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn KeyVaultManagementApi>>;
    fn get_azure_key_vault_secrets_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn KeyVaultSecretsApi>>;
    fn get_azure_key_vault_certificates_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn KeyVaultCertificatesApi>>;
    fn get_azure_table_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn TableManagementApi>>;
    fn get_azure_service_bus_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ServiceBusManagementApi>>;
    fn get_azure_service_bus_data_plane_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ServiceBusDataPlaneApi>>;
    fn get_azure_network_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureNetworkApi>>;

    // Kubernetes clients
    async fn get_kubernetes_deployment_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn DeploymentApi>>;
    async fn get_kubernetes_job_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn JobApi>>;
    async fn get_kubernetes_pod_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn PodApi>>;
    async fn get_kubernetes_secrets_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn SecretsApi>>;
    async fn get_kubernetes_service_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn ServiceApi>>;

    // Local platform service managers (return None for non-local platforms)
    #[cfg(feature = "local")]
    fn get_local_storage_manager(&self) -> Option<Arc<alien_local::LocalStorageManager>>;
    #[cfg(not(feature = "local"))]
    fn get_local_storage_manager(&self) -> Option<Arc<()>> {
        None
    }

    #[cfg(feature = "local")]
    fn get_local_kv_manager(&self) -> Option<Arc<alien_local::LocalKvManager>>;
    #[cfg(not(feature = "local"))]
    fn get_local_kv_manager(&self) -> Option<Arc<()>> {
        None
    }

    #[cfg(feature = "local")]
    fn get_local_vault_manager(&self) -> Option<Arc<alien_local::LocalVaultManager>>;
    #[cfg(not(feature = "local"))]
    fn get_local_vault_manager(&self) -> Option<Arc<()>> {
        None
    }

    #[cfg(feature = "local")]
    fn get_local_function_manager(&self) -> Option<Arc<alien_local::LocalFunctionManager>>;
    #[cfg(not(feature = "local"))]
    fn get_local_function_manager(&self) -> Option<Arc<()>> {
        None
    }

    #[cfg(feature = "local")]
    fn get_local_artifact_registry_manager(
        &self,
    ) -> Option<Arc<alien_local::LocalArtifactRegistryManager>>;
    #[cfg(not(feature = "local"))]
    fn get_local_artifact_registry_manager(&self) -> Option<Arc<()>> {
        None
    }

    #[cfg(feature = "local")]
    fn get_local_container_manager(&self) -> Option<Arc<alien_local::LocalContainerManager>>;
    #[cfg(not(feature = "local"))]
    fn get_local_container_manager(&self) -> Option<Arc<()>> {
        None
    }
}

/// Default implementation that creates real platform service clients.
/// This is used in production and when no mock provider is specified.
///
/// For cloud platforms, creates HTTP API clients on demand.
/// For local platform, stores reference to the local bindings provider.
#[derive(Debug, Clone)]
pub struct DefaultPlatformServiceProvider {
    #[cfg(feature = "local")]
    local_bindings: Option<Arc<alien_local::LocalBindingsProvider>>,
}

impl Default for DefaultPlatformServiceProvider {
    fn default() -> Self {
        Self {
            #[cfg(feature = "local")]
            local_bindings: None,
        }
    }
}

impl DefaultPlatformServiceProvider {
    /// Creates a new service provider with local bindings provider.
    ///
    /// This is used for the local platform to enable controllers to access
    /// local service managers.
    #[cfg(feature = "local")]
    pub fn with_local_bindings(local_bindings: Arc<alien_local::LocalBindingsProvider>) -> Self {
        Self {
            local_bindings: Some(local_bindings),
        }
    }
}

#[async_trait::async_trait]
impl PlatformServiceProvider for DefaultPlatformServiceProvider {
    // AWS implementations
    async fn get_aws_iam_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn IamApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(IamClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_lambda_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn LambdaApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(LambdaClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_s3_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn S3Api>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(S3Client::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_cloudformation_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CloudFormationApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(CloudFormationClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_codebuild_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CodeBuildApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(CodeBuildClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_ecr_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn EcrApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(EcrClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_secrets_manager_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn SecretsManagerApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(SecretsManagerClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_dynamodb_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn DynamoDbApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(DynamoDbClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_sqs_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SqsApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(SqsClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_ec2_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn Ec2Api>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(Ec2Client::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_autoscaling_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn AutoScalingApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(AutoScalingClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_elbv2_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn Elbv2Api>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(Elbv2Client::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_acm_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn AcmApi>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(AcmClient::new(reqwest::Client::new(), credentials)))
    }

    async fn get_aws_apigatewayv2_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn ApiGatewayV2Api>> {
        let credentials = AwsCredentialProvider::from_config(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        Ok(Arc::new(ApiGatewayV2Client::new(reqwest::Client::new(), credentials)))
    }

    // GCP implementations
    fn get_gcp_iam_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpIamApi>> {
        Ok(Arc::new(GcpIamClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_cloudbuild_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn CloudBuildApi>> {
        Ok(Arc::new(CloudBuildClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn CloudRunApi>> {
        Ok(Arc::new(CloudRunClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_resource_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ResourceManagerApi>> {
        Ok(Arc::new(ResourceManagerClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_service_usage_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ServiceUsageApi>> {
        Ok(Arc::new(ServiceUsageClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_gcs_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcsApi>> {
        Ok(Arc::new(GcsClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ArtifactRegistryApi>> {
        Ok(Arc::new(ArtifactRegistryClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_secret_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn SecretManagerApi>> {
        Ok(Arc::new(SecretManagerClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_firestore_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn FirestoreApi>> {
        Ok(Arc::new(FirestoreClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_pubsub_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn PubSubApi>> {
        Ok(Arc::new(PubSubClient::new(reqwest::Client::new(), config.clone())))
    }

    fn get_gcp_compute_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpComputeApi>> {
        Ok(Arc::new(GcpComputeClient::new(reqwest::Client::new(), config.clone())))
    }

    // Azure implementations
    fn get_azure_authorization_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AuthorizationApi>> {
        Ok(Arc::new(AzureAuthorizationClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_blob_container_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn BlobContainerApi>> {
        Ok(Arc::new(AzureBlobContainerClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_compute_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn VirtualMachineScaleSetsApi>> {
        Ok(Arc::new(AzureVmssClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_container_apps_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ContainerAppsApi>> {
        Ok(Arc::new(AzureContainerAppsClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_container_registry_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ContainerRegistryApi>> {
        Ok(Arc::new(AzureContainerRegistryClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_long_running_operation_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn LongRunningOperationApi>> {
        Ok(Arc::new(LongRunningOperationClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_load_balancer_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn LoadBalancerApi>> {
        Ok(Arc::new(AzureLoadBalancerClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_managed_disks_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedDisksApi>> {
        Ok(Arc::new(AzureManagedDisksClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_managed_identity_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedIdentityApi>> {
        Ok(Arc::new(AzureManagedIdentityClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_managed_services_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedServicesApi>> {
        Ok(Arc::new(AzureManagedServicesClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_resources_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ResourcesApi>> {
        Ok(Arc::new(AzureResourcesClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_storage_accounts_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn StorageAccountsApi>> {
        Ok(Arc::new(AzureStorageAccountsClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_key_vault_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn KeyVaultManagementApi>> {
        Ok(Arc::new(AzureKeyVaultManagementClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_key_vault_secrets_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn KeyVaultSecretsApi>> {
        Ok(Arc::new(AzureKeyVaultSecretsClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_key_vault_certificates_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn KeyVaultCertificatesApi>> {
        Ok(Arc::new(AzureKeyVaultCertificatesClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_table_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn TableManagementApi>> {
        Ok(Arc::new(AzureTableManagementClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_service_bus_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ServiceBusManagementApi>> {
        Ok(Arc::new(AzureServiceBusManagementClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_service_bus_data_plane_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ServiceBusDataPlaneApi>> {
        Ok(Arc::new(AzureServiceBusDataPlaneClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    fn get_azure_network_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureNetworkApi>> {
        Ok(Arc::new(AzureNetworkClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
        )))
    }

    // Kubernetes implementations
    async fn get_kubernetes_deployment_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn DeploymentApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes deployment client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    async fn get_kubernetes_job_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn JobApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes job client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    async fn get_kubernetes_pod_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn PodApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes pod client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    async fn get_kubernetes_secrets_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn SecretsApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes secrets client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    async fn get_kubernetes_service_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn ServiceApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes service client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    // Local platform service managers
    #[cfg(feature = "local")]
    fn get_local_storage_manager(&self) -> Option<Arc<alien_local::LocalStorageManager>> {
        self.local_bindings
            .as_ref()
            .map(|p| p.storage_manager().clone())
    }

    #[cfg(feature = "local")]
    fn get_local_kv_manager(&self) -> Option<Arc<alien_local::LocalKvManager>> {
        self.local_bindings.as_ref().map(|p| p.kv_manager().clone())
    }

    #[cfg(feature = "local")]
    fn get_local_vault_manager(&self) -> Option<Arc<alien_local::LocalVaultManager>> {
        self.local_bindings
            .as_ref()
            .map(|p| p.vault_manager().clone())
    }

    #[cfg(feature = "local")]
    fn get_local_function_manager(&self) -> Option<Arc<alien_local::LocalFunctionManager>> {
        self.local_bindings.as_ref().map(|p| p.function_manager())
    }

    #[cfg(feature = "local")]
    fn get_local_artifact_registry_manager(
        &self,
    ) -> Option<Arc<alien_local::LocalArtifactRegistryManager>> {
        self.local_bindings
            .as_ref()
            .map(|p| p.artifact_registry_manager().clone())
    }

    #[cfg(feature = "local")]
    fn get_local_container_manager(&self) -> Option<Arc<alien_local::LocalContainerManager>> {
        self.local_bindings
            .as_ref()
            .and_then(|p| p.container_manager())
    }
}
