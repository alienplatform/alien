#[cfg(feature = "aws")]
use crate::aws_sdk::{
    acm_client_from_alien_config, apigatewayv2_client_from_alien_config,
    codebuild_client_from_alien_config, dynamodb_client_from_alien_config,
    ec2_client_from_alien_config, ecr_client_from_alien_config,
    eventbridge_client_from_alien_config, iam_client_from_alien_config,
    lambda_client_from_alien_config, s3_client_from_alien_config, sqs_client_from_alien_config,
};
use crate::azure_container_apps::{
    ContainerAppsApi, LongRunningOperationApi, OfficialAzureContainerAppsClient,
    OfficialAzureLongRunningOperationClient,
};
use crate::error::Result;
use crate::gcp_cloudrun::cloud_run_services_from_alien_config;
#[cfg(feature = "kubernetes")]
use crate::kubernetes_client::{
    DeploymentApi, EventApi, JobApi, KubernetesClient, MetricsApi, NodeApi, PodApi, RouteApi,
    SecretsApi, ServiceApi, VersionApi,
};
#[cfg(feature = "kubernetes")]
use alien_core::KubernetesClientConfig;
use alien_core::{
    AwsClientConfig, AzureClientConfig, AzureCredentials, GcpClientConfig, GcpCredentials,
    GcpImpersonationConfig,
};
use alien_error::{AlienError, Context, ContextError as _, IntoAlienError, IntoAlienErrorDirect};
use azure_core::{
    cloud::{CloudConfiguration, CustomConfiguration},
    credentials::{AccessToken, Secret, TokenCredential, TokenRequestOptions},
    http::ClientOptions,
    time::{Duration as AzureDuration, OffsetDateTime},
};
use azure_identity::{
    ClientAssertionCredentialOptions, ClientSecretCredential, ClientSecretCredentialOptions,
    ManagedIdentityCredential, ManagedIdentityCredentialOptions, UserAssignedId,
    WorkloadIdentityCredential, WorkloadIdentityCredentialOptions,
};
use azure_mgmt_authorization::package_2022_04_01 as azure_authorization_2022_04;
use azure_mgmt_containerregistry::package_2023_11_preview as azure_containerregistry_2023_11;
use azure_mgmt_keyvault::package_preview_2022_02 as azure_keyvault_2022_02;
use azure_mgmt_msi::package_2023_01_31 as azure_msi_2023_01_31;
use azure_mgmt_network::package_2024_03 as azure_network_2024_03;
use azure_mgmt_resources::package_resources_2021_04 as azure_resources_2021_04;
use azure_mgmt_servicebus::package_2024_01;
use azure_mgmt_storage::package_2023_05 as azure_storage_2023_05;
use google_cloud_api_serviceusage_v1::client::ServiceUsage;
use google_cloud_artifactregistry_v1::client::ArtifactRegistry;
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use google_cloud_compute_v1::client::{
    BackendServices, Firewalls, GlobalAddresses, GlobalForwardingRules, GlobalOperations, Networks,
    RegionNetworkEndpointGroups, RegionOperations, Routers, SslCertificates, Subnetworks,
    TargetHttpsProxies, UrlMaps,
};
use google_cloud_firestore_admin_v1::client::FirestoreAdmin;
use google_cloud_iam_admin_v1::client::Iam;
use google_cloud_iam_v1::client::IAMPolicy;
use google_cloud_pubsub::client::{SubscriptionAdmin, TopicAdmin};
use google_cloud_resourcemanager_v3::client::Projects;
use google_cloud_run_v2::client::Services;
use google_cloud_scheduler_v1::client::CloudScheduler;
use google_cloud_storage::client::StorageControl;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{future::Future, path::PathBuf, sync::Arc, time::Duration};

#[cfg(any(test, feature = "test-utils"))]
use mockall::automock;

const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

#[derive(Debug, Clone)]
struct StaticGcpAccessTokenCredentials {
    token: String,
    entity_tag: EntityTag,
}

impl StaticGcpAccessTokenCredentials {
    fn new(token: String) -> Self {
        Self {
            token,
            entity_tag: EntityTag::new(),
        }
    }
}

impl CredentialsProvider for StaticGcpAccessTokenCredentials {
    fn headers(
        &self,
        _extensions: Extensions,
    ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>> + Send
    {
        let token = self.token.clone();
        let entity_tag = self.entity_tag.clone();
        async move {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| CredentialsError::from_source(false, error))?;
            value.set_sensitive(true);

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);

            Ok(CacheableResource::New {
                entity_tag,
                data: headers,
            })
        }
    }

    fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

#[derive(Debug, Clone)]
pub enum Scope {
    Subscription,
    ResourceGroup {
        resource_group_name: String,
    },
    Resource {
        resource_group_name: String,
        resource_provider: String,
        parent_resource_path: Option<String>,
        resource_type: String,
        resource_name: String,
    },
}

