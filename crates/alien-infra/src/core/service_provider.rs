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
use azure_mgmt_authorization::package_2022_04_01 as azure_authorization_2022_04;
pub use azure_mgmt_authorization::package_2022_04_01::models::{
    role_assignment_properties::{self, PrincipalType as RoleAssignmentPropertiesPrincipalType},
    Permission, RoleAssignment, RoleAssignmentCreateParameters, RoleAssignmentProperties,
    RoleDefinition, RoleDefinitionProperties,
};
use azure_mgmt_containerregistry::package_2023_11_preview as azure_containerregistry_2023_11;
pub use azure_mgmt_containerregistry::package_2023_11_preview::models::Registry;
use azure_mgmt_keyvault::package_preview_2022_02 as azure_keyvault_2022_02;
use azure_mgmt_keyvault::package_preview_2022_02::models::{Vault, VaultCreateOrUpdateParameters};
use azure_mgmt_msi::package_2023_01_31 as azure_msi_2023_01_31;
pub use azure_mgmt_msi::package_2023_01_31::models::{FederatedIdentityCredential, Identity};
use azure_mgmt_network::package_2024_03 as azure_network_2024_03;
pub use azure_mgmt_network::package_2024_03::models::{
    AddressSpace, NatGateway, NetworkSecurityGroup, PublicIpAddress, Subnet, VirtualNetwork,
};
use azure_mgmt_resources::package_resources_2021_04 as azure_resources_2021_04;
pub use azure_mgmt_resources::package_resources_2021_04::models::{Provider, ResourceGroup};
use azure_mgmt_servicebus::package_2024_01;
pub use azure_mgmt_servicebus::package_2024_01::models::{SbNamespace, SbQueue, SbQueueProperties};
use azure_mgmt_storage::package_2023_05 as azure_storage_2023_05;
use azure_mgmt_storage::package_2023_05::models::{BlobContainer, BlobServiceProperties};
pub use azure_mgmt_storage::package_2023_05::models::{
    Endpoints, StorageAccount, StorageAccountCreateParameters, StorageAccountProperties,
    StorageAccountPropertiesCreateParameters,
};
use futures_util::StreamExt;
use google_cloud_api_serviceusage_v1::client::ServiceUsage;
use google_cloud_artifactregistry_v1::client::ArtifactRegistry;
pub use google_cloud_artifactregistry_v1::model::{
    repository::Format as ArtifactRegistryRepositoryFormat,
    Repository as ArtifactRegistryRepository,
};
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use google_cloud_firestore_admin_v1::client::FirestoreAdmin;
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_iam_admin_v1::client::Iam;
pub use google_cloud_iam_admin_v1::model::{
    role::RoleLaunchStage, CreateRoleRequest, CreateServiceAccountRequest, ListRolesResponse, Role,
    ServiceAccount,
};
use google_cloud_iam_v1::client::IAMPolicy;
pub use google_cloud_iam_v1::model::{Binding, GetPolicyOptions, Policy};
use google_cloud_longrunning::model::Operation;
use google_cloud_pubsub::client::{SubscriptionAdmin, TopicAdmin};
use google_cloud_resourcemanager_v3::client::Projects;
pub use google_cloud_resourcemanager_v3::model::Project;
use google_cloud_scheduler_v1::client::CloudScheduler;
use google_cloud_storage::{
    client::StorageControl,
    model::{Bucket, DeleteObjectRequest},
};
pub use google_cloud_type::model::Expr;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::{future::Future, path::PathBuf, sync::Arc, time::Duration};
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

    async fn get_service_account_iam_policy(&self, service_account_name: String) -> Result<Policy>;

    async fn set_service_account_iam_policy(
        &self,
        service_account_name: String,
        iam_policy: Policy,
    ) -> Result<Policy>;

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

struct IamClient {
    config: GcpClientConfig,
    client: OnceCell<Iam>,
}

impl std::fmt::Debug for IamClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IamClient")
            .field("project_id", &self.config.project_id)
            .finish_non_exhaustive()
    }
}

