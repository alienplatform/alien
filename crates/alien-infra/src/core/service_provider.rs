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
use crate::azure_container_apps::{
    ContainerAppsApi, LongRunningOperationApi, OfficialAzureContainerAppsClient,
    OfficialAzureLongRunningOperationClient,
};
use crate::error::Result;
use crate::gcp_cloudrun::{CloudRunApi, OfficialGcpCloudRunClient};
use crate::gcp_compute::{GcpComputeApi, OfficialGcpComputeClient};
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
pub use azure_mgmt_authorization::package_2022_04_01::models::{
    role_assignment_properties::{self, PrincipalType as RoleAssignmentPropertiesPrincipalType},
    Permission, RoleAssignment, RoleAssignmentListResult, RoleAssignmentProperties, RoleDefinition,
    RoleDefinitionProperties,
};
pub use azure_mgmt_containerregistry::package_2023_11_preview::models::{
    encryption_property, network_rule_set, registry_properties, sku, EncryptionProperty,
    KeyVaultProperties, NetworkRuleSet, Policies, Registry, RegistryProperties,
    Resource as AzureContainerRegistryResource, Sku,
};
pub use azure_mgmt_keyvault::package_preview_2022_02::models::{
    Sku as AzureKeyVaultSku, Vault as AzureKeyVault,
    VaultCreateOrUpdateParameters as AzureKeyVaultCreateOrUpdateParameters,
    VaultProperties as AzureKeyVaultProperties,
};
pub use azure_mgmt_msi::package_2023_01_31::models::{
    FederatedIdentityCredential,
    FederatedIdentityCredentialProperties as FederatedCredentialProperties, Identity,
    TrackedResource as AzureManagedIdentityTrackedResource, UserAssignedIdentityProperties,
};
pub use azure_mgmt_network::package_2024_03::models::{
    nat_gateway_sku, public_ip_address_sku, security_rule_properties_format, AddressSpace,
    IpAllocationMethod, NatGateway, NatGatewayPropertiesFormat, NatGatewaySku,
    NetworkSecurityGroup, NetworkSecurityGroupPropertiesFormat, PublicIpAddress,
    PublicIpAddressPropertiesFormat, PublicIpAddressSku, Resource as AzureNetworkResource,
    SecurityRule, SecurityRuleAccess, SecurityRuleDirection, SecurityRulePropertiesFormat,
    SubResource, Subnet, SubnetPropertiesFormat, VirtualNetwork, VirtualNetworkPropertiesFormat,
};
pub use azure_mgmt_resources::package_resources_2021_04::models::{Provider, ResourceGroup};
pub use azure_mgmt_servicebus::package_2024_01::models::{
    MessageCountDetails as AzureServiceBusMessageCountDetails, Resource as AzureServiceBusResource,
    SbNamespace, SbNamespaceProperties as AzureServiceBusNamespaceProperties, SbQueue,
    SbQueueProperties, TrackedResource as AzureServiceBusTrackedResource,
};
pub use azure_mgmt_storage::package_2023_05::models::{
    BlobContainer as AzureBlobContainer, BlobServiceProperties as AzureBlobServiceProperties,
    ContainerProperties as AzureBlobContainerProperties, Endpoints,
    Resource as AzureStorageResource, Sku as AzureStorageSku, SkuName as AzureStorageSkuName,
    StorageAccount, StorageAccountCreateParameters, StorageAccountProperties,
    StorageAccountPropertiesCreateParameters, Table, TableProperties,
};
use bon::Builder;
use google_cloud_api_serviceusage_v1::{client::ServiceUsage, model::Service};
use google_cloud_artifactregistry_v1::client::ArtifactRegistry as OfficialArtifactRegistry;
pub use google_cloud_artifactregistry_v1::model::{
    repository::Format as ArtifactRegistryRepositoryFormat,
    Repository as ArtifactRegistryRepository,
};
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use google_cloud_firestore_admin_v1::{
    client::FirestoreAdmin, model::Database as FirestoreDatabase,
};
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_iam_admin_v1::client::Iam as OfficialGcpIam;
pub use google_cloud_iam_admin_v1::model::{
    role::RoleLaunchStage, CreateRoleRequest, CreateServiceAccountRequest, ListRolesResponse, Role,
    ServiceAccount,
};
pub use google_cloud_iam_v1::model::{Binding, GetPolicyOptions, Policy as IamPolicy};
use google_cloud_longrunning::model::Operation;
use google_cloud_pubsub::client::{
    SubscriptionAdmin as OfficialSubscriptionAdmin, TopicAdmin as OfficialTopicAdmin,
};
pub use google_cloud_pubsub::model::{push_config::OidcToken, PushConfig, Subscription, Topic};
use google_cloud_resourcemanager_v3::client::Projects;
pub use google_cloud_resourcemanager_v3::model::Project;
use google_cloud_scheduler_v1::client::CloudScheduler as OfficialCloudScheduler;
pub use google_cloud_scheduler_v1::model::{
    HttpMethod as SchedulerHttpMethod, HttpTarget, Job as SchedulerJob,
    OidcToken as SchedulerOidcToken,
};
use google_cloud_storage::{
    client::StorageControl as OfficialStorageControl,
    model::{Bucket, DeleteObjectRequest},
};
pub use google_cloud_type::model::Expr;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashMap, future::Future, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::OnceCell;

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

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait GcpIamApi: Send + Sync + std::fmt::Debug {
    async fn create_service_account(
        &self,
        request: CreateServiceAccountRequest,
    ) -> Result<ServiceAccount>;

    async fn delete_service_account(&self, service_account_name: String) -> Result<()>;

    async fn get_service_account(&self, service_account_name: String) -> Result<ServiceAccount>;

    async fn patch_service_account(
        &self,
        service_account_name: String,
        service_account: ServiceAccount,
        update_mask: Option<String>,
    ) -> Result<ServiceAccount>;

    async fn get_service_account_iam_policy(
        &self,
        service_account_name: String,
    ) -> Result<IamPolicy>;

    async fn set_service_account_iam_policy(
        &self,
        service_account_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;

    async fn create_role(&self, request: CreateRoleRequest) -> Result<Role>;

    async fn delete_role(&self, role_name: String) -> Result<Role>;

    async fn undelete_role(&self, role_name: String) -> Result<Role>;

    async fn get_role(&self, role_name: String) -> Result<Role>;

    async fn list_roles(
        &self,
        page_size: Option<i32>,
        page_token: Option<String>,
        show_deleted: Option<bool>,
    ) -> Result<ListRolesResponse>;

    async fn patch_role(
        &self,
        role_name: String,
        role: Role,
        update_mask: Option<String>,
    ) -> Result<Role>;
}

pub type GcpIamPolicy = IamPolicy;

struct OfficialGcpIamClient {
    config: GcpClientConfig,
    client: OnceCell<OfficialGcpIam>,
}

impl std::fmt::Debug for OfficialGcpIamClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpIamClient")
            .field("project_id", &self.config.project_id)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpIamClient {
    fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<OfficialGcpIam> {
        let client = self
            .client
            .get_or_try_init(|| async { iam_admin_client_from_alien_config(&self.config).await })
            .await?;
        Ok(client.clone())
    }

    fn role_name(&self, role_name: &str) -> String {
        if role_name.starts_with("projects/") || role_name.starts_with("organizations/") {
            role_name.to_string()
        } else {
            format!("projects/{}/roles/{role_name}", self.config.project_id)
        }
    }
}

#[async_trait::async_trait]
impl GcpIamApi for OfficialGcpIamClient {
    async fn create_service_account(
        &self,
        mut request: CreateServiceAccountRequest,
    ) -> Result<ServiceAccount> {
        if request.name.is_empty() {
            request.name = format!("projects/{}", self.config.project_id);
        }
        let account_id = request.account_id.clone();

        match self
            .client()
            .await?
            .create_service_account()
            .with_request(request)
            .send()
            .await
        {
            Ok(service_account) => Ok(service_account),
            Err(error) if gax_error_is_conflict(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceConflict {
                    resource_type: "GCP service account".to_string(),
                    resource_name: account_id,
                    message: "create_service_account reported the account already exists"
                        .to_string(),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "IAM create_service_account request failed".to_string(),
                        resource_id: Some(account_id),
                    }))
            }
        }
    }

    async fn delete_service_account(&self, service_account_name: String) -> Result<()> {
        match self
            .client()
            .await?
            .delete_service_account()
            .set_name(service_account_resource_name(
                &self.config.project_id,
                &service_account_name,
            ))
            .send()
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "GCP service account".to_string(),
                    resource_name: service_account_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "IAM delete_service_account request failed".to_string(),
                        resource_id: Some(service_account_name),
                    }))
            }
        }
    }

    async fn get_service_account(&self, service_account_name: String) -> Result<ServiceAccount> {
        match self
            .client()
            .await?
            .get_service_account()
            .set_name(service_account_resource_name(
                &self.config.project_id,
                &service_account_name,
            ))
            .send()
            .await
        {
            Ok(service_account) => Ok(service_account),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "GCP service account".to_string(),
                    resource_name: service_account_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "IAM get_service_account request failed".to_string(),
                        resource_id: Some(service_account_name),
                    }))
            }
        }
    }

    async fn patch_service_account(
        &self,
        service_account_name: String,
        service_account: ServiceAccount,
        update_mask: Option<String>,
    ) -> Result<ServiceAccount> {
        let mut request = google_cloud_iam_admin_v1::model::PatchServiceAccountRequest::new()
            .set_service_account(service_account);
        if let Some(update_mask) = update_mask {
            request = request.set_update_mask(field_mask_from_comma_separated(update_mask));
        }

        self.client()
            .await?
            .patch_service_account()
            .with_request(request)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "IAM patch_service_account request failed".to_string(),
                resource_id: Some(service_account_name),
            })
    }

    async fn get_service_account_iam_policy(
        &self,
        service_account_name: String,
    ) -> Result<IamPolicy> {
        self.client()
            .await?
            .get_iam_policy()
            .set_resource(service_account_resource_name(
                &self.config.project_id,
                &service_account_name,
            ))
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "IAM get_iam_policy request failed".to_string(),
                resource_id: Some(service_account_name),
            })
    }

    async fn set_service_account_iam_policy(
        &self,
        service_account_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        self.client()
            .await?
            .set_iam_policy()
            .set_resource(service_account_resource_name(
                &self.config.project_id,
                &service_account_name,
            ))
            .set_policy(iam_policy)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "IAM set_iam_policy request failed".to_string(),
                resource_id: Some(service_account_name),
            })
    }

    async fn create_role(&self, mut request: CreateRoleRequest) -> Result<Role> {
        if request.parent.is_empty() {
            request.parent = format!("projects/{}", self.config.project_id);
        }
        let role_id = request.role_id.clone();

        match self
            .client()
            .await?
            .create_role()
            .with_request(request)
            .send()
            .await
        {
            Ok(role) => Ok(role),
            Err(error) if gax_error_is_conflict(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceConflict {
                    resource_type: "GCP custom role".to_string(),
                    resource_name: role_id,
                    message: "create_role reported the role already exists".to_string(),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "IAM create_role request failed".to_string(),
                        resource_id: Some(role_id),
                    }))
            }
        }
    }

    async fn delete_role(&self, role_name: String) -> Result<Role> {
        self.client()
            .await?
            .delete_role()
            .set_name(self.role_name(&role_name))
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "IAM delete_role request failed".to_string(),
                resource_id: Some(role_name),
            })
    }

    async fn undelete_role(&self, role_name: String) -> Result<Role> {
        self.client()
            .await?
            .undelete_role()
            .set_name(self.role_name(&role_name))
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "IAM undelete_role request failed".to_string(),
                resource_id: Some(role_name),
            })
    }

    async fn get_role(&self, role_name: String) -> Result<Role> {
        match self
            .client()
            .await?
            .get_role()
            .set_name(self.role_name(&role_name))
            .send()
            .await
        {
            Ok(role) => Ok(role),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "GCP custom role".to_string(),
                    resource_name: role_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "IAM get_role request failed".to_string(),
                        resource_id: Some(role_name),
                    }))
            }
        }
    }

    async fn list_roles(
        &self,
        page_size: Option<i32>,
        page_token: Option<String>,
        show_deleted: Option<bool>,
    ) -> Result<ListRolesResponse> {
        let mut request = self
            .client()
            .await?
            .list_roles()
            .set_parent(format!("projects/{}", self.config.project_id));
        if let Some(page_size) = page_size {
            request = request.set_page_size(page_size);
        }
        if let Some(page_token) = page_token {
            request = request.set_page_token(page_token);
        }
        if let Some(show_deleted) = show_deleted {
            request = request.set_show_deleted(show_deleted);
        }

        request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: "IAM list_roles request failed".to_string(),
                resource_id: Some(self.config.project_id.clone()),
            },
        )
    }

    async fn patch_role(
        &self,
        role_name: String,
        role: Role,
        update_mask: Option<String>,
    ) -> Result<Role> {
        let mut request = self
            .client()
            .await?
            .update_role()
            .set_name(self.role_name(&role_name))
            .set_role(role);
        if let Some(update_mask) = update_mask {
            request = request.set_update_mask(field_mask_from_comma_separated(update_mask));
        }

        request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: "IAM patch_role request failed".to_string(),
                resource_id: Some(role_name),
            },
        )
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait PubSubApi: Send + Sync + std::fmt::Debug {
    async fn create_topic(&self, topic_id: String, topic: Topic) -> Result<Topic>;
    async fn get_topic(&self, topic_id: String) -> Result<Topic>;
    async fn delete_topic(&self, topic_id: String) -> Result<()>;
    async fn set_topic_iam_policy(
        &self,
        topic_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;

    async fn create_subscription(
        &self,
        subscription_id: String,
        subscription: Subscription,
    ) -> Result<Subscription>;
    async fn get_subscription(&self, subscription_id: String) -> Result<Subscription>;
    async fn delete_subscription(&self, subscription_id: String) -> Result<()>;
    async fn set_subscription_iam_policy(
        &self,
        subscription_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;
}

struct OfficialGcpPubSubClient {
    config: GcpClientConfig,
    topic_admin: OnceCell<OfficialTopicAdmin>,
    subscription_admin: OnceCell<OfficialSubscriptionAdmin>,
    credentials: Credentials,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialGcpPubSubClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpPubSubClient")
            .field("project_id", &self.config.project_id)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpPubSubClient {
    fn new(config: GcpClientConfig) -> Result<Self> {
        let credentials = gcp_credentials_from_alien_config(&config)?;
        Ok(Self {
            config,
            topic_admin: OnceCell::new(),
            subscription_admin: OnceCell::new(),
            credentials,
            http_client: reqwest::Client::new(),
        })
    }

    async fn topic_admin(&self) -> Result<OfficialTopicAdmin> {
        let client = self
            .topic_admin
            .get_or_try_init(|| async { pubsub_topic_admin_from_alien_config(&self.config).await })
            .await?;
        Ok(client.clone())
    }

    async fn subscription_admin(&self) -> Result<OfficialSubscriptionAdmin> {
        let client = self
            .subscription_admin
            .get_or_try_init(|| async {
                pubsub_subscription_admin_from_alien_config(&self.config).await
            })
            .await?;
        Ok(client.clone())
    }

    fn topic_name(&self, topic_id: &str) -> String {
        if topic_id.starts_with("projects/") {
            topic_id.to_string()
        } else {
            format!("projects/{}/topics/{topic_id}", self.config.project_id)
        }
    }

    fn subscription_name(&self, subscription_id: &str) -> String {
        if subscription_id.starts_with("projects/") {
            subscription_id.to_string()
        } else {
            format!(
                "projects/{}/subscriptions/{subscription_id}",
                self.config.project_id
            )
        }
    }

    async fn set_iam_policy(&self, resource: String, policy: IamPolicy) -> Result<IamPolicy> {
        let url = format!(
            "{}/{}:setIamPolicy",
            pubsub_rest_endpoint(&self.config),
            resource
        );
        let headers = match self
            .credentials
            .headers(Extensions::new())
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get GCP Pub/Sub authorization headers".to_string(),
                resource_id: Some(resource.clone()),
            })? {
            CacheableResource::New { data, .. } => data,
            CacheableResource::NotModified => {
                return Err(AlienError::new(
                    crate::error::ErrorData::CloudPlatformError {
                        message: "GCP Pub/Sub authorization headers were not refreshed and no cached headers are available".to_string(),
                        resource_id: Some(resource),
                    },
                ));
            }
        };

        let body = json!({ "policy": policy });
        let response = self
            .http_client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Pub/Sub setIamPolicy request failed".to_string(),
                resource_id: Some(resource.clone()),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Pub/Sub setIamPolicy for '{resource}' returned HTTP {}: {text}",
                        status.as_u16()
                    ),
                    resource_id: Some(resource),
                },
            ));
        }

        response
            .json::<IamPolicy>()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to parse Pub/Sub setIamPolicy response".to_string(),
                resource_id: None,
            })
    }
}