impl Scope {
    pub fn to_scope_string(&self, client_config: &AzureClientConfig) -> String {
        match self {
            Scope::Subscription => format!("subscriptions/{}", client_config.subscription_id),
            Scope::ResourceGroup {
                resource_group_name,
            } => format!(
                "subscriptions/{}/resourceGroups/{}",
                client_config.subscription_id, resource_group_name
            ),
            Scope::Resource {
                resource_group_name,
                resource_provider,
                parent_resource_path,
                resource_type,
                resource_name,
            } => {
                let base = format!(
                    "subscriptions/{}/resourceGroups/{}/providers/{}",
                    client_config.subscription_id, resource_group_name, resource_provider
                );

                if let Some(parent_path) = parent_resource_path {
                    format!(
                        "{}/{}/{}/{}",
                        base, parent_path, resource_type, resource_name
                    )
                } else {
                    format!("{}/{}/{}", base, resource_type, resource_name)
                }
            }
        }
    }

    pub fn to_resource_id_string(&self, client_config: &AzureClientConfig) -> String {
        format!(
            "/{}",
            self.to_scope_string(client_config).trim_start_matches('/')
        )
    }
}

#[derive(Debug)]
pub enum OperationResult<T> {
    Completed(T),
    LongRunning(LongRunningOperation),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LongRunningOperation {
    /// The URL to poll for operation status.
    pub url: String,
    /// Retry delay suggested by Azure.
    pub retry_after: Option<Duration>,
    /// Location header URL for fetching a final operation result.
    #[serde(default)]
    pub location_url: Option<String>,
}

impl LongRunningOperation {
    fn from_azure_core_021_headers(
        headers: &azure_core_021::headers::Headers,
    ) -> Result<Option<Self>> {
        let async_operation_url =
            azure_core_021_header(headers, azure_core_021::headers::AZURE_ASYNCOPERATION);
        let location_url = azure_core_021_header(headers, azure_core_021::headers::LOCATION);
        let url = async_operation_url
            .as_ref()
            .or(location_url.as_ref())
            .cloned();
        let Some(url) = url else {
            return Ok(None);
        };

        let retry_after = azure_core_021_header(headers, azure_core_021::headers::RETRY_AFTER)
            .map(|value| {
                let seconds = value.parse::<u64>().into_alien_error().context(
                    crate::error::ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to parse Azure Retry-After header '{value}' as seconds"
                        ),
                        resource_id: None,
                    },
                )?;
                Ok(Duration::from_secs(seconds))
            })
            .transpose()?;

        Ok(Some(Self {
            url,
            retry_after,
            location_url: async_operation_url.and(location_url),
        }))
    }
}

fn azure_core_021_header(
    headers: &azure_core_021::headers::Headers,
    name: azure_core_021::headers::HeaderName,
) -> Option<String> {
    headers
        .get_optional_str(&name)
        .map(std::string::ToString::to_string)
}