impl IamClient {
    fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<Iam> {
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
impl GcpIamApi for IamClient {
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

    async fn get_service_account_iam_policy(&self, service_account_name: String) -> Result<Policy> {
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
        iam_policy: Policy,
    ) -> Result<Policy> {
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
pub trait GcsApi: Send + Sync + std::fmt::Debug {
    async fn create_bucket(&self, bucket_name: String, bucket: Bucket) -> Result<Bucket>;
    async fn get_bucket(&self, bucket_name: String) -> Result<Bucket>;
    async fn update_bucket(&self, bucket_name: String, bucket_patch: Bucket) -> Result<Bucket>;
    async fn delete_bucket(&self, bucket_name: String) -> Result<()>;
    async fn get_bucket_iam_policy(&self, bucket_name: String) -> Result<Policy>;
    async fn set_bucket_iam_policy(
        &self,
        bucket_name: String,
        iam_policy: Policy,
    ) -> Result<Policy>;
    async fn empty_bucket(&self, bucket_name: String) -> Result<()>;
    async fn insert_notification(
        &self,
        bucket_name: String,
        notification: serde_json::Value,
    ) -> Result<serde_json::Value>;
    async fn list_notifications(&self, bucket_name: String) -> Result<Vec<serde_json::Value>>;
    async fn delete_notification(&self, bucket_name: String, notification_id: String)
        -> Result<()>;
}

struct OfficialGcpGcsClient {
    config: GcpClientConfig,
    storage_control: OnceCell<StorageControl>,
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

    async fn storage_control(&self) -> Result<StorageControl> {
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

    async fn get_bucket_iam_policy(&self, bucket_name: String) -> Result<Policy> {
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
        iam_policy: Policy,
    ) -> Result<Policy> {
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
        notification: serde_json::Value,
    ) -> Result<serde_json::Value> {
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

    async fn list_notifications(&self, bucket_name: String) -> Result<Vec<serde_json::Value>> {
        let url = format!(
            "{}/b/{}/notificationConfigs",
            gcs_rest_endpoint(&self.config),
            bucket_name
        );
        let response: serde_json::Value = self
            .send_gcs_json(
                self.http_client.get(url),
                "list_notifications",
                Some(bucket_name),
                Option::<&()>::None,
            )
            .await?;

        Ok(response
            .get("items")
            .and_then(|items| items.as_array())
            .cloned()
            .unwrap_or_default())
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
    ) -> Result<Policy>;

    async fn set_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        iam_policy: Policy,
    ) -> Result<Policy>;

    async fn get_operation(
        &self,
        project_id: String,
        location: String,
        operation_name: String,
    ) -> Result<Operation>;
}

struct OfficialGcpArtifactRegistryClient {
    config: GcpClientConfig,
    client: OnceCell<ArtifactRegistry>,
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

    async fn client(&self) -> Result<ArtifactRegistry> {
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
    ) -> Result<Policy> {
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
        iam_policy: Policy,
    ) -> Result<Policy> {
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
    ) -> Result<Policy>;

    async fn set_project_iam_policy(
        &self,
        project_id: String,
        policy: Policy,
        update_mask: Option<String>,
    ) -> Result<Policy>;

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
    ) -> Result<Policy> {
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
        policy: Policy,
        update_mask: Option<String>,
    ) -> Result<Policy> {
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
        role_assignment: &RoleAssignmentCreateParameters,
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
    client: OnceCell<azure_authorization_2022_04::Client>,
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
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_authorization_2022_04::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_authorization_2022_04::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Authorization client".to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
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
        let scope_string = scope.to_scope_string(&self.config);
        let result = self
            .client()
            .await?
            .role_definitions_client()
            .create_or_update(
                scope_string,
                role_definition_id.clone(),
                role_definition.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Authorization",
            result,
            "role definition create or update",
            "Azure role definition",
            &role_definition_id,
        )
    }

    async fn delete_role_definition(
        &self,
        scope: &Scope,
        role_definition_id: String,
    ) -> Result<Option<RoleDefinition>> {
        let scope_string = scope.to_scope_string(&self.config);
        let response = self
            .client()
            .await?
            .role_definitions_client()
            .delete(scope_string, role_definition_id.clone())
            .send()
            .await;
        let response = map_azure_core_021_sdk_error(
            "Azure Authorization",
            response,
            "role definition delete",
            "Azure role definition",
            &role_definition_id,
        )?;
        if response.as_raw_response().status() == azure_core_021::StatusCode::NoContent {
            Ok(None)
        } else {
            response
                .into_body()
                .await
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
        let scope_string = scope.to_scope_string(&self.config);
        let result = self
            .client()
            .await?
            .role_definitions_client()
            .get(scope_string, role_definition_id.clone())
            .await;
        map_azure_core_021_sdk_error(
            "Azure Authorization",
            result,
            "role definition get",
            "Azure role definition",
            &role_definition_id,
        )
    }

    async fn create_or_update_role_assignment_by_id(
        &self,
        role_assignment_id: String,
        role_assignment: &RoleAssignmentCreateParameters,
    ) -> Result<RoleAssignment> {
        let result = self
            .client()
            .await?
            .role_assignments_client()
            .create_by_id(role_assignment_id.clone(), role_assignment.clone())
            .await;
        map_azure_core_021_sdk_error(
            "Azure Authorization",
            result,
            "role assignment create or update",
            "Azure role assignment",
            &role_assignment_id,
        )
    }

    async fn delete_role_assignment_by_id(
        &self,
        role_assignment_id: String,
    ) -> Result<Option<RoleAssignment>> {
        let response = self
            .client()
            .await?
            .role_assignments_client()
            .delete_by_id(role_assignment_id.clone())
            .send()
            .await;
        let response = map_azure_core_021_sdk_error(
            "Azure Authorization",
            response,
            "role assignment delete",
            "Azure role assignment",
            &role_assignment_id,
        )?;
        if response.as_raw_response().status() == azure_core_021::StatusCode::NoContent {
            Ok(None)
        } else {
            response
                .into_body()
                .await
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
        let result = self
            .client()
            .await?
            .role_assignments_client()
            .get_by_id(role_assignment_id.clone())
            .await;
        map_azure_core_021_sdk_error(
            "Azure Authorization",
            result,
            "role assignment get",
            "Azure role assignment",
            &role_assignment_id,
        )
    }

    async fn list_role_assignments(
        &self,
        scope: &Scope,
        role_definition_id: Option<String>,
    ) -> Result<Vec<RoleAssignment>> {
        let scope_string = scope.to_scope_string(&self.config);
        let mut stream = self
            .client()
            .await?
            .role_assignments_client()
            .list_for_scope(scope_string.clone())
            .filter("atScope()")
            .into_stream();

        let mut assignments = Vec::new();
        while let Some(page) = stream.next().await {
            let page = map_azure_core_021_sdk_error(
                "Azure Authorization",
                page,
                "role assignments list",
                "Azure role assignments",
                &scope_string,
            )?;
            assignments.extend(page.value);
        }

        if let Some(role_definition_id) = role_definition_id {
            assignments.retain(|assignment| {
                assignment
                    .properties
                    .as_ref()
                    .is_some_and(|properties| properties.role_definition_id == role_definition_id)
            });
        }
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
    client: OnceCell<azure_msi_2023_01_31::Client>,
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
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_msi_2023_01_31::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_msi_2023_01_31::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Managed Identity client"
                            .to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
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
        let result = self
            .client()
            .await?
            .user_assigned_identities_client()
            .create_or_update(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                resource_name.to_string(),
                identity.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Managed Identity",
            result,
            "user assigned identity create or update",
            "Azure managed identity",
            resource_name,
        )
    }

    async fn delete_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<()> {
        let result = self
            .client()
            .await?
            .user_assigned_identities_client()
            .delete(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                resource_name.to_string(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Managed Identity",
            result,
            "user assigned identity delete",
            "Azure managed identity",
            resource_name,
        )
    }

    async fn get_user_assigned_identity(
        &self,
        resource_group_name: &str,
        resource_name: &str,
    ) -> Result<Identity> {
        let result = self
            .client()
            .await?
            .user_assigned_identities_client()
            .get(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                resource_name.to_string(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Managed Identity",
            result,
            "user assigned identity get",
            "Azure managed identity",
            resource_name,
        )
    }

    async fn create_or_update_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
        credential: &FederatedIdentityCredential,
    ) -> Result<FederatedIdentityCredential> {
        let result = self
            .client()
            .await?
            .federated_identity_credentials_client()
            .create_or_update(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                identity_name.to_string(),
                credential_name.to_string(),
                credential.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Managed Identity",
            result,
            "federated credential create or update",
            "Azure federated identity credential",
            credential_name,
        )
    }

    async fn get_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
    ) -> Result<FederatedIdentityCredential> {
        let result = self
            .client()
            .await?
            .federated_identity_credentials_client()
            .get(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                identity_name.to_string(),
                credential_name.to_string(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Managed Identity",
            result,
            "federated credential get",
            "Azure federated identity credential",
            credential_name,
        )
    }

    async fn delete_federated_credential(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        credential_name: &str,
    ) -> Result<()> {
        let result = self
            .client()
            .await?
            .federated_identity_credentials_client()
            .delete(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                identity_name.to_string(),
                credential_name.to_string(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Managed Identity",
            result,
            "federated credential delete",
            "Azure federated identity credential",
            credential_name,
        )
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
    client: OnceCell<azure_containerregistry_2023_11::Client>,
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
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_containerregistry_2023_11::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_containerregistry_2023_11::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Container Registry client"
                            .to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
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
        let response = self
            .client()
            .await?
            .registries_client()
            .create(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                registry_name.to_string(),
                parameters.clone(),
            )
            .send()
            .await;
        let response = map_azure_core_021_sdk_error(
            "Azure Container Registry",
            response,
            "registry create",
            "Azure Container Registry",
            registry_name,
        )?;

        if response.as_raw_response().status() == azure_core_021::StatusCode::Accepted {
            let operation =
                LongRunningOperation::from_azure_core_021_headers(response.as_raw_response().headers())?
                    .ok_or_else(|| {
                        AlienError::new(crate::error::ErrorData::CloudPlatformError {
                            message: format!(
                                "Azure Container Registry '{registry_name}' returned 202 without an operation URL"
                            ),
                            resource_id: None,
                        })
                    })?;
            Ok(OperationResult::LongRunning(operation))
        } else {
            let registry = response.into_body().await.into_alien_error().context(
                crate::error::ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to parse Azure Container Registry '{registry_name}' response"
                    ),
                    resource_id: None,
                },
            )?;
            Ok(OperationResult::Completed(registry))
        }
    }

    async fn delete_registry(&self, resource_group_name: &str, registry_name: &str) -> Result<()> {
        let result = self
            .client()
            .await?
            .registries_client()
            .delete(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                registry_name.to_string(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Container Registry",
            result,
            "registry delete",
            "Azure Container Registry",
            registry_name,
        )
    }

    async fn get_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Registry> {
        let result = self
            .client()
            .await?
            .registries_client()
            .get(
                self.config.subscription_id.clone(),
                resource_group_name.to_string(),
                registry_name.to_string(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Container Registry",
            result,
            "registry get",
            "Azure Container Registry",
            registry_name,
        )
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
    client: OnceCell<azure_network_2024_03::Client>,
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
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_network_2024_03::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_network_2024_03::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Network client".to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
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
        let result = self
            .client()
            .await?
            .virtual_networks_client()
            .create_or_update(
                resource_group_name.to_string(),
                virtual_network_name.to_string(),
                virtual_network.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_lro_response(
            "Azure Network",
            result,
            "virtual network create or update",
            "Azure virtual network",
            virtual_network_name,
            |response| response.into_body(),
        )
        .await
    }

    async fn get_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<VirtualNetwork> {
        let result = self
            .client()
            .await?
            .virtual_networks_client()
            .get(
                resource_group_name.to_string(),
                virtual_network_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Network",
            result,
            "virtual network get",
            "Azure virtual network",
            virtual_network_name,
        )
    }

    async fn delete_virtual_network(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
    ) -> Result<OperationResult<()>> {
        let result = self
            .client()
            .await?
            .virtual_networks_client()
            .delete(
                resource_group_name.to_string(),
                virtual_network_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_delete_lro_response(
            "Azure Network",
            result,
            "virtual network delete",
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
        let result = self
            .client()
            .await?
            .subnets_client()
            .create_or_update(
                resource_group_name.to_string(),
                virtual_network_name.to_string(),
                subnet_name.to_string(),
                subnet.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_lro_response(
            "Azure Network",
            result,
            "subnet create or update",
            "Azure subnet",
            subnet_name,
            |response| response.into_body(),
        )
        .await
    }

    async fn get_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<Subnet> {
        let result = self
            .client()
            .await?
            .subnets_client()
            .get(
                resource_group_name.to_string(),
                virtual_network_name.to_string(),
                subnet_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Network",
            result,
            "subnet get",
            "Azure subnet",
            subnet_name,
        )
    }

    async fn delete_subnet(
        &self,
        resource_group_name: &str,
        virtual_network_name: &str,
        subnet_name: &str,
    ) -> Result<OperationResult<()>> {
        let result = self
            .client()
            .await?
            .subnets_client()
            .delete(
                resource_group_name.to_string(),
                virtual_network_name.to_string(),
                subnet_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_delete_lro_response(
            "Azure Network",
            result,
            "subnet delete",
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
        let result = self
            .client()
            .await?
            .nat_gateways_client()
            .create_or_update(
                resource_group_name.to_string(),
                nat_gateway_name.to_string(),
                nat_gateway.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_lro_response(
            "Azure Network",
            result,
            "NAT gateway create or update",
            "Azure NAT gateway",
            nat_gateway_name,
            |response| response.into_body(),
        )
        .await
    }

    async fn get_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<NatGateway> {
        let result = self
            .client()
            .await?
            .nat_gateways_client()
            .get(
                resource_group_name.to_string(),
                nat_gateway_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Network",
            result,
            "NAT gateway get",
            "Azure NAT gateway",
            nat_gateway_name,
        )
    }

    async fn delete_nat_gateway(
        &self,
        resource_group_name: &str,
        nat_gateway_name: &str,
    ) -> Result<OperationResult<()>> {
        let result = self
            .client()
            .await?
            .nat_gateways_client()
            .delete(
                resource_group_name.to_string(),
                nat_gateway_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_delete_lro_response(
            "Azure Network",
            result,
            "NAT gateway delete",
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
        let result = self
            .client()
            .await?
            .public_ip_addresses_client()
            .create_or_update(
                resource_group_name.to_string(),
                public_ip_address_name.to_string(),
                public_ip_address.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_lro_response(
            "Azure Network",
            result,
            "public IP address create or update",
            "Azure public IP address",
            public_ip_address_name,
            |response| response.into_body(),
        )
        .await
    }

    async fn get_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<PublicIpAddress> {
        let result = self
            .client()
            .await?
            .public_ip_addresses_client()
            .get(
                resource_group_name.to_string(),
                public_ip_address_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Network",
            result,
            "public IP address get",
            "Azure public IP address",
            public_ip_address_name,
        )
    }

    async fn delete_public_ip_address(
        &self,
        resource_group_name: &str,
        public_ip_address_name: &str,
    ) -> Result<OperationResult<()>> {
        let result = self
            .client()
            .await?
            .public_ip_addresses_client()
            .delete(
                resource_group_name.to_string(),
                public_ip_address_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_delete_lro_response(
            "Azure Network",
            result,
            "public IP address delete",
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
        let result = self
            .client()
            .await?
            .network_security_groups_client()
            .create_or_update(
                resource_group_name.to_string(),
                network_security_group_name.to_string(),
                network_security_group.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_lro_response(
            "Azure Network",
            result,
            "network security group create or update",
            "Azure network security group",
            network_security_group_name,
            |response| response.into_body(),
        )
        .await
    }

    async fn get_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<NetworkSecurityGroup> {
        let result = self
            .client()
            .await?
            .network_security_groups_client()
            .get(
                resource_group_name.to_string(),
                network_security_group_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Network",
            result,
            "network security group get",
            "Azure network security group",
            network_security_group_name,
        )
    }

    async fn delete_network_security_group(
        &self,
        resource_group_name: &str,
        network_security_group_name: &str,
    ) -> Result<OperationResult<()>> {
        let result = self
            .client()
            .await?
            .network_security_groups_client()
            .delete(
                resource_group_name.to_string(),
                network_security_group_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await;
        map_azure_core_021_delete_lro_response(
            "Azure Network",
            result,
            "network security group delete",
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
    client: OnceCell<azure_storage_2023_05::Client>,
}

impl OfficialAzureTableManagementClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_storage_2023_05::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_storage_2023_05::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Storage client".to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
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
        let result = self
            .client()
            .await?
            .table_client()
            .create(
                resource_group_name.to_string(),
                storage_account_name.to_string(),
                self.config.subscription_id.clone(),
                table_name.to_string(),
            )
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "table create",
            "Azure Table",
            table_name,
        )
    }

    async fn delete_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<()> {
        let result = self
            .client()
            .await?
            .table_client()
            .delete(
                resource_group_name.to_string(),
                storage_account_name.to_string(),
                self.config.subscription_id.clone(),
                table_name.to_string(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "table delete",
            "Azure Table",
            table_name,
        )
    }

    async fn get_table_signed_identifier_count(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<usize> {
        let result = self
            .client()
            .await?
            .table_client()
            .get(
                resource_group_name.to_string(),
                storage_account_name.to_string(),
                self.config.subscription_id.clone(),
                table_name.to_string(),
            )
            .await;
        let table = map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "table get",
            "Azure Table",
            table_name,
        )?;
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
    client: OnceCell<azure_resources_2021_04::Client>,
}

impl OfficialAzureResourcesClient {
    fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_resources_2021_04::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_resources_2021_04::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Resources client".to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
    }
}

#[async_trait::async_trait]
impl AzureResourcesApi for OfficialAzureResourcesClient {
    async fn create_or_update_resource_group(
        &self,
        resource_group_name: &str,
        resource_group: &ResourceGroup,
    ) -> Result<ResourceGroup> {
        let result = self
            .client()
            .await?
            .resource_groups_client()
            .create_or_update(
                resource_group_name.to_string(),
                resource_group.clone(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Resources",
            result,
            "resource group create or update",
            "Azure Resource Group",
            resource_group_name,
        )
    }

    async fn delete_resource_group(&self, resource_group_name: &str) -> Result<()> {
        let result = self
            .client()
            .await?
            .resource_groups_client()
            .delete(
                resource_group_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Resources",
            result,
            "resource group delete",
            "Azure Resource Group",
            resource_group_name,
        )
    }

    async fn get_resource_group(&self, resource_group_name: &str) -> Result<ResourceGroup> {
        let result = self
            .client()
            .await?
            .resource_groups_client()
            .get(
                resource_group_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Resources",
            result,
            "resource group get",
            "Azure Resource Group",
            resource_group_name,
        )
    }

    async fn get_provider(&self, resource_provider_namespace: &str) -> Result<Provider> {
        let result = self
            .client()
            .await?
            .providers_client()
            .get(
                resource_provider_namespace.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Resources",
            result,
            "provider get",
            "Azure Resource Provider",
            resource_provider_namespace,
        )
    }

    async fn register_provider(&self, resource_provider_namespace: &str) -> Result<Provider> {
        let result = self
            .client()
            .await?
            .providers_client()
            .register(
                resource_provider_namespace.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Resources",
            result,
            "provider register",
            "Azure Resource Provider",
            resource_provider_namespace,
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
        parameters: VaultCreateOrUpdateParameters,
    ) -> Result<Vault>;

    async fn delete_vault(&self, resource_group_name: String, vault_name: String) -> Result<()>;

    async fn get_vault(&self, resource_group_name: String, vault_name: String) -> Result<Vault>;
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
    client: OnceCell<package_2024_01::Client>,
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
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<package_2024_01::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                package_2024_01::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Service Bus client".to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
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
        let result = self
            .client()
            .await?
            .namespaces_client()
            .create_or_update(
                resource_group_name,
                namespace_name.clone(),
                parameters,
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Service Bus",
            result,
            "namespace create or update",
            "Azure Service Bus namespace",
            &namespace_name,
        )
    }

    async fn get_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<SbNamespace> {
        let result = self
            .client()
            .await?
            .namespaces_client()
            .get(
                resource_group_name,
                namespace_name.clone(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Service Bus",
            result,
            "namespace get",
            "Azure Service Bus namespace",
            &namespace_name,
        )
    }

    async fn delete_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<()> {
        let result = self
            .client()
            .await?
            .namespaces_client()
            .delete(
                resource_group_name,
                namespace_name.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Service Bus",
            result,
            "namespace delete",
            "Azure Service Bus namespace",
            &namespace_name,
        )
    }

    async fn create_or_update_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
        parameters: SbQueue,
    ) -> Result<SbQueue> {
        let result = self
            .client()
            .await?
            .queues_client()
            .create_or_update(
                resource_group_name,
                namespace_name,
                queue_name.clone(),
                parameters,
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Service Bus",
            result,
            "queue create or update",
            "Azure Service Bus queue",
            &queue_name,
        )
    }

    async fn get_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<SbQueue> {
        let result = self
            .client()
            .await?
            .queues_client()
            .get(
                resource_group_name,
                namespace_name,
                queue_name.clone(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Service Bus",
            result,
            "queue get",
            "Azure Service Bus queue",
            &queue_name,
        )
    }

    async fn delete_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<()> {
        let result = self
            .client()
            .await?
            .queues_client()
            .delete(
                resource_group_name,
                namespace_name,
                queue_name.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Service Bus",
            result,
            "queue delete",
            "Azure Service Bus queue",
            &queue_name,
        )
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
    client: OnceCell<azure_storage_2023_05::Client>,
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
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_storage_2023_05::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_storage_2023_05::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Storage client".to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
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
        let result = self
            .client()
            .await?
            .storage_accounts_client()
            .create(
                resource_group_name.to_string(),
                account_name.to_string(),
                parameters.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "account create",
            "Azure Storage account",
            account_name,
        )
    }

    async fn delete_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<()> {
        let result = self
            .client()
            .await?
            .storage_accounts_client()
            .delete(
                resource_group_name.to_string(),
                account_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "account delete",
            "Azure Storage account",
            account_name,
        )
    }

    async fn get_storage_account_properties(
        &self,
        resource_group_name: &str,
        account_name: &str,
    ) -> Result<StorageAccount> {
        let result = self
            .client()
            .await?
            .storage_accounts_client()
            .get_properties(
                resource_group_name.to_string(),
                account_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "account get properties",
            "Azure Storage account",
            account_name,
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
        blob_container: &BlobContainer,
    ) -> Result<BlobContainer>;

    async fn get_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<BlobContainer>;

    async fn get_blob_service_properties(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
    ) -> Result<BlobServiceProperties>;

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
        blob_container: &BlobContainer,
    ) -> Result<BlobContainer>;
}

struct OfficialAzureBlobContainerClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    client: OnceCell<azure_storage_2023_05::Client>,
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
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_storage_2023_05::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_storage_2023_05::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Storage client".to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
    }
}

#[async_trait::async_trait]
impl BlobContainerApi for OfficialAzureBlobContainerClient {
    async fn create_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &BlobContainer,
    ) -> Result<BlobContainer> {
        let result = self
            .client()
            .await?
            .blob_containers_client()
            .create(
                resource_group_name.to_string(),
                storage_account_name.to_string(),
                container_name.to_string(),
                blob_container.clone(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "blob container create",
            "Azure Blob container",
            container_name,
        )
    }

    async fn get_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<BlobContainer> {
        let result = self
            .client()
            .await?
            .blob_containers_client()
            .get(
                resource_group_name.to_string(),
                storage_account_name.to_string(),
                container_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "blob container get",
            "Azure Blob container",
            container_name,
        )
    }

    async fn get_blob_service_properties(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
    ) -> Result<BlobServiceProperties> {
        let result = self
            .client()
            .await?
            .blob_services_client()
            .get_service_properties(
                resource_group_name.to_string(),
                storage_account_name.to_string(),
                self.config.subscription_id.clone(),
                "default".to_string(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "blob service get properties",
            "Azure Blob service",
            storage_account_name,
        )
    }

    async fn delete_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
    ) -> Result<()> {
        let result = self
            .client()
            .await?
            .blob_containers_client()
            .delete(
                resource_group_name.to_string(),
                storage_account_name.to_string(),
                container_name.to_string(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "blob container delete",
            "Azure Blob container",
            container_name,
        )
    }

    async fn update_blob_container(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        container_name: &str,
        blob_container: &BlobContainer,
    ) -> Result<BlobContainer> {
        let result = self
            .client()
            .await?
            .blob_containers_client()
            .update(
                resource_group_name.to_string(),
                storage_account_name.to_string(),
                container_name.to_string(),
                blob_container.clone(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Storage",
            result,
            "blob container update",
            "Azure Blob container",
            container_name,
        )
    }
}

struct OfficialAzureKeyVaultManagementClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    client: OnceCell<azure_keyvault_2022_02::Client>,
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
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<azure_keyvault_2022_02::Client> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let endpoint = azure_core_021::Url::parse(azure_management_endpoint(&self.config))
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to parse Azure management endpoint".to_string(),
                        resource_id: None,
                    })?;

                let credential: Arc<dyn azure_core_021::auth::TokenCredential> =
                    Arc::new(AzureCore021Credential::new(self.credential.clone()));

                azure_keyvault_2022_02::Client::builder(credential)
                    .endpoint(endpoint)
                    .build()
                    .into_alien_error()
                    .context(crate::error::ErrorData::CloudPlatformError {
                        message: "Failed to build official Azure Key Vault client".to_string(),
                        resource_id: None,
                    })
            })
            .await?;
        Ok(client.clone())
    }
}

#[async_trait::async_trait]
impl AzureKeyVaultManagementApi for OfficialAzureKeyVaultManagementClient {
    async fn create_or_update_vault(
        &self,
        resource_group_name: String,
        vault_name: String,
        parameters: VaultCreateOrUpdateParameters,
    ) -> Result<Vault> {
        let result = self
            .client()
            .await?
            .vaults_client()
            .create_or_update(
                resource_group_name,
                vault_name.clone(),
                parameters,
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Key Vault",
            result,
            "vault create or update",
            "Azure Key Vault",
            &vault_name,
        )
    }

    async fn delete_vault(&self, resource_group_name: String, vault_name: String) -> Result<()> {
        let result = self
            .client()
            .await?
            .vaults_client()
            .delete(
                resource_group_name,
                vault_name.clone(),
                self.config.subscription_id.clone(),
            )
            .send()
            .await
            .map(|_| ());
        map_azure_core_021_sdk_error(
            "Azure Key Vault",
            result,
            "vault delete",
            "Azure Key Vault",
            &vault_name,
        )
    }

    async fn get_vault(&self, resource_group_name: String, vault_name: String) -> Result<Vault> {
        let result = self
            .client()
            .await?
            .vaults_client()
            .get(
                resource_group_name,
                vault_name.clone(),
                self.config.subscription_id.clone(),
            )
            .await;
        map_azure_core_021_sdk_error(
            "Azure Key Vault",
            result,
            "vault get",
            "Azure Key Vault",
            &vault_name,
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
    fn get_gcp_iam_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpIamApi>>;
    fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn CloudRunApi>>;
    fn get_gcp_resource_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ResourceManagerApi>>;
    async fn get_gcp_service_usage_client(&self, config: &GcpClientConfig) -> Result<ServiceUsage>;
    fn get_gcp_gcs_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcsApi>>;
    fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ArtifactRegistryApi>>;
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
    fn get_gcp_compute_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpComputeApi>>;
    async fn get_gcp_cloud_scheduler_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<CloudScheduler>;
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
    fn get_gcp_iam_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpIamApi>> {
        Ok(Arc::new(IamClient::new(config.clone())))
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

    async fn get_gcp_service_usage_client(&self, config: &GcpClientConfig) -> Result<ServiceUsage> {
        service_usage_client_from_alien_config(config).await
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

    fn get_gcp_compute_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpComputeApi>> {
        Ok(Arc::new(OfficialGcpComputeClient::new(config.clone())))
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

fn artifact_registry_repository_resource_name(
    project_id: &str,
    location: &str,
    repository_id: &str,
) -> String {
    format!("projects/{project_id}/locations/{location}/repositories/{repository_id}")
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

#[derive(Debug)]
struct AzureCore021Credential {
    inner: Arc<dyn TokenCredential>,
}

impl AzureCore021Credential {
    fn new(inner: Arc<dyn TokenCredential>) -> Self {
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

fn map_azure_core_021_sdk_error<T>(
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

async fn map_azure_core_021_lro_response<T, R, F, Fut>(
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

async fn map_azure_core_021_delete_lro_response<R>(
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