#[async_trait::async_trait]
impl PubSubApi for OfficialGcpPubSubClient {
    async fn create_topic(&self, topic_id: String, topic: Topic) -> Result<Topic> {
        let mut topic = topic;
        if topic.name.is_empty() {
            topic.name = self.topic_name(&topic_id);
        }

        match self
            .topic_admin()
            .await?
            .create_topic()
            .with_request(topic)
            .send()
            .await
        {
            Ok(topic) => Ok(topic),
            Err(error) if gax_error_is_conflict(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceConflict {
                    resource_type: "Pub/Sub topic".to_string(),
                    resource_name: self.topic_name(&topic_id),
                    message: "create_topic reported the topic already exists".to_string(),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Pub/Sub create_topic request failed".to_string(),
                        resource_id: Some(topic_id),
                    }))
            }
        }
    }

    async fn get_topic(&self, topic_id: String) -> Result<Topic> {
        match self
            .topic_admin()
            .await?
            .get_topic()
            .set_topic(self.topic_name(&topic_id))
            .send()
            .await
        {
            Ok(topic) => Ok(topic),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Pub/Sub topic".to_string(),
                    resource_name: self.topic_name(&topic_id),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Pub/Sub get_topic request failed".to_string(),
                        resource_id: Some(topic_id),
                    }))
            }
        }
    }

    async fn delete_topic(&self, topic_id: String) -> Result<()> {
        match self
            .topic_admin()
            .await?
            .delete_topic()
            .set_topic(self.topic_name(&topic_id))
            .send()
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Pub/Sub topic".to_string(),
                    resource_name: self.topic_name(&topic_id),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Pub/Sub delete_topic request failed".to_string(),
                        resource_id: Some(topic_id),
                    }))
            }
        }
    }

    async fn set_topic_iam_policy(
        &self,
        topic_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        self.set_iam_policy(self.topic_name(&topic_id), iam_policy)
            .await
    }

    async fn create_subscription(
        &self,
        subscription_id: String,
        subscription: Subscription,
    ) -> Result<Subscription> {
        let mut subscription = subscription;
        if subscription.name.is_empty() {
            subscription.name = self.subscription_name(&subscription_id);
        }

        match self
            .subscription_admin()
            .await?
            .create_subscription()
            .with_request(subscription)
            .send()
            .await
        {
            Ok(subscription) => Ok(subscription),
            Err(error) if gax_error_is_conflict(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceConflict {
                    resource_type: "Pub/Sub subscription".to_string(),
                    resource_name: self.subscription_name(&subscription_id),
                    message: "create_subscription reported the subscription already exists"
                        .to_string(),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Pub/Sub create_subscription request failed".to_string(),
                        resource_id: Some(subscription_id),
                    }))
            }
        }
    }

    async fn get_subscription(&self, subscription_id: String) -> Result<Subscription> {
        match self
            .subscription_admin()
            .await?
            .get_subscription()
            .set_subscription(self.subscription_name(&subscription_id))
            .send()
            .await
        {
            Ok(subscription) => Ok(subscription),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Pub/Sub subscription".to_string(),
                    resource_name: self.subscription_name(&subscription_id),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Pub/Sub get_subscription request failed".to_string(),
                        resource_id: Some(subscription_id),
                    }))
            }
        }
    }

    async fn delete_subscription(&self, subscription_id: String) -> Result<()> {
        match self
            .subscription_admin()
            .await?
            .delete_subscription()
            .set_subscription(self.subscription_name(&subscription_id))
            .send()
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Pub/Sub subscription".to_string(),
                    resource_name: self.subscription_name(&subscription_id),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Pub/Sub delete_subscription request failed".to_string(),
                        resource_id: Some(subscription_id),
                    }))
            }
        }
    }

    async fn set_subscription_iam_policy(
        &self,
        subscription_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        self.set_iam_policy(self.subscription_name(&subscription_id), iam_policy)
            .await
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait GcsApi: Send + Sync + std::fmt::Debug {
    async fn create_bucket(&self, bucket_name: String, bucket: Bucket) -> Result<Bucket>;
    async fn get_bucket(&self, bucket_name: String) -> Result<Bucket>;
    async fn update_bucket(&self, bucket_name: String, bucket_patch: Bucket) -> Result<Bucket>;
    async fn delete_bucket(&self, bucket_name: String) -> Result<()>;
    async fn get_bucket_iam_policy(&self, bucket_name: String) -> Result<IamPolicy>;
    async fn set_bucket_iam_policy(
        &self,
        bucket_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;
    async fn empty_bucket(&self, bucket_name: String) -> Result<()>;
    async fn insert_notification(
        &self,
        bucket_name: String,
        notification: GcsNotification,
    ) -> Result<GcsNotification>;
    async fn list_notifications(&self, bucket_name: String) -> Result<ListNotificationsResponse>;
    async fn delete_notification(&self, bucket_name: String, notification_id: String)
        -> Result<()>;
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListNotificationsResponse {
    /// Response resource kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Notification items.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<GcsNotification>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GcsNotification {
    /// Server-assigned notification ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Pub/Sub topic to publish to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Event types that trigger notification publishing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_types: Vec<String>,
    /// Notification payload format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_format: Option<String>,
    /// Object name prefix filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_name_prefix: Option<String>,
    /// Custom Pub/Sub message attributes.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_attributes: HashMap<String, String>,
}

struct OfficialGcpGcsClient {
    config: GcpClientConfig,
    storage_control: OnceCell<OfficialStorageControl>,
    credentials: Credentials,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialGcpGcsClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpGcsClient")
            .field("project_id", &self.config.project_id)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpGcsClient {
    fn new(config: GcpClientConfig) -> Result<Self> {
        let credentials = gcp_credentials_from_alien_config(&config)?;
        Ok(Self {
            config,
            storage_control: OnceCell::new(),
            credentials,
            http_client: reqwest::Client::new(),
        })
    }

    async fn storage_control(&self) -> Result<OfficialStorageControl> {
        let client = self
            .storage_control
            .get_or_try_init(|| async { gcs_storage_control_from_alien_config(&self.config).await })
            .await?;
        Ok(client.clone())
    }

    fn bucket_resource_name(&self, bucket_name: &str) -> String {
        if bucket_name.starts_with("projects/") {
            bucket_name.to_string()
        } else {
            format!("projects/_/buckets/{bucket_name}")
        }
    }

    async fn auth_headers(&self, resource_id: Option<String>) -> Result<HeaderMap> {
        match self
            .credentials
            .headers(Extensions::new())
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get GCP Cloud Storage authorization headers".to_string(),
                resource_id: resource_id.clone(),
            })? {
            CacheableResource::New { data, .. } => Ok(data),
            CacheableResource::NotModified => Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: "GCP Cloud Storage authorization headers were not refreshed and no cached headers are available".to_string(),
                    resource_id,
                },
            )),
        }
    }

    async fn send_gcs_json<T, B>(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
        resource_id: Option<String>,
        body: Option<&B>,
    ) -> Result<T>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        let headers = self.auth_headers(resource_id.clone()).await?;
        let request = request.headers(headers);
        let request = if let Some(body) = body {
            request.json(body)
        } else {
            request
        };
        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("GCS {operation} request failed"),
                resource_id: resource_id.clone(),
            },
        )?;

        if !response.status().is_success() {
            return Err(gcs_http_error(operation, resource_id, response).await);
        }

        response.json::<T>().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to parse GCS {operation} response"),
                resource_id,
            },
        )
    }

    async fn send_gcs_empty(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
        resource_id: Option<String>,
    ) -> Result<()> {
        let headers = self.auth_headers(resource_id.clone()).await?;
        let response = request
            .headers(headers)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!("GCS {operation} request failed"),
                resource_id: resource_id.clone(),
            })?;

        if !response.status().is_success() {
            return Err(gcs_http_error(operation, resource_id, response).await);
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl GcsApi for OfficialGcpGcsClient {
    async fn create_bucket(&self, bucket_name: String, bucket: Bucket) -> Result<Bucket> {
        match self
            .storage_control()
            .await?
            .create_bucket()
            .set_parent(format!("projects/{}", self.config.project_id))
            .set_bucket_id(bucket_name.clone())
            .set_bucket(bucket)
            .send()
            .await
        {
            Ok(bucket) => Ok(bucket),
            Err(error) if gax_error_is_conflict(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceConflict {
                    resource_type: "GCS bucket".to_string(),
                    resource_name: bucket_name,
                    message: "create_bucket reported the bucket already exists".to_string(),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "GCS create_bucket request failed".to_string(),
                        resource_id: Some(bucket_name),
                    }))
            }
        }
    }

    async fn get_bucket(&self, bucket_name: String) -> Result<Bucket> {
        match self
            .storage_control()
            .await?
            .get_bucket()
            .set_name(self.bucket_resource_name(&bucket_name))
            .send()
            .await
        {
            Ok(bucket) => Ok(bucket),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "GCS bucket".to_string(),
                    resource_name: bucket_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "GCS get_bucket request failed".to_string(),
                        resource_id: Some(bucket_name),
                    }))
            }
        }
    }

    async fn update_bucket(&self, bucket_name: String, bucket_patch: Bucket) -> Result<Bucket> {
        let update_mask = bucket_update_mask(&bucket_patch);
        let mut bucket_patch = bucket_patch;
        if bucket_patch.name.is_empty() {
            bucket_patch.name = self.bucket_resource_name(&bucket_name);
        }

        let mut request =
            google_cloud_storage::model::UpdateBucketRequest::new().set_bucket(bucket_patch);
        if !update_mask.paths.is_empty() {
            request = request.set_update_mask(update_mask);
        }

        match self
            .storage_control()
            .await?
            .update_bucket()
            .with_request(request)
            .send()
            .await
        {
            Ok(bucket) => Ok(bucket),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "GCS bucket".to_string(),
                    resource_name: bucket_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "GCS update_bucket request failed".to_string(),
                        resource_id: Some(bucket_name),
                    }))
            }
        }
    }

    async fn delete_bucket(&self, bucket_name: String) -> Result<()> {
        match self
            .storage_control()
            .await?
            .delete_bucket()
            .set_name(self.bucket_resource_name(&bucket_name))
            .send()
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "GCS bucket".to_string(),
                    resource_name: bucket_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "GCS delete_bucket request failed".to_string(),
                        resource_id: Some(bucket_name),
                    }))
            }
        }
    }

    async fn get_bucket_iam_policy(&self, bucket_name: String) -> Result<IamPolicy> {
        let policy = self
            .storage_control()
            .await?
            .get_iam_policy()
            .set_resource(self.bucket_resource_name(&bucket_name))
            .send()
            .await
            .map_err(|error| {
                if gax_error_is_not_found(&error) {
                    AlienError::new(crate::error::ErrorData::CloudResourceNotFound {
                        resource_type: "GCS bucket IAM policy".to_string(),
                        resource_name: bucket_name.clone(),
                    })
                } else {
                    error
                        .into_alien_error()
                        .context(crate::error::ErrorData::CloudPlatformError {
                            message: "GCS get_bucket_iam_policy request failed".to_string(),
                            resource_id: Some(bucket_name.clone()),
                        })
                }
            })?;
        Ok(policy)
    }

    async fn set_bucket_iam_policy(
        &self,
        bucket_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let policy = iam_policy;
        self.storage_control()
            .await?
            .set_iam_policy()
            .set_resource(self.bucket_resource_name(&bucket_name))
            .set_policy(policy)
            .send()
            .await
            .map_err(|error| {
                if gax_error_is_not_found(&error) {
                    AlienError::new(crate::error::ErrorData::CloudResourceNotFound {
                        resource_type: "GCS bucket IAM policy".to_string(),
                        resource_name: bucket_name.clone(),
                    })
                } else {
                    error
                        .into_alien_error()
                        .context(crate::error::ErrorData::CloudPlatformError {
                            message: "GCS set_bucket_iam_policy request failed".to_string(),
                            resource_id: Some(bucket_name.clone()),
                        })
                }
            })
    }

    async fn empty_bucket(&self, bucket_name: String) -> Result<()> {
        let mut page_token = String::new();
        loop {
            let response = match self
                .storage_control()
                .await?
                .list_objects()
                .set_parent(self.bucket_resource_name(&bucket_name))
                .set_page_size(1000)
                .set_page_token(page_token.clone())
                .set_versions(true)
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) if gax_error_is_not_found(&error) => return Ok(()),
                Err(error) => {
                    return Err(error.into_alien_error().context(
                        crate::error::ErrorData::CloudPlatformError {
                            message: "GCS list_objects request failed while emptying bucket"
                                .to_string(),
                            resource_id: Some(bucket_name),
                        },
                    ));
                }
            };

            for object in response.objects {
                let generation = if object.generation == 0 {
                    None
                } else {
                    Some(object.generation)
                };
                let mut request = DeleteObjectRequest::new()
                    .set_bucket(self.bucket_resource_name(&bucket_name))
                    .set_object(object.name.clone());
                if let Some(generation) = generation {
                    request = request.set_generation(generation);
                }
                match self
                    .storage_control()
                    .await?
                    .delete_object()
                    .with_request(request)
                    .send()
                    .await
                {
                    Ok(()) => {}
                    Err(error) if gax_error_is_not_found(&error) => {}
                    Err(error) => {
                        return Err(error.into_alien_error().context(
                            crate::error::ErrorData::CloudPlatformError {
                                message: format!(
                                    "GCS delete_object request failed while emptying bucket object '{}'",
                                    object.name
                                ),
                                resource_id: Some(bucket_name),
                            },
                        ));
                    }
                }
            }

            if response.next_page_token.is_empty() {
                break;
            }
            page_token = response.next_page_token;
        }

        Ok(())
    }

    async fn insert_notification(
        &self,
        bucket_name: String,
        notification: GcsNotification,
    ) -> Result<GcsNotification> {
        let url = format!(
            "{}/b/{}/notificationConfigs",
            gcs_rest_endpoint(&self.config),
            bucket_name
        );
        self.send_gcs_json(
            self.http_client.post(url),
            "insert_notification",
            Some(bucket_name),
            Some(&notification),
        )
        .await
    }

    async fn list_notifications(&self, bucket_name: String) -> Result<ListNotificationsResponse> {
        let url = format!(
            "{}/b/{}/notificationConfigs",
            gcs_rest_endpoint(&self.config),
            bucket_name
        );
        self.send_gcs_json(
            self.http_client.get(url),
            "list_notifications",
            Some(bucket_name),
            Option::<&()>::None,
        )
        .await
    }

    async fn delete_notification(
        &self,
        bucket_name: String,
        notification_id: String,
    ) -> Result<()> {
        let url = format!(
            "{}/b/{}/notificationConfigs/{}",
            gcs_rest_endpoint(&self.config),
            bucket_name,
            notification_id
        );
        self.send_gcs_empty(
            self.http_client.delete(url),
            "delete_notification",
            Some(bucket_name),
        )
        .await
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait GcpServiceUsageApi: Send + Sync {
    async fn enable_service(&self, service_name: String) -> Result<Operation>;
    async fn get_service(&self, service_name: String) -> Result<Service>;
    async fn get_operation(&self, operation_name: String) -> Result<Operation>;
}

struct OfficialGcpServiceUsageClient {
    config: GcpClientConfig,
    client: OnceCell<ServiceUsage>,
}

impl OfficialGcpServiceUsageClient {
    fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<ServiceUsage> {
        let client = self
            .client
            .get_or_try_init(|| async {
                service_usage_client_from_alien_config(&self.config).await
            })
            .await?;
        Ok(client.clone())
    }
}

#[async_trait::async_trait]
impl GcpServiceUsageApi for OfficialGcpServiceUsageClient {
    async fn enable_service(&self, service_name: String) -> Result<Operation> {
        self.client()
            .await?
            .enable_service()
            .set_name(service_name)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "ServiceUsage enable_service request failed".to_string(),
                resource_id: None,
            })
    }

    async fn get_service(&self, service_name: String) -> Result<Service> {
        self.client()
            .await?
            .get_service()
            .set_name(service_name)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "ServiceUsage get_service request failed".to_string(),
                resource_id: None,
            })
    }

    async fn get_operation(&self, operation_name: String) -> Result<Operation> {
        self.client()
            .await?
            .get_operation()
            .set_name(operation_name)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "ServiceUsage get_operation request failed".to_string(),
                resource_id: None,
            })
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait CloudSchedulerApi: Send + Sync + std::fmt::Debug {
    async fn create_job(
        &self,
        location: String,
        job_id: String,
        job: SchedulerJob,
    ) -> Result<SchedulerJob>;

    async fn delete_job(&self, job_name: String) -> Result<()>;

    async fn get_job(&self, job_name: String) -> Result<SchedulerJob>;
}