/// Trait that provides methods to get platform service clients.
/// This enables dependency injection for testing by allowing mock clients to be provided.
///
/// For cloud platforms (AWS, GCP, Azure, Kubernetes), this provides API clients.
/// For local platform, this will provide local service managers (function manager, storage manager, etc).
#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait PlatformServiceProvider: Send + Sync {
    // AWS clients
    async fn get_aws_iam_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_iam::Client>;
    async fn get_aws_lambda_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_lambda::Client>;
    async fn get_aws_s3_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_s3::Client>;
    async fn get_aws_codebuild_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_codebuild::Client>;
    async fn get_aws_ecr_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_ecr::Client>;
    async fn get_aws_ssm_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_ssm::Client>;
    async fn get_aws_dynamodb_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_dynamodb::Client>;
    async fn get_aws_sqs_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_sqs::Client>;
    async fn get_aws_ec2_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_ec2::Client>;
    async fn get_aws_acm_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_acm::Client>;
    async fn get_aws_apigatewayv2_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_apigatewayv2::Client>;
    async fn get_aws_eventbridge_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_eventbridge::Client>;

    // GCP clients
    async fn get_gcp_iam_admin_client(&self, config: &GcpClientConfig) -> Result<Iam>;
    async fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Services>;
    async fn get_gcp_resource_manager_client(&self, config: &GcpClientConfig) -> Result<Projects>;
    async fn get_gcp_service_usage_client(&self, config: &GcpClientConfig) -> Result<ServiceUsage>;
    async fn get_gcp_storage_control_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<StorageControl>;
    async fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<ArtifactRegistry>;
    async fn get_gcp_firestore_client(&self, config: &GcpClientConfig) -> Result<FirestoreAdmin>;
    async fn get_gcp_pubsub_topic_admin_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<TopicAdmin>;
    async fn get_gcp_pubsub_subscription_admin_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<SubscriptionAdmin>;
    async fn get_gcp_pubsub_iam_policy_client(&self, config: &GcpClientConfig)
        -> Result<IAMPolicy>;
    async fn get_gcp_compute_networks_client(&self, config: &GcpClientConfig) -> Result<Networks>;
    async fn get_gcp_compute_subnetworks_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Subnetworks>;
    async fn get_gcp_compute_routers_client(&self, config: &GcpClientConfig) -> Result<Routers>;
    async fn get_gcp_compute_firewalls_client(&self, config: &GcpClientConfig)
        -> Result<Firewalls>;
    async fn get_gcp_compute_global_operations_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<GlobalOperations>;
    async fn get_gcp_compute_region_operations_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<RegionOperations>;
    async fn get_gcp_compute_backend_services_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<BackendServices>;
    async fn get_gcp_compute_url_maps_client(&self, config: &GcpClientConfig) -> Result<UrlMaps>;
    async fn get_gcp_compute_target_https_proxies_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<TargetHttpsProxies>;
    async fn get_gcp_compute_ssl_certificates_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<SslCertificates>;
    async fn get_gcp_compute_global_addresses_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<GlobalAddresses>;
    async fn get_gcp_compute_global_forwarding_rules_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<GlobalForwardingRules>;
    async fn get_gcp_compute_region_network_endpoint_groups_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<RegionNetworkEndpointGroups>;
    async fn get_gcp_cloud_scheduler_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<CloudScheduler>;
    // Azure clients
    fn get_azure_authorization_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_authorization_2022_04::Client>;
    fn get_azure_blob_container_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_storage_2023_05::Client>;
    fn get_azure_container_apps_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ContainerAppsApi>>;
    fn get_azure_container_registry_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_containerregistry_2023_11::Client>;
    fn get_azure_long_running_operation_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn LongRunningOperationApi>>;
    fn get_azure_managed_identity_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_msi_2023_01_31::Client>;
    fn get_azure_resources_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_resources_2021_04::Client>;
    fn get_azure_storage_accounts_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_storage_2023_05::Client>;
    fn get_azure_key_vault_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_keyvault_2022_02::Client>;
    fn get_azure_table_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_storage_2023_05::Client>;
    fn get_azure_service_bus_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<package_2024_01::Client>;
    fn get_azure_network_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_network_2024_03::Client>;

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
    async fn get_aws_iam_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_iam::Client> {
        iam_client_from_alien_config(config).await
    }

    async fn get_aws_lambda_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_lambda::Client> {
        lambda_client_from_alien_config(config).await
    }

    async fn get_aws_s3_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_s3::Client> {
        s3_client_from_alien_config(config).await
    }

    async fn get_aws_codebuild_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_codebuild::Client> {
        codebuild_client_from_alien_config(config).await
    }

    async fn get_aws_ecr_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_ecr::Client> {
        ecr_client_from_alien_config(config).await
    }

    async fn get_aws_ssm_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_ssm::Client> {
        crate::aws_sdk::ssm_client_from_alien_config(config).await
    }

    async fn get_aws_dynamodb_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_dynamodb::Client> {
        dynamodb_client_from_alien_config(config).await
    }

    async fn get_aws_sqs_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_sqs::Client> {
        sqs_client_from_alien_config(config).await
    }

    async fn get_aws_ec2_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_ec2::Client> {
        ec2_client_from_alien_config(config).await
    }

    async fn get_aws_acm_client(&self, config: &AwsClientConfig) -> Result<aws_sdk_acm::Client> {
        acm_client_from_alien_config(config).await
    }

    async fn get_aws_apigatewayv2_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_apigatewayv2::Client> {
        apigatewayv2_client_from_alien_config(config).await
    }

    async fn get_aws_eventbridge_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<aws_sdk_eventbridge::Client> {
        eventbridge_client_from_alien_config(config).await
    }

    // GCP implementations
    async fn get_gcp_iam_admin_client(&self, config: &GcpClientConfig) -> Result<Iam> {
        iam_admin_client_from_alien_config(config).await
    }

    async fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Services> {
        cloud_run_services_from_alien_config(config).await
    }

    async fn get_gcp_resource_manager_client(&self, config: &GcpClientConfig) -> Result<Projects> {
        resource_manager_projects_client_from_alien_config(config).await
    }

    async fn get_gcp_service_usage_client(&self, config: &GcpClientConfig) -> Result<ServiceUsage> {
        service_usage_client_from_alien_config(config).await
    }

    async fn get_gcp_storage_control_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<StorageControl> {
        gcs_storage_control_from_alien_config(config).await
    }

    async fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<ArtifactRegistry> {
        artifact_registry_client_from_alien_config(config).await
    }

    async fn get_gcp_firestore_client(&self, config: &GcpClientConfig) -> Result<FirestoreAdmin> {
        firestore_admin_client_from_alien_config(config).await
    }

    async fn get_gcp_pubsub_topic_admin_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<TopicAdmin> {
        pubsub_topic_admin_from_alien_config(config).await
    }

    async fn get_gcp_pubsub_subscription_admin_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<SubscriptionAdmin> {
        pubsub_subscription_admin_from_alien_config(config).await
    }

    async fn get_gcp_pubsub_iam_policy_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<IAMPolicy> {
        pubsub_iam_policy_from_alien_config(config).await
    }

    async fn get_gcp_compute_networks_client(&self, config: &GcpClientConfig) -> Result<Networks> {
        crate::gcp_compute::compute_client_from_alien_config(config, Networks::builder).await
    }

    async fn get_gcp_compute_subnetworks_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Subnetworks> {
        crate::gcp_compute::compute_client_from_alien_config(config, Subnetworks::builder).await
    }

    async fn get_gcp_compute_routers_client(&self, config: &GcpClientConfig) -> Result<Routers> {
        crate::gcp_compute::compute_client_from_alien_config(config, Routers::builder).await
    }

    async fn get_gcp_compute_firewalls_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Firewalls> {
        crate::gcp_compute::compute_client_from_alien_config(config, Firewalls::builder).await
    }

    async fn get_gcp_compute_global_operations_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<GlobalOperations> {
        crate::gcp_compute::compute_client_from_alien_config(config, GlobalOperations::builder)
            .await
    }

    async fn get_gcp_compute_region_operations_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<RegionOperations> {
        crate::gcp_compute::compute_client_from_alien_config(config, RegionOperations::builder)
            .await
    }

    async fn get_gcp_compute_backend_services_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<BackendServices> {
        crate::gcp_compute::compute_client_from_alien_config(config, BackendServices::builder).await
    }

    async fn get_gcp_compute_url_maps_client(&self, config: &GcpClientConfig) -> Result<UrlMaps> {
        crate::gcp_compute::compute_client_from_alien_config(config, UrlMaps::builder).await
    }

    async fn get_gcp_compute_target_https_proxies_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<TargetHttpsProxies> {
        crate::gcp_compute::compute_client_from_alien_config(config, TargetHttpsProxies::builder)
            .await
    }

    async fn get_gcp_compute_ssl_certificates_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<SslCertificates> {
        crate::gcp_compute::compute_client_from_alien_config(config, SslCertificates::builder).await
    }

    async fn get_gcp_compute_global_addresses_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<GlobalAddresses> {
        crate::gcp_compute::compute_client_from_alien_config(config, GlobalAddresses::builder).await
    }

    async fn get_gcp_compute_global_forwarding_rules_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<GlobalForwardingRules> {
        crate::gcp_compute::compute_client_from_alien_config(config, GlobalForwardingRules::builder)
            .await
    }

    async fn get_gcp_compute_region_network_endpoint_groups_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<RegionNetworkEndpointGroups> {
        crate::gcp_compute::compute_client_from_alien_config(
            config,
            RegionNetworkEndpointGroups::builder,
        )
        .await
    }

    async fn get_gcp_cloud_scheduler_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<CloudScheduler> {
        cloud_scheduler_client_from_alien_config(config).await
    }

    // Azure implementations
    fn get_azure_authorization_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_authorization_2022_04::Client> {
        azure_authorization_client_from_alien_config(config)
    }

    fn get_azure_blob_container_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_storage_2023_05::Client> {
        azure_storage_client_from_alien_config(config)
    }

    fn get_azure_container_apps_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ContainerAppsApi>> {
        Ok(Arc::new(OfficialAzureContainerAppsClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_container_registry_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_containerregistry_2023_11::Client> {
        azure_containerregistry_client_from_alien_config(config)
    }

    fn get_azure_long_running_operation_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn LongRunningOperationApi>> {
        Ok(Arc::new(OfficialAzureLongRunningOperationClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_managed_identity_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_msi_2023_01_31::Client> {
        azure_msi_client_from_alien_config(config)
    }

    fn get_azure_resources_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_resources_2021_04::Client> {
        azure_resources_client_from_alien_config(config)
    }

    fn get_azure_storage_accounts_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_storage_2023_05::Client> {
        azure_storage_client_from_alien_config(config)
    }

    fn get_azure_key_vault_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_keyvault_2022_02::Client> {
        azure_keyvault_client_from_alien_config(config)
    }

    fn get_azure_table_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_storage_2023_05::Client> {
        azure_storage_client_from_alien_config(config)
    }

    fn get_azure_service_bus_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<package_2024_01::Client> {
        azure_servicebus_client_from_alien_config(config)
    }

    fn get_azure_network_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<azure_network_2024_03::Client> {
        azure_network_client_from_alien_config(config)
    }

    async fn get_azure_caller_principal_id(&self, config: &AzureClientConfig) -> Result<String> {
        let credential = azure_credential_from_config(config)?;
        let token = credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management token for principal ID resolution"
                    .to_string(),
                resource_id: None,
            })?;
        extract_oid_from_token(token.token.secret()).context(
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

async fn service_usage_client_from_alien_config(config: &GcpClientConfig) -> Result<ServiceUsage> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = ServiceUsage::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("serviceusage"))
    {
        builder = builder.with_endpoint(endpoint.clone());
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Service Usage client".to_string(),
            resource_id: None,
        })
}

async fn iam_admin_client_from_alien_config(config: &GcpClientConfig) -> Result<Iam> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = Iam::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("iam"))
    {
        builder = builder.with_endpoint(endpoint.clone());
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP IAM Admin client".to_string(),
            resource_id: None,
        })
}

