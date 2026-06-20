#[cfg(feature = "aws")]
use crate::aws_sdk::{
    acm_client_from_alien_config, apigatewayv2_client_from_alien_config,
    codebuild_client_from_alien_config, dynamodb_client_from_alien_config,
    ec2_client_from_alien_config, ecr_client_from_alien_config,
    eventbridge_client_from_alien_config, iam_client_from_alien_config,
    lambda_client_from_alien_config, s3_client_from_alien_config, sqs_client_from_alien_config,
    AcmApi, ApiGatewayV2Api, CodeBuildApi, DynamoDbApi, Ec2Api, EcrApi, EventBridgeApi, IamApi,
    LambdaApi, S3Api, SqsApi, SsmApi,
};
use crate::error::Result;
#[cfg(feature = "kubernetes")]
use crate::kubernetes_client::{
    DeploymentApi, EventApi, JobApi, KubernetesClient, MetricsApi, NodeApi, PodApi, RouteApi,
    SecretsApi, ServiceApi, VersionApi,
};
use alien_azure_clients::{
    application_gateways::{ApplicationGatewayApi, AzureApplicationGatewayClient},
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
    managed_clusters::{AzureManagedClustersClient, ManagedClustersApi},
    managed_identity::{AzureManagedIdentityClient, ManagedIdentityApi},
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
use alien_core::AwsClientConfig;
#[cfg(feature = "kubernetes")]
use alien_core::KubernetesClientConfig;
use alien_error::Context;
use alien_gcp_clients::{
    artifactregistry::{ArtifactRegistryApi, ArtifactRegistryClient},
    cloudbuild::{CloudBuildApi, CloudBuildClient},
    cloudrun::{CloudRunApi, CloudRunClient},
    cloudscheduler::{CloudSchedulerApi, CloudSchedulerClient},
    compute::{ComputeApi as GcpComputeApi, ComputeClient as GcpComputeClient},
    container::{ContainerApi as GkeContainerApi, ContainerClient as GkeContainerClient},
    firestore::{FirestoreApi, FirestoreClient},
    gcs::{GcsApi, GcsClient},
    iam::{IamApi as GcpIamApi, IamClient as GcpIamClient},
    pubsub::{PubSubApi, PubSubClient},
    resource_manager::{ResourceManagerApi, ResourceManagerClient},
    secret_manager::{SecretManagerApi, SecretManagerClient},
    service_usage::{ServiceUsageApi, ServiceUsageClient},
    GcpClientConfig,
};
use std::sync::Arc;

#[cfg(any(test, feature = "test-utils"))]
use mockall::automock;

/// Trait that provides methods to get platform service clients.
/// This enables dependency injection for testing by allowing mock clients to be provided.
///
/// For cloud platforms (AWS, GCP, Azure, Kubernetes), this provides API clients.
/// For local platform, this will provide local service managers (function manager, storage manager, etc).
#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait PlatformServiceProvider: Send + Sync {
    // AWS clients
    async fn get_aws_iam_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn IamApi>>;
    async fn get_aws_lambda_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn LambdaApi>>;
    async fn get_aws_s3_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn S3Api>>;
    async fn get_aws_codebuild_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CodeBuildApi>>;
    async fn get_aws_ecr_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn EcrApi>>;
    async fn get_aws_ssm_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SsmApi>>;
    async fn get_aws_dynamodb_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn DynamoDbApi>>;
    async fn get_aws_sqs_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SqsApi>>;
    async fn get_aws_ec2_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn Ec2Api>>;
    async fn get_aws_acm_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn AcmApi>>;
    async fn get_aws_apigatewayv2_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn ApiGatewayV2Api>>;
    async fn get_aws_eventbridge_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn EventBridgeApi>>;

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
    fn get_gcp_cloud_scheduler_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn CloudSchedulerApi>>;
    fn get_gcp_container_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn GkeContainerApi>>;

    // Azure clients
    fn get_azure_application_gateway_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ApplicationGatewayApi>>;
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
    fn get_azure_managed_clusters_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedClustersApi>>;
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

    /// Resolve the caller's Azure AD principal ID (object ID) from the current credentials.
    ///
    /// Obtains a management token and extracts the `oid` claim from the JWT.
    /// This is needed because Azure separates management plane (ARM) from data plane —
    /// Contributor grants ARM access but not data actions like Service Bus send.
    /// Controllers use this to self-assign data-plane roles.
    async fn get_azure_caller_principal_id(&self, config: &AzureClientConfig) -> Result<String>;

    // Kubernetes clients
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_deployment_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn DeploymentApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_job_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn JobApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_pod_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn PodApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_event_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn EventApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_node_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn NodeApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_metrics_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn MetricsApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_secrets_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn SecretsApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_service_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn ServiceApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_route_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn RouteApi>>;
    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_version_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn VersionApi>>;

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
    fn get_local_worker_manager(&self) -> Option<Arc<alien_local::LocalWorkerManager>>;
    #[cfg(not(feature = "local"))]
    fn get_local_worker_manager(&self) -> Option<Arc<()>> {
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

    #[cfg(feature = "local")]
    fn get_local_queue_manager(&self) -> Option<Arc<alien_local::LocalQueueManager>>;
    #[cfg(not(feature = "local"))]
    fn get_local_queue_manager(&self) -> Option<Arc<()>> {
        None
    }

    #[cfg(feature = "local")]
    fn get_local_bindings_provider(&self) -> Option<Arc<alien_local::LocalBindingsProvider>>;
    #[cfg(not(feature = "local"))]
    fn get_local_bindings_provider(&self) -> Option<Arc<()>> {
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
        Ok(Arc::new(iam_client_from_alien_config(config).await?))
    }

    async fn get_aws_lambda_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn LambdaApi>> {
        Ok(Arc::new(lambda_client_from_alien_config(config).await?))
    }

    async fn get_aws_s3_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn S3Api>> {
        Ok(Arc::new(s3_client_from_alien_config(config).await?))
    }

    async fn get_aws_codebuild_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CodeBuildApi>> {
        Ok(Arc::new(codebuild_client_from_alien_config(config).await?))
    }

    async fn get_aws_ecr_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn EcrApi>> {
        Ok(Arc::new(ecr_client_from_alien_config(config).await?))
    }

    async fn get_aws_ssm_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SsmApi>> {
        Ok(Arc::new(
            crate::aws_sdk::ssm_client_from_alien_config(config).await?,
        ))
    }

    async fn get_aws_dynamodb_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn DynamoDbApi>> {
        Ok(Arc::new(dynamodb_client_from_alien_config(config).await?))
    }

    async fn get_aws_sqs_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SqsApi>> {
        Ok(Arc::new(sqs_client_from_alien_config(config).await?))
    }

    async fn get_aws_ec2_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn Ec2Api>> {
        Ok(Arc::new(ec2_client_from_alien_config(config).await?))
    }

    async fn get_aws_acm_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn AcmApi>> {
        Ok(Arc::new(acm_client_from_alien_config(config).await?))
    }

    async fn get_aws_apigatewayv2_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn ApiGatewayV2Api>> {
        Ok(Arc::new(
            apigatewayv2_client_from_alien_config(config).await?,
        ))
    }

    async fn get_aws_eventbridge_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn EventBridgeApi>> {
        Ok(Arc::new(
            eventbridge_client_from_alien_config(config).await?,
        ))
    }

    // GCP implementations
    fn get_gcp_iam_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpIamApi>> {
        Ok(Arc::new(GcpIamClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_cloudbuild_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn CloudBuildApi>> {
        Ok(Arc::new(CloudBuildClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn CloudRunApi>> {
        Ok(Arc::new(CloudRunClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_resource_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ResourceManagerApi>> {
        Ok(Arc::new(ResourceManagerClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_service_usage_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ServiceUsageApi>> {
        Ok(Arc::new(ServiceUsageClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_gcs_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcsApi>> {
        Ok(Arc::new(GcsClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ArtifactRegistryApi>> {
        Ok(Arc::new(ArtifactRegistryClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_secret_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn SecretManagerApi>> {
        Ok(Arc::new(SecretManagerClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_firestore_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn FirestoreApi>> {
        Ok(Arc::new(FirestoreClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_pubsub_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn PubSubApi>> {
        Ok(Arc::new(PubSubClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_compute_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpComputeApi>> {
        Ok(Arc::new(GcpComputeClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_cloud_scheduler_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn CloudSchedulerApi>> {
        Ok(Arc::new(CloudSchedulerClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
    }

    fn get_gcp_container_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn GkeContainerApi>> {
        Ok(Arc::new(GkeContainerClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
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

    fn get_azure_application_gateway_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ApplicationGatewayApi>> {
        Ok(Arc::new(AzureApplicationGatewayClient::new(
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

    fn get_azure_managed_clusters_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedClustersApi>> {
        Ok(Arc::new(AzureManagedClustersClient::new(
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

    async fn get_azure_caller_principal_id(&self, config: &AzureClientConfig) -> Result<String> {
        let token_cache = AzureTokenCache::new(config.clone());
        let token = token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management token for principal ID resolution"
                    .to_string(),
                resource_id: None,
            })?;
        alien_azure_clients::extract_oid_from_token(&token).context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to extract principal ID from Azure token".to_string(),
                resource_id: None,
            },
        )
    }

    // Kubernetes implementations
    #[cfg(feature = "kubernetes")]
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

    #[cfg(feature = "kubernetes")]
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

    #[cfg(feature = "kubernetes")]
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

    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_event_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn EventApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes event client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_node_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn NodeApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes node client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_metrics_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn MetricsApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes metrics client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    #[cfg(feature = "kubernetes")]
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

    #[cfg(feature = "kubernetes")]
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

    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_route_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn RouteApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes route client".to_string(),
                resource_id: None,
            },
        )?;
        Ok(Arc::new(client))
    }

    #[cfg(feature = "kubernetes")]
    async fn get_kubernetes_version_client<'a>(
        &'a self,
        config: &'a KubernetesClientConfig,
    ) -> Result<Arc<dyn VersionApi>> {
        let client = KubernetesClient::new(config.clone()).await.context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Failed to create Kubernetes version client".to_string(),
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
    fn get_local_worker_manager(&self) -> Option<Arc<alien_local::LocalWorkerManager>> {
        self.local_bindings.as_ref().map(|p| p.worker_manager())
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

    #[cfg(feature = "local")]
    fn get_local_queue_manager(&self) -> Option<Arc<alien_local::LocalQueueManager>> {
        self.local_bindings
            .as_ref()
            .map(|p| p.queue_manager().clone())
    }

    #[cfg(feature = "local")]
    fn get_local_bindings_provider(&self) -> Option<Arc<alien_local::LocalBindingsProvider>> {
        self.local_bindings.clone()
    }
}