struct OfficialGcpCloudSchedulerClient {
    config: GcpClientConfig,
    client: OnceCell<OfficialCloudScheduler>,
}

impl std::fmt::Debug for OfficialGcpCloudSchedulerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpCloudSchedulerClient")
            .field("project_id", &self.config.project_id)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpCloudSchedulerClient {
    fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<OfficialCloudScheduler> {
        let client = self
            .client
            .get_or_try_init(|| async {
                cloud_scheduler_client_from_alien_config(&self.config).await
            })
            .await?;
        Ok(client.clone())
    }
}

#[async_trait::async_trait]
impl CloudSchedulerApi for OfficialGcpCloudSchedulerClient {
    async fn create_job(
        &self,
        location: String,
        job_id: String,
        job: SchedulerJob,
    ) -> Result<SchedulerJob> {
        let job_name =
            cloud_scheduler_job_resource_name(&self.config.project_id, &location, &job_id);
        let mut job = job;
        if job.name.is_empty() {
            job.name = job_name.clone();
        }

        match self
            .client()
            .await?
            .create_job()
            .set_parent(format!(
                "projects/{}/locations/{location}",
                self.config.project_id
            ))
            .set_job(job)
            .send()
            .await
        {
            Ok(job) => Ok(job),
            Err(error) if gax_error_is_conflict(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceConflict {
                    resource_type: "Cloud Scheduler job".to_string(),
                    resource_name: job_name,
                    message: "create_job reported the job already exists".to_string(),
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Cloud Scheduler create_job request failed".to_string(),
                        resource_id: Some(job_id),
                    }))
            }
        }
    }

    async fn delete_job(&self, job_name: String) -> Result<()> {
        match self
            .client()
            .await?
            .delete_job()
            .set_name(job_name.clone())
            .send()
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Cloud Scheduler job".to_string(),
                    resource_name: job_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Cloud Scheduler delete_job request failed".to_string(),
                        resource_id: Some(job_name),
                    }))
            }
        }
    }

    async fn get_job(&self, job_name: String) -> Result<SchedulerJob> {
        match self
            .client()
            .await?
            .get_job()
            .set_name(job_name.clone())
            .send()
            .await
        {
            Ok(job) => Ok(job),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Cloud Scheduler job".to_string(),
                    resource_name: job_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Cloud Scheduler get_job request failed".to_string(),
                        resource_id: Some(job_name),
                    }))
            }
        }
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait GcpFirestoreAdminApi: Send + Sync {
    async fn create_database(
        &self,
        database_id: String,
        database: FirestoreDatabase,
    ) -> Result<Operation>;
    async fn get_database(&self, database_id: String) -> Result<FirestoreDatabase>;
    async fn get_operation(&self, operation_name: String) -> Result<Operation>;
}