async fn pubsub_topic_admin_from_alien_config(config: &GcpClientConfig) -> Result<TopicAdmin> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = TopicAdmin::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("pubsub"))
    {
        builder = builder.with_endpoint(pubsub_admin_endpoint(endpoint));
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Pub/Sub TopicAdmin client".to_string(),
            resource_id: None,
        })
}

async fn pubsub_subscription_admin_from_alien_config(
    config: &GcpClientConfig,
) -> Result<SubscriptionAdmin> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = SubscriptionAdmin::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("pubsub"))
    {
        builder = builder.with_endpoint(pubsub_admin_endpoint(endpoint));
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Pub/Sub SubscriptionAdmin client".to_string(),
            resource_id: None,
        })
}

async fn pubsub_iam_policy_from_alien_config(config: &GcpClientConfig) -> Result<IAMPolicy> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = IAMPolicy::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("pubsub"))
    {
        builder = builder.with_endpoint(pubsub_admin_endpoint(endpoint));
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Pub/Sub IAMPolicy client".to_string(),
            resource_id: None,
        })
}

async fn gcs_storage_control_from_alien_config(config: &GcpClientConfig) -> Result<StorageControl> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = StorageControl::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("storage"))
    {
        builder = builder.with_endpoint(gcs_control_endpoint(endpoint));
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Cloud Storage client".to_string(),
            resource_id: None,
        })
}

async fn firestore_admin_client_from_alien_config(
    config: &GcpClientConfig,
) -> Result<FirestoreAdmin> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = FirestoreAdmin::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("firestore"))
    {
        builder = builder.with_endpoint(endpoint.clone());
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Firestore Admin client".to_string(),
            resource_id: None,
        })
}

async fn artifact_registry_client_from_alien_config(
    config: &GcpClientConfig,
) -> Result<ArtifactRegistry> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = ArtifactRegistry::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("artifactregistry"))
    {
        builder = builder.with_endpoint(endpoint.clone());
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Artifact Registry client".to_string(),
            resource_id: None,
        })
}

async fn cloud_scheduler_client_from_alien_config(
    config: &GcpClientConfig,
) -> Result<CloudScheduler> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = CloudScheduler::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("cloudscheduler"))
    {
        builder = builder.with_endpoint(endpoint.clone());
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Cloud Scheduler client".to_string(),
            resource_id: None,
        })
}

async fn resource_manager_projects_client_from_alien_config(
    config: &GcpClientConfig,
) -> Result<Projects> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = Projects::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("resourcemanager"))
    {
        builder = builder.with_endpoint(endpoint.clone());
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP Resource Manager Projects client".to_string(),
            resource_id: None,
        })
}

fn pubsub_admin_endpoint(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}

fn gcs_control_endpoint(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .trim_end_matches("/storage/v1")
        .trim_end_matches("/upload/storage/v1")
        .to_string()
}

pub(crate) fn gcp_credentials_from_alien_config(config: &GcpClientConfig) -> Result<Credentials> {
    gcp_credentials_from_alien_credentials(&config.credentials)
}

fn gcp_credentials_from_alien_credentials(credentials: &GcpCredentials) -> Result<Credentials> {
    match credentials {
        GcpCredentials::AccessToken { token } => {
            Ok(Credentials::from(StaticGcpAccessTokenCredentials::new(token.clone())))
        }
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<Value>(json).into_alien_error().context(
                crate::error::ErrorData::CloudPlatformError {
                    message: "Failed to parse GCP service account key JSON".to_string(),
                    resource_id: None,
                },
            )?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    [GCP_CLOUD_PLATFORM_SCOPE],
                ))
                .build()
                .into_alien_error()
                .context(crate::error::ErrorData::CloudPlatformError {
                    message: "Failed to build official GCP service account credentials".to_string(),
                    resource_id: None,
                })
        }
        GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
            .with_scopes([GCP_CLOUD_PLATFORM_SCOPE])
            .build()
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to build official GCP metadata server credentials".to_string(),
                resource_id: None,
            }),
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        } => {
            let external_account = gcp_external_account_json(
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url.as_deref(),
            );
            credentials::external_account::Builder::new(external_account)
                .build()
                .into_alien_error()
                .context(crate::error::ErrorData::CloudPlatformError {
                    message: "Failed to build official GCP external account credentials"
                        .to_string(),
                    resource_id: None,
                })
        }
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => {
            let authorized_user = json!({
                "type": "authorized_user",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            });
            credentials::user_account::Builder::new(authorized_user)
                .with_scopes([GCP_CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(crate::error::ErrorData::CloudPlatformError {
                    message: "Failed to build official GCP authorized user credentials".to_string(),
                    resource_id: None,
                })
        }
        GcpCredentials::ImpersonatedServiceAccount { source, config } => {
            gcp_impersonated_credentials_from_alien_config(source, config)
        }
        GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
            crate::error::ErrorData::CloudPlatformError {
                message: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
                resource_id: None,
            },
        )),
    }
}

fn gcp_impersonated_credentials_from_alien_config(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<Credentials> {
    let source_credentials = gcp_credentials_from_alien_config(source)?;
    let mut builder =
        credentials::impersonated::Builder::from_source_credentials(source_credentials)
            .with_target_principal(config.service_account_email.clone())
            .with_scopes(config.scopes.clone());

    if let Some(delegates) = &config.delegates {
        builder = builder.with_delegates(delegates.clone());
    }

    if let Some(lifetime) = &config.lifetime {
        builder = builder.with_lifetime(parse_gcp_duration(lifetime)?);
    }

    builder
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official GCP impersonated service account credentials"
                .to_string(),
            resource_id: None,
        })
}