struct OfficialGcpFirestoreAdminClient {
    config: GcpClientConfig,
    client: OnceCell<FirestoreAdmin>,
}

impl OfficialGcpFirestoreAdminClient {
    fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<FirestoreAdmin> {
        let client = self
            .client
            .get_or_try_init(|| async {
                firestore_admin_client_from_alien_config(&self.config).await
            })
            .await?;
        Ok(client.clone())
    }

    fn database_resource_name(&self, database_id: &str) -> String {
        format!(
            "projects/{}/databases/{}",
            self.config.project_id, database_id
        )
    }
}

#[async_trait::async_trait]
impl GcpFirestoreAdminApi for OfficialGcpFirestoreAdminClient {
    async fn create_database(
        &self,
        database_id: String,
        database: FirestoreDatabase,
    ) -> Result<Operation> {
        self.client()
            .await?
            .create_database()
            .set_parent(format!("projects/{}", self.config.project_id))
            .set_database_id(database_id)
            .set_database(database)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "FirestoreAdmin create_database request failed".to_string(),
                resource_id: None,
            })
    }

    async fn get_database(&self, database_id: String) -> Result<FirestoreDatabase> {
        let resource_name = self.database_resource_name(&database_id);
        match self
            .client()
            .await?
            .get_database()
            .set_name(resource_name.clone())
            .send()
            .await
        {
            Ok(database) => Ok(database),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Firestore database".to_string(),
                    resource_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "FirestoreAdmin get_database request failed".to_string(),
                        resource_id: None,
                    }))
            }
        }
    }

    async fn get_operation(&self, operation_name: String) -> Result<Operation> {
        self.client()
            .await?
            .get_operation()
            .set_name(operation_name)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "FirestoreAdmin get_operation request failed".to_string(),
                resource_id: None,
            })
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait ArtifactRegistryApi: Send + Sync + std::fmt::Debug {
    async fn create_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        repository: ArtifactRegistryRepository,
    ) -> Result<Operation>;

    async fn delete_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<Operation>;

    async fn get_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<ArtifactRegistryRepository>;

    async fn get_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<GcpIamPolicy>;

    async fn set_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        iam_policy: GcpIamPolicy,
    ) -> Result<GcpIamPolicy>;

    async fn get_operation(
        &self,
        project_id: String,
        location: String,
        operation_name: String,
    ) -> Result<Operation>;
}

struct OfficialGcpArtifactRegistryClient {
    config: GcpClientConfig,
    client: OnceCell<OfficialArtifactRegistry>,
}

impl std::fmt::Debug for OfficialGcpArtifactRegistryClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpArtifactRegistryClient")
            .field("project_id", &self.config.project_id)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpArtifactRegistryClient {
    fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<OfficialArtifactRegistry> {
        let client = self
            .client
            .get_or_try_init(|| async {
                artifact_registry_client_from_alien_config(&self.config).await
            })
            .await?;
        Ok(client.clone())
    }
}

#[async_trait::async_trait]
impl ArtifactRegistryApi for OfficialGcpArtifactRegistryClient {
    async fn create_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        repository: ArtifactRegistryRepository,
    ) -> Result<Operation> {
        self.client()
            .await?
            .create_repository()
            .set_parent(format!("projects/{project_id}/locations/{location}"))
            .set_repository_id(repository_id.clone())
            .set_repository(repository)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Artifact Registry create_repository request failed".to_string(),
                resource_id: Some(repository_id),
            })
    }

    async fn delete_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<Operation> {
        let resource_name =
            artifact_registry_repository_resource_name(&project_id, &location, &repository_id);
        match self
            .client()
            .await?
            .delete_repository()
            .set_name(resource_name.clone())
            .send()
            .await
        {
            Ok(operation) => Ok(operation),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Artifact Registry repository".to_string(),
                    resource_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Artifact Registry delete_repository request failed".to_string(),
                        resource_id: Some(repository_id),
                    }))
            }
        }
    }

    async fn get_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<ArtifactRegistryRepository> {
        let resource_name =
            artifact_registry_repository_resource_name(&project_id, &location, &repository_id);
        match self
            .client()
            .await?
            .get_repository()
            .set_name(resource_name.clone())
            .send()
            .await
        {
            Ok(repository) => Ok(repository),
            Err(error) if gax_error_is_not_found(&error) => Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Artifact Registry repository".to_string(),
                    resource_name,
                },
            )),
            Err(error) => {
                Err(error
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Artifact Registry get_repository request failed".to_string(),
                        resource_id: Some(repository_id),
                    }))
            }
        }
    }

    async fn get_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<GcpIamPolicy> {
        self.client()
            .await?
            .get_iam_policy()
            .set_resource(artifact_registry_repository_resource_name(
                &project_id,
                &location,
                &repository_id,
            ))
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Artifact Registry get_iam_policy request failed".to_string(),
                resource_id: Some(repository_id),
            })
    }

    async fn set_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        iam_policy: GcpIamPolicy,
    ) -> Result<GcpIamPolicy> {
        self.client()
            .await?
            .set_iam_policy()
            .set_resource(artifact_registry_repository_resource_name(
                &project_id,
                &location,
                &repository_id,
            ))
            .set_policy(iam_policy)
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Artifact Registry set_iam_policy request failed".to_string(),
                resource_id: Some(repository_id),
            })
    }

    async fn get_operation(
        &self,
        _project_id: String,
        _location: String,
        operation_name: String,
    ) -> Result<Operation> {
        self.client()
            .await?
            .get_operation()
            .set_name(operation_name.clone())
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Artifact Registry get_operation request failed".to_string(),
                resource_id: Some(operation_name),
            })
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait ResourceManagerApi: Send + Sync + std::fmt::Debug {
    async fn get_project_iam_policy(
        &self,
        project_id: String,
        options: Option<GetPolicyOptions>,
    ) -> Result<GcpIamPolicy>;

    async fn set_project_iam_policy(
        &self,
        project_id: String,
        policy: GcpIamPolicy,
        update_mask: Option<String>,
    ) -> Result<GcpIamPolicy>;

    async fn get_project_metadata(&self, project_id: String) -> Result<Project>;
}

struct OfficialGcpResourceManagerClient {
    config: GcpClientConfig,
    client: OnceCell<Projects>,
}

impl std::fmt::Debug for OfficialGcpResourceManagerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpResourceManagerClient")
            .field("project_id", &self.config.project_id)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpResourceManagerClient {
    fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<Projects> {
        let client = self
            .client
            .get_or_try_init(|| async {
                resource_manager_projects_client_from_alien_config(&self.config).await
            })
            .await?;
        Ok(client.clone())
    }
}

#[async_trait::async_trait]
impl ResourceManagerApi for OfficialGcpResourceManagerClient {
    async fn get_project_iam_policy(
        &self,
        project_id: String,
        options: Option<GetPolicyOptions>,
    ) -> Result<GcpIamPolicy> {
        let mut request = self
            .client()
            .await?
            .get_iam_policy()
            .set_resource(format!("projects/{project_id}"));
        if let Some(options) = options {
            request = request.set_options(options);
        }

        request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Resource Manager get_iam_policy request failed".to_string(),
                resource_id: Some(project_id),
            },
        )
    }

    async fn set_project_iam_policy(
        &self,
        project_id: String,
        policy: GcpIamPolicy,
        update_mask: Option<String>,
    ) -> Result<GcpIamPolicy> {
        if let Some(update_mask) = update_mask {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Resource Manager set_project_iam_policy update_mask '{update_mask}' is not supported by the official adapter yet"
                    ),
                    resource_id: Some(project_id),
                },
            ));
        }

        let request = self
            .client()
            .await?
            .set_iam_policy()
            .set_resource(format!("projects/{project_id}"))
            .set_policy(policy);

        request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: "Resource Manager set_iam_policy request failed".to_string(),
                resource_id: Some(project_id),
            },
        )
    }

    async fn get_project_metadata(&self, project_id: String) -> Result<Project> {
        self.client()
            .await?
            .get_project()
            .set_name(format!("projects/{project_id}"))
            .send()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Resource Manager get_project request failed".to_string(),
                resource_id: Some(project_id),
            })
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

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait AuthorizationApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
        role_definition: &RoleDefinition,
    ) -> Result<RoleDefinition>;

    async fn delete_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<Option<RoleDefinition>>;

    async fn get_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<RoleDefinition>;

    async fn create_or_update_role_assignment_by_id(
        &self,
        role_assignment_id: String,
        role_assignment: &RoleAssignment,
    ) -> Result<RoleAssignment>;

    async fn delete_role_assignment_by_id(
        &self,
        role_assignment_id: String,
    ) -> Result<Option<RoleAssignment>>;

    async fn get_role_assignment_by_id(&self, role_assignment_id: String)
        -> Result<RoleAssignment>;

    async fn list_role_assignments(
        &self,
        scope: &Scope,
        role_definition_id: Option<String>,
    ) -> Result<Vec<RoleAssignment>>;

    fn build_role_assignment_id(&self, scope: &Scope, role_assignment_name: String) -> String;

    fn build_resource_group_role_assignment_id(
        &self,
        resource_group_name: String,
        role_assignment_name: String,
    ) -> String;

    fn build_resource_role_assignment_id(
        &self,
        resource_group_name: String,
        resource_provider: String,
        parent_resource_path: Option<String>,
        resource_type: String,
        resource_name: String,
        role_assignment_name: String,
    ) -> String;
}

struct OfficialAzureAuthorizationClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialAzureAuthorizationClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureAuthorizationClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureAuthorizationClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn arm_url(&self, path: &str, api_version: &str) -> String {
        format!(
            "{}/{}?api-version={}",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            path.trim_start_matches('/'),
            api_version
        )
    }

    fn role_definition_url(&self, scope: &Scope, role_definition_id: &str) -> String {
        self.arm_url(
            &format!(
                "/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                scope.to_scope_string(&self.config),
                role_definition_id
            ),
            "2022-04-01",
        )
    }

    fn role_assignment_url(&self, role_assignment_id: &str) -> String {
        self.arm_url(role_assignment_id, "2022-04-01")
    }

    fn role_assignments_url(&self, scope: &Scope) -> String {
        format!(
            "{}/{}/providers/Microsoft.Authorization/roleAssignments?api-version=2022-04-01&$filter=atScope()",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            scope.to_scope_string(&self.config)
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<String> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if status == StatusCode::CONFLICT {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceConflict {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                    message: text,
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok(text)
    }
}

#[async_trait::async_trait]
impl AuthorizationApi for OfficialAzureAuthorizationClient {
    async fn create_or_update_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
        role_definition: &RoleDefinition,
    ) -> Result<RoleDefinition> {
        let body = serde_json::to_string(role_definition)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to serialize Azure role definition request".to_string(),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PUT,
                self.role_definition_url(scope, &role_definition_id),
                Some(body),
                "Azure role definition",
                &role_definition_id,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure role definition '{role_definition_id}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn delete_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<Option<RoleDefinition>> {
        let response = self
            .request(
                Method::DELETE,
                self.role_definition_url(scope, &role_definition_id),
                None,
                "Azure role definition",
                &role_definition_id,
            )
            .await?;
        if response.is_empty() {
            Ok(None)
        } else {
            serde_json::from_str(&response)
                .map(Some)
                .into_alien_error()
                .context(crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to parse Azure role definition '{role_definition_id}' delete response"
                    ),
                    resource_id: None,
                })
        }
    }

    async fn get_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<RoleDefinition> {
        let response = self
            .request(
                Method::GET,
                self.role_definition_url(scope, &role_definition_id),
                None,
                "Azure role definition",
                &role_definition_id,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure role definition '{role_definition_id}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn create_or_update_role_assignment_by_id(
        &self,
        role_assignment_id: String,
        role_assignment: &RoleAssignment,
    ) -> Result<RoleAssignment> {
        let body = serde_json::to_string(role_assignment)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to serialize Azure role assignment request".to_string(),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PUT,
                self.role_assignment_url(&role_assignment_id),
                Some(body),
                "Azure role assignment",
                &role_assignment_id,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure role assignment '{role_assignment_id}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn delete_role_assignment_by_id(
        &self,
        role_assignment_id: String,
    ) -> Result<Option<RoleAssignment>> {
        let response = self
            .request(
                Method::DELETE,
                self.role_assignment_url(&role_assignment_id),
                None,
                "Azure role assignment",
                &role_assignment_id,
            )
            .await?;
        if response.is_empty() {
            Ok(None)
        } else {
            serde_json::from_str(&response)
                .map(Some)
                .into_alien_error()
                .context(crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to parse Azure role assignment '{role_assignment_id}' delete response"
                    ),
                    resource_id: None,
                })
        }
    }

    async fn get_role_assignment_by_id(
        &self,
        role_assignment_id: String,
    ) -> Result<RoleAssignment> {
        let response = self
            .request(
                Method::GET,
                self.role_assignment_url(&role_assignment_id),
                None,
                "Azure role assignment",
                &role_assignment_id,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure role assignment '{role_assignment_id}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn list_role_assignments(
        &self,
        scope: &Scope,
        role_definition_id: Option<String>,
    ) -> Result<Vec<RoleAssignment>> {
        let scope_string = scope.to_scope_string(&self.config);
        let response = self
            .request(
                Method::GET,
                self.role_assignments_url(scope),
                None,
                "Azure role assignments",
                &scope_string,
            )
            .await?;
        let response = serde_json::from_str::<RoleAssignmentListResult>(&response)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to parse Azure role assignments for '{scope_string}'"),
                resource_id: None,
            })?;

        let assignments = if let Some(role_definition_id) = role_definition_id {
            response
                .value
                .into_iter()
                .filter(|assignment| {
                    assignment.properties.as_ref().is_some_and(|properties| {
                        properties.role_definition_id == role_definition_id
                    })
                })
                .collect()
        } else {
            response.value
        };
        Ok(assignments)
    }

    fn build_role_assignment_id(&self, scope: &Scope, role_assignment_name: String) -> String {
        format!(
            "/{}/providers/Microsoft.Authorization/roleAssignments/{}",
            scope.to_scope_string(&self.config),
            role_assignment_name
        )
    }

    fn build_resource_group_role_assignment_id(
        &self,
        resource_group_name: String,
        role_assignment_name: String,
    ) -> String {
        self.build_role_assignment_id(
            &Scope::ResourceGroup {
                resource_group_name,
            },
            role_assignment_name,
        )
    }

    fn build_resource_role_assignment_id(
        &self,
        resource_group_name: String,
        resource_provider: String,
        parent_resource_path: Option<String>,
        resource_type: String,
        resource_name: String,
        role_assignment_name: String,
    ) -> String {
        self.build_role_assignment_id(
            &Scope::Resource {
                resource_group_name,
                resource_provider,
                parent_resource_path,
                resource_type,
                resource_name,
            },
            role_assignment_name,
        )
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait ManagedIdentityApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
        identity: &Identity,
    ) -> Result<Identity>;

    async fn delete_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<()>;

    async fn get_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<Identity>;

    async fn create_or_update_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
        credential: &FederatedIdentityCredential,
    ) -> Result<FederatedIdentityCredential>;

    async fn get_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
    ) -> Result<FederatedIdentityCredential>;

    async fn delete_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
    ) -> Result<()>;

    fn build_user_assigned_identity_id(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> String;
}

struct OfficialAzureManagedIdentityClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialAzureManagedIdentityClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureManagedIdentityClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureManagedIdentityClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn user_assigned_identity_url(&self, resource_group_name: &str, resource_name: &str) -> String {
        format!(
            "{}/{}?api-version=2023-01-31",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.build_user_assigned_identity_id(resource_group_name, resource_name)
                .trim_start_matches('/')
        )
    }

    fn federated_credential_url(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
    ) -> String {
        format!(
            "{}/{}/federatedIdentityCredentials/{}?api-version=2023-01-31",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.build_user_assigned_identity_id(resource_group_name, identity_name)
                .trim_start_matches('/'),
            credential_name
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<String> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok(text)
    }
}

#[async_trait::async_trait]
impl ManagedIdentityApi for OfficialAzureManagedIdentityClient {
    async fn create_or_update_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
        identity: &Identity,
    ) -> Result<Identity> {
        let body = serde_json::to_string(identity).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure managed identity '{resource_name}' request"
                ),
                resource_id: None,
            },
        )?;
        let response = self
            .request(
                Method::PUT,
                self.user_assigned_identity_url(resource_group_name, resource_name),
                Some(body),
                "Azure managed identity",
                resource_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure managed identity '{resource_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn delete_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<()> {
        self.request(
            Method::DELETE,
            self.user_assigned_identity_url(resource_group_name, resource_name),
            None,
            "Azure managed identity",
            resource_name,
        )
        .await?;
        Ok(())
    }

    async fn get_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<Identity> {
        let response = self
            .request(
                Method::GET,
                self.user_assigned_identity_url(resource_group_name, resource_name),
                None,
                "Azure managed identity",
                resource_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure managed identity '{resource_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn create_or_update_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
        credential: &FederatedIdentityCredential,
    ) -> Result<FederatedIdentityCredential> {
        let body = serde_json::to_string(credential)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure federated credential '{credential_name}' request"
                ),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PUT,
                self.federated_credential_url(resource_group_name, identity_name, credential_name),
                Some(body),
                "Azure federated identity credential",
                credential_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure federated credential '{credential_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn get_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
    ) -> Result<FederatedIdentityCredential> {
        let response = self
            .request(
                Method::GET,
                self.federated_credential_url(resource_group_name, identity_name, credential_name),
                None,
                "Azure federated identity credential",
                credential_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure federated credential '{credential_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn delete_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
    ) -> Result<()> {
        self.request(
            Method::DELETE,
            self.federated_credential_url(resource_group_name, identity_name, credential_name),
            None,
            "Azure federated identity credential",
            credential_name,
        )
        .await?;
        Ok(())
    }

    fn build_user_assigned_identity_id(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}",
            self.config.subscription_id, resource_group_name, resource_name
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
    fn from_response_headers(headers: &reqwest::header::HeaderMap) -> Result<Option<Self>> {
        let async_operation_url = headers
            .get("azure-asyncoperation")
            .map(|value| {
                value.to_str().into_alien_error().context(
                    crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure-AsyncOperation header".to_string(),
                        resource_id: None,
                    },
                )
            })
            .transpose()?
            .map(ToString::to_string);

        let location_url = headers
            .get("location")
            .map(|value| {
                value.to_str().into_alien_error().context(
                    crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure Location header".to_string(),
                        resource_id: None,
                    },
                )
            })
            .transpose()?
            .map(ToString::to_string);

        let url = async_operation_url
            .as_ref()
            .or(location_url.as_ref())
            .cloned();
        let Some(url) = url else {
            return Ok(None);
        };

        let retry_after = headers
            .get("retry-after")
            .map(|value| {
                let value = value.to_str().into_alien_error().context(
                    crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure Retry-After header".to_string(),
                        resource_id: None,
                    },
                )?;
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

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait ContainerRegistryApi: Send + Sync + std::fmt::Debug {
    async fn create_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &Registry,
    ) -> Result<OperationResult<Registry>>;

    async fn delete_registry(&self, resource_group_name: &str, registry_name: &str) -> Result<()>;

    async fn get_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Registry>;
}

struct OfficialAzureContainerRegistryClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialAzureContainerRegistryClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureContainerRegistryClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureContainerRegistryClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn registry_url(&self, resource_group_name: &str, registry_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}?api-version=2025-04-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            registry_name
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<(StatusCode, reqwest::header::HeaderMap, String)> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok((status, headers, text))
    }

    fn parse_response<T: DeserializeOwned>(
        &self,
        resource_type: &str,
        resource_name: &str,
        body: &str,
    ) -> Result<T> {
        serde_json::from_str(body).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure ARM {resource_type} '{resource_name}' response"
                ),
                resource_id: None,
            },
        )
    }
}