fn gcp_external_account_json(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> Value {
    let mut value = json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": subject_token_type,
        "token_url": token_url,
        "credential_source": {
            "file": credential_source_file,
        },
        "scopes": [GCP_CLOUD_PLATFORM_SCOPE],
    });

    if let Some(url) = service_account_impersonation_url {
        value["service_account_impersonation_url"] = Value::String(url.to_string());
    }

    value
}

fn parse_gcp_duration(value: &str) -> Result<Duration> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| {
            AlienError::new(crate::error::ErrorData::CloudPlatformError {
                message: format!("Invalid GCP impersonation lifetime '{value}': expected Ns"),
                resource_id: None,
            })
        })?
        .parse::<u64>()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: format!("Invalid GCP impersonation lifetime '{value}'"),
            resource_id: None,
        })?;

    Ok(Duration::from_secs(seconds))
}

#[derive(Debug)]
struct StaticAzureAccessTokenCredential {
    token: String,
}

#[derive(Debug)]
pub(crate) struct AzureCore021Credential {
    inner: Arc<dyn TokenCredential>,
}

impl AzureCore021Credential {
    pub(crate) fn new(inner: Arc<dyn TokenCredential>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl azure_core_021::auth::TokenCredential for AzureCore021Credential {
    async fn get_token(
        &self,
        scopes: &[&str],
    ) -> azure_core_021::Result<azure_core_021::auth::AccessToken> {
        let token = self.inner.get_token(scopes, None).await.map_err(|error| {
            azure_core_021::Error::full(
                azure_core_021::error::ErrorKind::Credential,
                error,
                "failed to get Azure token for generated management client",
            )
        })?;

        Ok(azure_core_021::auth::AccessToken::new(
            token.token.secret().to_string(),
            token.expires_on,
        ))
    }

    async fn clear_cache(&self) -> azure_core_021::Result<()> {
        Ok(())
    }
}

pub(crate) fn map_azure_core_021_sdk_error<T>(
    service_name: &str,
    result: azure_core_021::Result<T>,
    action: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let not_found = matches!(
                error.kind(),
                azure_core_021::error::ErrorKind::HttpResponse {
                    status: azure_core_021::StatusCode::NotFound,
                    ..
                }
            );
            let conflict = matches!(
                error.kind(),
                azure_core_021::error::ErrorKind::HttpResponse {
                    status: azure_core_021::StatusCode::Conflict,
                    ..
                }
            );
            if not_found {
                Err(AlienError::new(
                    crate::error::ErrorData::CloudResourceNotFound {
                        resource_type: resource_type.to_string(),
                        resource_name: resource_name.to_string(),
                    },
                ))
            } else if conflict {
                Err(AlienError::new(
                    crate::error::ErrorData::CloudResourceConflict {
                        resource_type: resource_type.to_string(),
                        resource_name: resource_name.to_string(),
                        message: error.to_string(),
                    },
                ))
            } else {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: format!(
                            "{service_name} {action} failed for {resource_type} '{resource_name}'"
                        ),
                        resource_id: None,
                    }))
            }
        }
    }
}

pub(crate) async fn map_azure_core_021_lro_response<T, R, F, Fut>(
    service_name: &str,
    result: azure_core_021::Result<R>,
    action: &str,
    resource_type: &str,
    resource_name: &str,
    into_body: F,
) -> Result<OperationResult<T>>
where
    R: AsRef<azure_core_021::Response>,
    F: FnOnce(R) -> Fut,
    Fut: std::future::Future<Output = azure_core_021::Result<T>>,
{
    let response =
        map_azure_core_021_sdk_error(service_name, result, action, resource_type, resource_name)?;
    if response.as_ref().status() == azure_core_021::StatusCode::Accepted {
        let operation =
            LongRunningOperation::from_azure_core_021_headers(response.as_ref().headers())?
                .ok_or_else(|| {
                    AlienError::new(crate::error::ErrorData::CloudPlatformError {
                        message: format!(
                            "{service_name} {action} for {resource_type} '{resource_name}' returned 202 without an operation URL"
                        ),
                        resource_id: None,
                    })
                })?;
        Ok(OperationResult::LongRunning(operation))
    } else {
        let body = into_body(response).await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse {service_name} {action} response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;
        Ok(OperationResult::Completed(body))
    }
}

pub(crate) async fn map_azure_core_021_delete_lro_response<R>(
    service_name: &str,
    result: azure_core_021::Result<R>,
    action: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<OperationResult<()>>
where
    R: AsRef<azure_core_021::Response>,
{
    let response =
        map_azure_core_021_sdk_error(service_name, result, action, resource_type, resource_name)?;
    if response.as_ref().status() == azure_core_021::StatusCode::Accepted {
        let operation =
            LongRunningOperation::from_azure_core_021_headers(response.as_ref().headers())?
                .ok_or_else(|| {
                    AlienError::new(crate::error::ErrorData::CloudPlatformError {
                        message: format!(
                            "{service_name} {action} for {resource_type} '{resource_name}' returned 202 without an operation URL"
                        ),
                        resource_id: None,
                    })
                })?;
        Ok(OperationResult::LongRunning(operation))
    } else {
        Ok(OperationResult::Completed(()))
    }
}

#[async_trait::async_trait]
impl TokenCredential for StaticAzureAccessTokenCredential {
    async fn get_token(
        &self,
        scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> azure_core::Result<AccessToken> {
        if scopes.is_empty() {
            return Err(azure_core::Error::with_message(
                azure_core::error::ErrorKind::Credential,
                "no scopes specified",
            ));
        }

        Ok(AccessToken::new(
            self.token.clone(),
            OffsetDateTime::now_utc() + AzureDuration::days(365),
        ))
    }
}

pub(crate) fn azure_credential_from_config(
    config: &AzureClientConfig,
) -> Result<Arc<dyn TokenCredential>> {
    match &config.credentials {
        AzureCredentials::AccessToken { token } => Ok(Arc::new(StaticAzureAccessTokenCredential {
            token: token.clone(),
        })),
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => ClientSecretCredential::new(
            &config.tenant_id,
            client_id.clone(),
            Secret::new(client_secret.clone()),
            Some(ClientSecretCredentialOptions {
                client_options: azure_client_options(None),
            }),
        )
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure service principal credentials".to_string(),
            resource_id: None,
        }),
        AzureCredentials::WorkloadIdentity {
            client_id,
            tenant_id,
            federated_token_file,
            authority_host,
        } => WorkloadIdentityCredential::new(Some(WorkloadIdentityCredentialOptions {
            credential_options: ClientAssertionCredentialOptions {
                client_options: azure_client_options(Some(authority_host)),
            },
            client_id: Some(client_id.clone()),
            tenant_id: Some(tenant_id.clone()),
            token_file_path: Some(PathBuf::from(federated_token_file)),
        }))
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure workload identity credentials".to_string(),
            resource_id: None,
        }),
        AzureCredentials::VmManagedIdentity {
            client_id,
            identity_endpoint,
        } => {
            if let Some(identity_endpoint) = identity_endpoint {
                return Err(AlienError::new(crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Official Azure ManagedIdentityCredential does not support per-config IMDS endpoint override '{}'; use the standard IMDS endpoint or provide an access token",
                        identity_endpoint
                    ),
                    resource_id: None,
                }));
            }

            ManagedIdentityCredential::new(Some(ManagedIdentityCredentialOptions {
                user_assigned_id: Some(UserAssignedId::ClientId(client_id.clone())),
                client_options: azure_client_options(None),
            }))
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to build official Azure VM managed identity credentials"
                    .to_string(),
                resource_id: None,
            })
        }
        AzureCredentials::ManagedIdentity {
            client_id,
            identity_endpoint,
            ..
        } => Err(AlienError::new(crate::error::ErrorData::CloudPlatformError {
            message: format!(
                "Official Azure ManagedIdentityCredential cannot be constructed from explicit App Service identity endpoint '{}' for client '{}'; use workload identity, VM managed identity, or provide an access token",
                identity_endpoint, client_id
            ),
            resource_id: None,
        })),
    }
}

fn azure_client_options(authority_host: Option<&str>) -> ClientOptions {
    let cloud = authority_host.map(|authority_host| {
        let mut custom = CustomConfiguration::default();
        custom.authority_host = authority_host.to_string();
        Arc::new(CloudConfiguration::Custom(custom))
    });

    ClientOptions {
        cloud,
        ..Default::default()
    }
}

pub(crate) fn azure_management_endpoint(config: &AzureClientConfig) -> &str {
    config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("management"))
        .map(String::as_str)
        .unwrap_or("https://management.azure.com")
}

pub(crate) fn azure_containerregistry_client_from_alien_config(
    config: &AzureClientConfig,
) -> Result<azure_containerregistry_2023_11::Client> {
    let endpoint = azure_core_021::Url::parse(azure_management_endpoint(config))
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to parse Azure management endpoint".to_string(),
            resource_id: None,
        })?;
    let credential: Arc<dyn azure_core_021::auth::TokenCredential> = Arc::new(
        AzureCore021Credential::new(azure_credential_from_config(config)?),
    );

    azure_containerregistry_2023_11::Client::builder(credential)
        .endpoint(endpoint)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure Container Registry client".to_string(),
            resource_id: None,
        })
}

pub(crate) fn azure_authorization_client_from_alien_config(
    config: &AzureClientConfig,
) -> Result<azure_authorization_2022_04::Client> {
    let endpoint = azure_core_021::Url::parse(azure_management_endpoint(config))
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to parse Azure management endpoint".to_string(),
            resource_id: None,
        })?;
    let credential: Arc<dyn azure_core_021::auth::TokenCredential> = Arc::new(
        AzureCore021Credential::new(azure_credential_from_config(config)?),
    );

    azure_authorization_2022_04::Client::builder(credential)
        .endpoint(endpoint)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure Authorization client".to_string(),
            resource_id: None,
        })
}