#[async_trait::async_trait]
impl ContainerRegistryApi for OfficialAzureContainerRegistryClient {
    async fn create_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        parameters: &Registry,
    ) -> Result<OperationResult<Registry>> {
        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure Container Registry '{registry_name}' request"
                ),
                resource_id: None,
            })?;
        let (status, headers, response) = self
            .request(
                Method::PUT,
                self.registry_url(resource_group_name, registry_name),
                Some(body),
                "Azure Container Registry",
                registry_name,
            )
            .await?;

        if status == StatusCode::ACCEPTED {
            let operation = LongRunningOperation::from_response_headers(&headers)?.ok_or_else(|| {
                AlienError::new(crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure Container Registry '{registry_name}' returned 202 without an operation URL"
                    ),
                    resource_id: None,
                })
            })?;
            Ok(OperationResult::LongRunning(operation))
        } else {
            Ok(OperationResult::Completed(self.parse_response(
                "Azure Container Registry",
                registry_name,
                &response,
            )?))
        }
    }

    async fn delete_registry(&self, resource_group_name: &str, registry_name: &str) -> Result<()> {
        self.request(
            Method::DELETE,
            self.registry_url(resource_group_name, registry_name),
            None,
            "Azure Container Registry",
            registry_name,
        )
        .await?;
        Ok(())
    }

    async fn get_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Registry> {
        let (_, _, response) = self
            .request(
                Method::GET,
                self.registry_url(resource_group_name, registry_name),
                None,
                "Azure Container Registry",
                registry_name,
            )
            .await?;
        self.parse_response("Azure Container Registry", registry_name, &response)
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait AzureNetworkApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        virtual_network: &VirtualNetwork,
    ) -> Result<OperationResult<VirtualNetwork>>;

    async fn get_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<VirtualNetwork>;

    async fn delete_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<OperationResult<()>>;

    async fn create_or_update_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
        subnet: &Subnet,
    ) -> Result<OperationResult<Subnet>>;

    async fn get_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<Subnet>;

    async fn delete_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<OperationResult<()>>;

    async fn create_or_update_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
        nat_gateway: &NatGateway,
    ) -> Result<OperationResult<NatGateway>>;

    async fn get_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<NatGateway>;

    async fn delete_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<OperationResult<()>>;

    async fn create_or_update_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
        public_ip_address: &PublicIpAddress,
    ) -> Result<OperationResult<PublicIpAddress>>;

    async fn get_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<PublicIpAddress>;

    async fn delete_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<OperationResult<()>>;

    async fn create_or_update_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
        network_security_group: &NetworkSecurityGroup,
    ) -> Result<OperationResult<NetworkSecurityGroup>>;

    async fn get_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<NetworkSecurityGroup>;

    async fn delete_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<OperationResult<()>>;
}

struct OfficialAzureNetworkClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialAzureNetworkClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureNetworkClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureNetworkClient {
    const API_VERSION: &'static str = "2024-05-01";

    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn network_resource_url(&self, resource_group_name: &str, path: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/{}?api-version={}",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            path.trim_start_matches('/'),
            Self::API_VERSION
        )
    }

    fn virtual_network_url(&self, resource_group_name: &str, virtual_network_name: &str) -> String {
        self.network_resource_url(
            resource_group_name,
            &format!("virtualNetworks/{virtual_network_name}"),
        )
    }

    fn subnet_url(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> String {
        self.network_resource_url(
            resource_group_name,
            &format!("virtualNetworks/{virtual_network_name}/subnets/{subnet_name}"),
        )
    }

    fn nat_gateway_url(&self, resource_group_name: &str, nat_gateway_name: &str) -> String {
        self.network_resource_url(
            resource_group_name,
            &format!("natGateways/{nat_gateway_name}"),
        )
    }

    fn public_ip_url(&self, resource_group_name: &str, public_ip_name: &str) -> String {
        self.network_resource_url(
            resource_group_name,
            &format!("publicIPAddresses/{public_ip_name}"),
        )
    }

    fn network_security_group_url(&self, resource_group_name: &str, nsg_name: &str) -> String {
        self.network_resource_url(
            resource_group_name,
            &format!("networkSecurityGroups/{nsg_name}"),
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<(StatusCode, reqwest::header::HeaderMap, String)> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok((status, headers, text))
    }

    fn parse_response<T: DeserializeOwned>(
        resource_type: &str,
        resource_name: &str,
        body: &str,
    ) -> Result<T> {
        serde_json::from_str(body).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure ARM {resource_type} '{resource_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn put_resource<T, B>(
        &self,
        url: String,
        body: &B,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<OperationResult<T>>
    where
        T: DeserializeOwned,
        B: Serialize + Sync,
    {
        let body = serde_json::to_string(body).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure ARM {resource_type} '{resource_name}' request"
                ),
                resource_id: None,
            },
        )?;
        let (status, headers, response) = self
            .request(Method::PUT, url, Some(body), resource_type, resource_name)
            .await?;
        self.operation_result(status, headers, response, resource_type, resource_name)
    }

    async fn delete_resource(
        &self,
        url: String,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<OperationResult<()>> {
        let (status, headers, _) = self
            .request(Method::DELETE, url, None, resource_type, resource_name)
            .await?;
        if status == StatusCode::ACCEPTED {
            let operation = LongRunningOperation::from_response_headers(&headers)?.ok_or_else(|| {
                AlienError::new(crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM {resource_type} '{resource_name}' returned 202 without an operation URL"
                    ),
                    resource_id: None,
                })
            })?;
            Ok(OperationResult::LongRunning(operation))
        } else {
            Ok(OperationResult::Completed(()))
        }
    }

    fn operation_result<T: DeserializeOwned>(
        &self,
        status: StatusCode,
        headers: reqwest::header::HeaderMap,
        response: String,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<OperationResult<T>> {
        if status == StatusCode::ACCEPTED {
            let operation = LongRunningOperation::from_response_headers(&headers)?.ok_or_else(|| {
                AlienError::new(crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM {resource_type} '{resource_name}' returned 202 without an operation URL"
                    ),
                    resource_id: None,
                })
            })?;
            Ok(OperationResult::LongRunning(operation))
        } else {
            Ok(OperationResult::Completed(Self::parse_response(
                resource_type,
                resource_name,
                &response,
            )?))
        }
    }
}

#[async_trait::async_trait]
impl AzureNetworkApi for OfficialAzureNetworkClient {
    async fn create_or_update_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        virtual_network: &VirtualNetwork,
    ) -> Result<OperationResult<VirtualNetwork>> {
        self.put_resource(
            self.virtual_network_url(resource_group_name, virtual_network_name),
            virtual_network,
            "Azure virtual network",
            virtual_network_name,
        )
        .await
    }

    async fn get_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<VirtualNetwork> {
        let (_, _, response) = self
            .request(
                Method::GET,
                self.virtual_network_url(resource_group_name, virtual_network_name),
                None,
                "Azure virtual network",
                virtual_network_name,
            )
            .await?;
        Self::parse_response("Azure virtual network", virtual_network_name, &response)
    }

    async fn delete_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<OperationResult<()>> {
        self.delete_resource(
            self.virtual_network_url(resource_group_name, virtual_network_name),
            "Azure virtual network",
            virtual_network_name,
        )
        .await
    }

    async fn create_or_update_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
        subnet: &Subnet,
    ) -> Result<OperationResult<Subnet>> {
        self.put_resource(
            self.subnet_url(resource_group_name, virtual_network_name, subnet_name),
            subnet,
            "Azure subnet",
            subnet_name,
        )
        .await
    }

    async fn get_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<Subnet> {
        let (_, _, response) = self
            .request(
                Method::GET,
                self.subnet_url(resource_group_name, virtual_network_name, subnet_name),
                None,
                "Azure subnet",
                subnet_name,
            )
            .await?;
        Self::parse_response("Azure subnet", subnet_name, &response)
    }

    async fn delete_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<OperationResult<()>> {
        self.delete_resource(
            self.subnet_url(resource_group_name, virtual_network_name, subnet_name),
            "Azure subnet",
            subnet_name,
        )
        .await
    }

    async fn create_or_update_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
        nat_gateway: &NatGateway,
    ) -> Result<OperationResult<NatGateway>> {
        self.put_resource(
            self.nat_gateway_url(resource_group_name, nat_gateway_name),
            nat_gateway,
            "Azure NAT gateway",
            nat_gateway_name,
        )
        .await
    }

    async fn get_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<NatGateway> {
        let (_, _, response) = self
            .request(
                Method::GET,
                self.nat_gateway_url(resource_group_name, nat_gateway_name),
                None,
                "Azure NAT gateway",
                nat_gateway_name,
            )
            .await?;
        Self::parse_response("Azure NAT gateway", nat_gateway_name, &response)
    }

    async fn delete_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<OperationResult<()>> {
        self.delete_resource(
            self.nat_gateway_url(resource_group_name, nat_gateway_name),
            "Azure NAT gateway",
            nat_gateway_name,
        )
        .await
    }

    async fn create_or_update_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
        public_ip_address: &PublicIpAddress,
    ) -> Result<OperationResult<PublicIpAddress>> {
        self.put_resource(
            self.public_ip_url(resource_group_name, public_ip_address_name),
            public_ip_address,
            "Azure public IP address",
            public_ip_address_name,
        )
        .await
    }

    async fn get_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<PublicIpAddress> {
        let (_, _, response) = self
            .request(
                Method::GET,
                self.public_ip_url(resource_group_name, public_ip_address_name),
                None,
                "Azure public IP address",
                public_ip_address_name,
            )
            .await?;
        Self::parse_response("Azure public IP address", public_ip_address_name, &response)
    }

    async fn delete_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<OperationResult<()>> {
        self.delete_resource(
            self.public_ip_url(resource_group_name, public_ip_address_name),
            "Azure public IP address",
            public_ip_address_name,
        )
        .await
    }

    async fn create_or_update_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
        network_security_group: &NetworkSecurityGroup,
    ) -> Result<OperationResult<NetworkSecurityGroup>> {
        self.put_resource(
            self.network_security_group_url(resource_group_name, network_security_group_name),
            network_security_group,
            "Azure network security group",
            network_security_group_name,
        )
        .await
    }

    async fn get_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<NetworkSecurityGroup> {
        let (_, _, response) = self
            .request(
                Method::GET,
                self.network_security_group_url(resource_group_name, network_security_group_name),
                None,
                "Azure network security group",
                network_security_group_name,
            )
            .await?;
        Self::parse_response(
            "Azure network security group",
            network_security_group_name,
            &response,
        )
    }

    async fn delete_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<OperationResult<()>> {
        self.delete_resource(
            self.network_security_group_url(resource_group_name, network_security_group_name),
            "Azure network security group",
            network_security_group_name,
        )
        .await
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait AzureTableManagementApi: Send + Sync {
    async fn create_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<()>;

    async fn delete_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<()>;

    async fn get_table_signed_identifier_count(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<usize>;
}

struct OfficialAzureTableManagementClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl OfficialAzureTableManagementClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn table_url(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/tableServices/default/tables/{}?api-version=2024-01-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            storage_account_name,
            table_name
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<String> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok(text)
    }
}

#[async_trait::async_trait]
impl AzureTableManagementApi for OfficialAzureTableManagementClient {
    async fn create_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<()> {
        let table = Table {
            resource: AzureStorageResource {
                id: None,
                name: Some(table_name.to_string()),
                type_: None,
            },
            properties: Some(TableProperties {
                signed_identifiers: vec![],
                table_name: Some(table_name.to_string()),
            }),
        };
        let body = serde_json::to_string(&table).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to serialize Azure Table '{table_name}' request"),
                resource_id: None,
            },
        )?;

        self.request(
            Method::PUT,
            self.table_url(resource_group_name, storage_account_name, table_name),
            Some(body),
            "Azure Table",
            table_name,
        )
        .await?;
        Ok(())
    }

    async fn delete_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<()> {
        self.request(
            Method::DELETE,
            self.table_url(resource_group_name, storage_account_name, table_name),
            None,
            "Azure Table",
            table_name,
        )
        .await?;
        Ok(())
    }

    async fn get_table_signed_identifier_count(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<usize> {
        let body = self
            .request(
                Method::GET,
                self.table_url(resource_group_name, storage_account_name, table_name),
                None,
                "Azure Table",
                table_name,
            )
            .await?;
        let table = serde_json::from_str::<Table>(&body)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to parse Azure Table '{table_name}' response"),
                resource_id: None,
            })?;
        Ok(table
            .properties
            .map(|properties| properties.signed_identifiers.len())
            .unwrap_or_default())
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait AzureResourcesApi: Send + Sync {
    async fn create_or_update_resource_group(
        &self,
        resource_group_name: &str,
        resource_group: &ResourceGroup,
    ) -> Result<ResourceGroup>;

    async fn delete_resource_group(&self, resource_group_name: &str) -> Result<()>;

    async fn get_resource_group(&self, resource_group_name: &str) -> Result<ResourceGroup>;

    async fn get_provider(&self, resource_provider_namespace: &str) -> Result<Provider>;

    async fn register_provider(&self, resource_provider_namespace: &str) -> Result<Provider>;
}