pub(crate) fn azure_resources_client_from_alien_config(
    config: &AzureClientConfig,
) -> Result<azure_resources_2021_04::Client> {
    let endpoint = azure_core_021::Url::parse(azure_management_endpoint(config))
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to parse Azure management endpoint".to_string(),
            resource_id: None,
        })?;
    let credential: Arc<dyn azure_core_021::auth::TokenCredential> = Arc::new(
        AzureCore021Credential::new(azure_credential_from_config(config)?),
    );

    azure_resources_2021_04::Client::builder(credential)
        .endpoint(endpoint)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure Resources client".to_string(),
            resource_id: None,
        })
}

pub(crate) fn azure_keyvault_client_from_alien_config(
    config: &AzureClientConfig,
) -> Result<azure_keyvault_2022_02::Client> {
    let endpoint = azure_core_021::Url::parse(azure_management_endpoint(config))
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to parse Azure management endpoint".to_string(),
            resource_id: None,
        })?;
    let credential: Arc<dyn azure_core_021::auth::TokenCredential> = Arc::new(
        AzureCore021Credential::new(azure_credential_from_config(config)?),
    );

    azure_keyvault_2022_02::Client::builder(credential)
        .endpoint(endpoint)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure Key Vault client".to_string(),
            resource_id: None,
        })
}

pub(crate) fn azure_servicebus_client_from_alien_config(
    config: &AzureClientConfig,
) -> Result<package_2024_01::Client> {
    let endpoint = azure_core_021::Url::parse(azure_management_endpoint(config))
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to parse Azure management endpoint".to_string(),
            resource_id: None,
        })?;
    let credential: Arc<dyn azure_core_021::auth::TokenCredential> = Arc::new(
        AzureCore021Credential::new(azure_credential_from_config(config)?),
    );

    package_2024_01::Client::builder(credential)
        .endpoint(endpoint)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure Service Bus client".to_string(),
            resource_id: None,
        })
}

pub(crate) fn azure_msi_client_from_alien_config(
    config: &AzureClientConfig,
) -> Result<azure_msi_2023_01_31::Client> {
    let endpoint = azure_core_021::Url::parse(azure_management_endpoint(config))
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to parse Azure management endpoint".to_string(),
            resource_id: None,
        })?;
    let credential: Arc<dyn azure_core_021::auth::TokenCredential> = Arc::new(
        AzureCore021Credential::new(azure_credential_from_config(config)?),
    );

    azure_msi_2023_01_31::Client::builder(credential)
        .endpoint(endpoint)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure Managed Identity client".to_string(),
            resource_id: None,
        })
}

pub(crate) fn azure_network_client_from_alien_config(
    config: &AzureClientConfig,
) -> Result<azure_network_2024_03::Client> {
    let endpoint = azure_core_021::Url::parse(azure_management_endpoint(config))
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to parse Azure management endpoint".to_string(),
            resource_id: None,
        })?;
    let credential: Arc<dyn azure_core_021::auth::TokenCredential> = Arc::new(
        AzureCore021Credential::new(azure_credential_from_config(config)?),
    );

    azure_network_2024_03::Client::builder(credential)
        .endpoint(endpoint)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure Network client".to_string(),
            resource_id: None,
        })
}

pub(crate) fn azure_storage_client_from_alien_config(
    config: &AzureClientConfig,
) -> Result<azure_storage_2023_05::Client> {
    let endpoint = azure_core_021::Url::parse(azure_management_endpoint(config))
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to parse Azure management endpoint".to_string(),
            resource_id: None,
        })?;
    let credential: Arc<dyn azure_core_021::auth::TokenCredential> = Arc::new(
        AzureCore021Credential::new(azure_credential_from_config(config)?),
    );

    azure_storage_2023_05::Client::builder(credential)
        .endpoint(endpoint)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::CloudPlatformError {
            message: "Failed to build official Azure Storage client".to_string(),
            resource_id: None,
        })
}

fn extract_oid_from_token(token: &str) -> Result<String> {
    use base64::Engine;

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AlienError::new(crate::error::ErrorData::InvalidInput {
            message: "Azure access token is not a valid JWT (expected 3 parts)".to_string(),
            field_name: None,
        }));
    }

    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .into_alien_error()
        .context(crate::error::ErrorData::InvalidInput {
            message: "Failed to base64-decode Azure JWT payload".to_string(),
            field_name: None,
        })?;

    #[derive(Deserialize)]
    struct JwtClaims {
        oid: Option<String>,
    }

    let claims: JwtClaims = serde_json::from_slice(&payload_bytes)
        .into_alien_error()
        .context(crate::error::ErrorData::InvalidInput {
            message: "Failed to parse Azure JWT payload".to_string(),
            field_name: None,
        })?;

    claims.oid.ok_or_else(|| {
        AlienError::new(crate::error::ErrorData::InvalidInput {
            message: "Azure JWT does not contain 'oid' claim".to_string(),
            field_name: None,
        })
    })
}