struct OfficialAzureResourcesClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl OfficialAzureResourcesClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn resource_group_url(&self, resource_group_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourcegroups/{}?api-version=2021-04-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name
        )
    }

    fn provider_url(&self, resource_provider_namespace: &str, action: Option<&str>) -> String {
        let action = action
            .map(|action| format!("/{action}"))
            .unwrap_or_default();
        format!(
            "{}/subscriptions/{}/providers/{}{}?api-version=2021-04-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_provider_namespace,
            action
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<String> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok(text)
    }
}

#[async_trait::async_trait]
impl AzureResourcesApi for OfficialAzureResourcesClient {
    async fn create_or_update_resource_group(
        &self,
        resource_group_name: &str,
        resource_group: &ResourceGroup,
    ) -> Result<ResourceGroup> {
        let body = serde_json::to_string(resource_group)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure resource group '{resource_group_name}' request"
                ),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PUT,
                self.resource_group_url(resource_group_name),
                Some(body),
                "Azure Resource Group",
                resource_group_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure resource group '{resource_group_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn delete_resource_group(&self, resource_group_name: &str) -> Result<()> {
        self.request(
            Method::DELETE,
            self.resource_group_url(resource_group_name),
            None,
            "Azure Resource Group",
            resource_group_name,
        )
        .await?;
        Ok(())
    }

    async fn get_resource_group(&self, resource_group_name: &str) -> Result<ResourceGroup> {
        let response = self
            .request(
                Method::GET,
                self.resource_group_url(resource_group_name),
                None,
                "Azure Resource Group",
                resource_group_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure resource group '{resource_group_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn get_provider(&self, resource_provider_namespace: &str) -> Result<Provider> {
        let response = self
            .request(
                Method::GET,
                self.provider_url(resource_provider_namespace, None),
                None,
                "Azure Resource Provider",
                resource_provider_namespace,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure provider '{resource_provider_namespace}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn register_provider(&self, resource_provider_namespace: &str) -> Result<Provider> {
        let response = self
            .request(
                Method::POST,
                self.provider_url(resource_provider_namespace, Some("register")),
                Some("{}".to_string()),
                "Azure Resource Provider",
                resource_provider_namespace,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure provider registration '{resource_provider_namespace}' response"
                ),
                resource_id: None,
            },
        )
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait AzureKeyVaultManagementApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
        parameters: AzureKeyVaultCreateOrUpdateParameters,
    ) -> Result<AzureKeyVault>;

    async fn delete_vault(&self, resource_group_name: String, vault_name: String) -> Result<()>;

    async fn get_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
    ) -> Result<AzureKeyVault>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait AzureServiceBusManagementApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
        parameters: SbNamespace,
    ) -> Result<SbNamespace>;

    async fn get_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<SbNamespace>;

    async fn delete_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<()>;

    async fn create_or_update_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
        parameters: SbQueue,
    ) -> Result<SbQueue>;

    async fn get_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<SbQueue>;

    async fn delete_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<()>;
}

struct OfficialAzureServiceBusManagementClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialAzureServiceBusManagementClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureServiceBusManagementClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureServiceBusManagementClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn namespace_url(&self, resource_group_name: &str, namespace_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}?api-version=2024-01-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            namespace_name
        )
    }

    fn queue_url(
        &self,
        resource_group_name: &str,
        namespace_name: &str,
        queue_name: &str,
    ) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ServiceBus/namespaces/{}/queues/{}?api-version=2024-01-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            namespace_name,
            queue_name
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<String> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok(text)
    }
}

#[async_trait::async_trait]
impl AzureServiceBusManagementApi for OfficialAzureServiceBusManagementClient {
    async fn create_or_update_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
        parameters: SbNamespace,
    ) -> Result<SbNamespace> {
        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure Service Bus namespace '{namespace_name}' request"
                ),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PUT,
                self.namespace_url(&resource_group_name, &namespace_name),
                Some(body),
                "Azure Service Bus namespace",
                &namespace_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure Service Bus namespace '{namespace_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn get_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<SbNamespace> {
        let response = self
            .request(
                Method::GET,
                self.namespace_url(&resource_group_name, &namespace_name),
                None,
                "Azure Service Bus namespace",
                &namespace_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure Service Bus namespace '{namespace_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn delete_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<()> {
        self.request(
            Method::DELETE,
            self.namespace_url(&resource_group_name, &namespace_name),
            None,
            "Azure Service Bus namespace",
            &namespace_name,
        )
        .await?;
        Ok(())
    }

    async fn create_or_update_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
        parameters: SbQueue,
    ) -> Result<SbQueue> {
        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure Service Bus queue '{queue_name}' request"
                ),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PUT,
                self.queue_url(&resource_group_name, &namespace_name, &queue_name),
                Some(body),
                "Azure Service Bus queue",
                &queue_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to parse Azure Service Bus queue '{queue_name}' response"),
                resource_id: None,
            },
        )
    }

    async fn get_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<SbQueue> {
        let response = self
            .request(
                Method::GET,
                self.queue_url(&resource_group_name, &namespace_name, &queue_name),
                None,
                "Azure Service Bus queue",
                &queue_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to parse Azure Service Bus queue '{queue_name}' response"),
                resource_id: None,
            },
        )
    }

    async fn delete_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<()> {
        self.request(
            Method::DELETE,
            self.queue_url(&resource_group_name, &namespace_name, &queue_name),
            None,
            "Azure Service Bus queue",
            &queue_name,
        )
        .await?;
        Ok(())
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait StorageAccountsApi: Send + Sync + std::fmt::Debug {
    async fn create_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &StorageAccountCreateParameters,
    ) -> Result<()>;

    async fn delete_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<()>;

    async fn get_storage_account_properties(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<StorageAccount>;
}

struct OfficialAzureStorageAccountsClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialAzureStorageAccountsClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureStorageAccountsClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureStorageAccountsClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn storage_account_url(&self, resource_group_name: &str, account_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}?api-version=2023-01-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            account_name
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<String> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok(text)
    }
}

#[async_trait::async_trait]
impl StorageAccountsApi for OfficialAzureStorageAccountsClient {
    async fn create_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &StorageAccountCreateParameters,
    ) -> Result<()> {
        let body = serde_json::to_string(parameters)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure Storage account '{account_name}' request"
                ),
                resource_id: None,
            })?;
        self.request(
            Method::PUT,
            self.storage_account_url(resource_group_name, account_name),
            Some(body),
            "Azure Storage account",
            account_name,
        )
        .await?;
        Ok(())
    }

    async fn delete_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<()> {
        self.request(
            Method::DELETE,
            self.storage_account_url(resource_group_name, account_name),
            None,
            "Azure Storage account",
            account_name,
        )
        .await?;
        Ok(())
    }

    async fn get_storage_account_properties(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<StorageAccount> {
        let response = self
            .request(
                Method::GET,
                self.storage_account_url(resource_group_name, account_name),
                None,
                "Azure Storage account",
                account_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to parse Azure Storage account '{account_name}' response"),
                resource_id: None,
            },
        )
    }
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait BlobContainerApi: Send + Sync + std::fmt::Debug {
    async fn create_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &AzureBlobContainer,
    ) -> Result<AzureBlobContainer>;

    async fn get_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<AzureBlobContainer>;

    async fn get_blob_service_properties(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
    ) -> Result<AzureBlobServiceProperties>;

    async fn delete_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<()>;

    async fn update_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &AzureBlobContainer,
    ) -> Result<AzureBlobContainer>;
}

struct OfficialAzureBlobContainerClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialAzureBlobContainerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureBlobContainerClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureBlobContainerClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn blob_container_url(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/blobServices/default/containers/{}?api-version=2024-01-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            storage_account_name,
            container_name
        )
    }

    fn blob_service_url(&self, resource_group_name: &str, storage_account_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/blobServices/default?api-version=2024-01-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            storage_account_name
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> Result<String> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: resource_type.to_string(),
                    resource_name: resource_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok(text)
    }
}

#[async_trait::async_trait]
impl BlobContainerApi for OfficialAzureBlobContainerClient {
    async fn create_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &AzureBlobContainer,
    ) -> Result<AzureBlobContainer> {
        let body = serde_json::to_string(blob_container)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure Blob container '{container_name}' request"
                ),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PUT,
                self.blob_container_url(resource_group_name, storage_account_name, container_name),
                Some(body),
                "Azure Blob container",
                container_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure Blob container '{container_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn get_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<AzureBlobContainer> {
        let response = self
            .request(
                Method::GET,
                self.blob_container_url(resource_group_name, storage_account_name, container_name),
                None,
                "Azure Blob container",
                container_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure Blob container '{container_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn get_blob_service_properties(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
    ) -> Result<AzureBlobServiceProperties> {
        let response = self
            .request(
                Method::GET,
                self.blob_service_url(resource_group_name, storage_account_name),
                None,
                "Azure Blob service",
                storage_account_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure Blob service '{storage_account_name}' response"
                ),
                resource_id: None,
            },
        )
    }

    async fn delete_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<()> {
        self.request(
            Method::DELETE,
            self.blob_container_url(resource_group_name, storage_account_name, container_name),
            None,
            "Azure Blob container",
            container_name,
        )
        .await?;
        Ok(())
    }

    async fn update_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &AzureBlobContainer,
    ) -> Result<AzureBlobContainer> {
        let body = serde_json::to_string(blob_container)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure Blob container '{container_name}' request"
                ),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PATCH,
                self.blob_container_url(resource_group_name, storage_account_name, container_name),
                Some(body),
                "Azure Blob container",
                container_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure Blob container '{container_name}' response"
                ),
                resource_id: None,
            },
        )
    }
}

struct OfficialAzureKeyVaultManagementClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl std::fmt::Debug for OfficialAzureKeyVaultManagementClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureKeyVaultManagementClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureKeyVaultManagementClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn vault_url(&self, resource_group_name: &str, vault_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}?api-version=2022-07-01",
            azure_management_endpoint(&self.config).trim_end_matches('/'),
            self.config.subscription_id,
            resource_group_name,
            vault_name
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Failed to get Azure management access token".to_string(),
                resource_id: None,
            })
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        vault_name: &str,
    ) -> Result<String> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Azure Key Vault ARM request failed for vault '{vault_name}'"),
                resource_id: None,
            },
        )?;
        let status = response.status();
        let text = response.text().await.into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to read Azure Key Vault ARM response for vault '{vault_name}'"
                ),
                resource_id: None,
            },
        )?;

        if status == StatusCode::NOT_FOUND {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudResourceNotFound {
                    resource_type: "Azure Key Vault".to_string(),
                    resource_name: vault_name.to_string(),
                },
            ));
        }

        if !status.is_success() {
            return Err(AlienError::new(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Azure Key Vault ARM request for vault '{vault_name}' returned HTTP {}: {}",
                        status.as_u16(),
                        text
                    ),
                    resource_id: None,
                },
            ));
        }

        Ok(text)
    }
}

#[async_trait::async_trait]
impl AzureKeyVaultManagementApi for OfficialAzureKeyVaultManagementClient {
    async fn create_or_update_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
        parameters: AzureKeyVaultCreateOrUpdateParameters,
    ) -> Result<AzureKeyVault> {
        let body = serde_json::to_string(&parameters)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to serialize Azure Key Vault '{vault_name}' request"),
                resource_id: None,
            })?;
        let response = self
            .request(
                Method::PUT,
                self.vault_url(&resource_group_name, &vault_name),
                Some(body),
                &vault_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to parse Azure Key Vault '{vault_name}' response"),
                resource_id: None,
            },
        )
    }

    async fn delete_vault(&self, resource_group_name: String, vault_name: String) -> Result<()> {
        self.request(
            Method::DELETE,
            self.vault_url(&resource_group_name, &vault_name),
            None,
            &vault_name,
        )
        .await?;
        Ok(())
    }

    async fn get_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
    ) -> Result<AzureKeyVault> {
        let response = self
            .request(
                Method::GET,
                self.vault_url(&resource_group_name, &vault_name),
                None,
                &vault_name,
            )
            .await?;
        serde_json::from_str(&response).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!("Failed to parse Azure Key Vault '{vault_name}' response"),
                resource_id: None,
            },
        )
    }
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
    fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn CloudRunApi>>;
    fn get_gcp_resource_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ResourceManagerApi>>;
    fn get_gcp_service_usage_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn GcpServiceUsageApi>>;
    fn get_gcp_gcs_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcsApi>>;
    fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ArtifactRegistryApi>>;
    fn get_gcp_firestore_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn GcpFirestoreAdminApi>>;
    fn get_gcp_pubsub_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn PubSubApi>>;
    fn get_gcp_compute_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpComputeApi>>;
    fn get_gcp_cloud_scheduler_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn CloudSchedulerApi>>;
    // Azure clients
    fn get_azure_authorization_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AuthorizationApi>>;
    fn get_azure_blob_container_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn BlobContainerApi>>;
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
    fn get_azure_managed_identity_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn ManagedIdentityApi>>;
    fn get_azure_resources_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureResourcesApi>>;
    fn get_azure_storage_accounts_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn StorageAccountsApi>>;
    fn get_azure_key_vault_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureKeyVaultManagementApi>>;
    fn get_azure_table_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureTableManagementApi>>;
    fn get_azure_service_bus_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureServiceBusManagementApi>>;
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
        Ok(Arc::new(OfficialGcpIamClient::new(config.clone())))
    }

    fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn CloudRunApi>> {
        Ok(Arc::new(OfficialGcpCloudRunClient::new(config.clone())))
    }

    fn get_gcp_resource_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ResourceManagerApi>> {
        Ok(Arc::new(OfficialGcpResourceManagerClient::new(
            config.clone(),
        )))
    }

    fn get_gcp_service_usage_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn GcpServiceUsageApi>> {
        Ok(Arc::new(OfficialGcpServiceUsageClient::new(config.clone())))
    }

    fn get_gcp_gcs_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcsApi>> {
        Ok(Arc::new(OfficialGcpGcsClient::new(config.clone())?))
    }

    fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ArtifactRegistryApi>> {
        Ok(Arc::new(OfficialGcpArtifactRegistryClient::new(
            config.clone(),
        )))
    }

    fn get_gcp_firestore_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn GcpFirestoreAdminApi>> {
        Ok(Arc::new(OfficialGcpFirestoreAdminClient::new(
            config.clone(),
        )))
    }

    fn get_gcp_pubsub_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn PubSubApi>> {
        Ok(Arc::new(OfficialGcpPubSubClient::new(config.clone())?))
    }

    fn get_gcp_compute_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpComputeApi>> {
        Ok(Arc::new(OfficialGcpComputeClient::new(config.clone())))
    }

    fn get_gcp_cloud_scheduler_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn CloudSchedulerApi>> {
        Ok(Arc::new(OfficialGcpCloudSchedulerClient::new(
            config.clone(),
        )))
    }

    // Azure implementations
    fn get_azure_authorization_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AuthorizationApi>> {
        Ok(Arc::new(OfficialAzureAuthorizationClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_blob_container_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn BlobContainerApi>> {
        Ok(Arc::new(OfficialAzureBlobContainerClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
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
    ) -> Result<Arc<dyn ContainerRegistryApi>> {
        Ok(Arc::new(OfficialAzureContainerRegistryClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
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
    ) -> Result<Arc<dyn ManagedIdentityApi>> {
        Ok(Arc::new(OfficialAzureManagedIdentityClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_resources_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureResourcesApi>> {
        Ok(Arc::new(OfficialAzureResourcesClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_storage_accounts_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn StorageAccountsApi>> {
        Ok(Arc::new(OfficialAzureStorageAccountsClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_key_vault_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureKeyVaultManagementApi>> {
        Ok(Arc::new(OfficialAzureKeyVaultManagementClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_table_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureTableManagementApi>> {
        Ok(Arc::new(OfficialAzureTableManagementClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_service_bus_management_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureServiceBusManagementApi>> {
        Ok(Arc::new(OfficialAzureServiceBusManagementClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
    }

    fn get_azure_network_client(
        &self,
        config: &AzureClientConfig,
    ) -> Result<Arc<dyn AzureNetworkApi>> {
        Ok(Arc::new(OfficialAzureNetworkClient::new(
            config.clone(),
            azure_credential_from_config(config)?,
        )))
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

async fn iam_admin_client_from_alien_config(config: &GcpClientConfig) -> Result<OfficialGcpIam> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = OfficialGcpIam::builder().with_credentials(credentials);

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

async fn pubsub_topic_admin_from_alien_config(
    config: &GcpClientConfig,
) -> Result<OfficialTopicAdmin> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = OfficialTopicAdmin::builder().with_credentials(credentials);

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
) -> Result<OfficialSubscriptionAdmin> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = OfficialSubscriptionAdmin::builder().with_credentials(credentials);

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

async fn gcs_storage_control_from_alien_config(
    config: &GcpClientConfig,
) -> Result<OfficialStorageControl> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = OfficialStorageControl::builder().with_credentials(credentials);

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
) -> Result<OfficialArtifactRegistry> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = OfficialArtifactRegistry::builder().with_credentials(credentials);

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
) -> Result<OfficialCloudScheduler> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    let mut builder = OfficialCloudScheduler::builder().with_credentials(credentials);

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

fn artifact_registry_repository_resource_name(
    project_id: &str,
    location: &str,
    repository_id: &str,
) -> String {
    format!("projects/{project_id}/locations/{location}/repositories/{repository_id}")
}

fn cloud_scheduler_job_resource_name(project_id: &str, location: &str, job_id: &str) -> String {
    format!("projects/{project_id}/locations/{location}/jobs/{job_id}")
}

fn service_account_resource_name(project_id: &str, service_account_name: &str) -> String {
    if service_account_name.starts_with("projects/") {
        service_account_name.to_string()
    } else {
        format!("projects/{project_id}/serviceAccounts/{service_account_name}")
    }
}

fn pubsub_admin_endpoint(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}

fn pubsub_rest_endpoint(config: &GcpClientConfig) -> String {
    config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("pubsub"))
        .map(|endpoint| {
            let endpoint = endpoint.trim_end_matches('/');
            if endpoint.ends_with("/v1") {
                endpoint.to_string()
            } else {
                format!("{endpoint}/v1")
            }
        })
        .unwrap_or_else(|| "https://pubsub.googleapis.com/v1".to_string())
}

fn gcs_control_endpoint(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .trim_end_matches("/storage/v1")
        .trim_end_matches("/upload/storage/v1")
        .to_string()
}

fn gcs_rest_endpoint(config: &GcpClientConfig) -> String {
    config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("storage"))
        .map(|endpoint| endpoint.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "https://storage.googleapis.com/storage/v1".to_string())
}

fn bucket_update_mask(bucket: &Bucket) -> wkt::FieldMask {
    let mut paths = Vec::new();
    if bucket.versioning.is_some() {
        paths.push("versioning".to_string());
    }
    if bucket.lifecycle.is_some() {
        paths.push("lifecycle".to_string());
    }
    if bucket.iam_config.is_some() {
        paths.push("iam_config".to_string());
    }
    if !bucket.labels.is_empty() {
        paths.push("labels".to_string());
    }
    wkt::FieldMask::default().set_paths(paths)
}

async fn gcs_http_error(
    operation: &str,
    resource_id: Option<String>,
    response: reqwest::Response,
) -> AlienError<crate::error::ErrorData> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status == StatusCode::NOT_FOUND {
        AlienError::new(crate::error::ErrorData::CloudResourceNotFound {
            resource_type: format!("GCS {operation}"),
            resource_name: resource_id.unwrap_or_else(|| "unknown".to_string()),
        })
    } else if status == StatusCode::CONFLICT {
        AlienError::new(crate::error::ErrorData::CloudResourceConflict {
            resource_type: format!("GCS {operation}"),
            resource_name: resource_id.unwrap_or_else(|| "unknown".to_string()),
            message: format!("HTTP {}: {text}", status.as_u16()),
        })
    } else {
        AlienError::new(crate::error::ErrorData::CloudPlatformError {
            message: format!("GCS {operation} returned HTTP {}: {text}", status.as_u16()),
            resource_id,
        })
    }
}

fn field_mask_from_comma_separated(update_mask: String) -> wkt::FieldMask {
    wkt::FieldMask::default().set_paths(
        update_mask
            .split(',')
            .map(str::trim)
            .filter(|path| !path.is_empty()),
    )
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::NOT_FOUND.as_u16())
}

fn gax_error_is_conflict(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::AlreadyExists)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::CONFLICT.as_u16())
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
