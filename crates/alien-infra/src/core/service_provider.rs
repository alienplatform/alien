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
    container_apps::{AzureContainerAppsClient, ContainerAppsApi},
    long_running_operation::{LongRunningOperationApi, LongRunningOperationClient},
    AzureTokenCache,
};
#[cfg(feature = "kubernetes")]
use alien_core::KubernetesClientConfig;
use alien_core::{
    AwsClientConfig, AzureClientConfig, AzureCredentials, GcpCredentials, GcpImpersonationConfig,
};
use alien_error::{AlienError, Context, ContextError as _, IntoAlienError, IntoAlienErrorDirect};
use alien_gcp_clients::{
    cloudrun::{CloudRunApi, CloudRunClient},
    cloudscheduler::{CloudSchedulerApi, CloudSchedulerClient},
    compute::{ComputeApi as GcpComputeApi, ComputeClient as GcpComputeClient},
    gcs::{GcsApi, GcsClient},
    iam::{IamApi as GcpIamApi, IamClient as GcpIamClient, IamPolicy as GcpIamPolicy},
    pubsub::{PubSubApi, PubSubClient},
    GcpClientConfig,
};
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
use google_cloud_api_serviceusage_v1::{client::ServiceUsage, model::Service};
use google_cloud_artifactregistry_v1::{
    client::ArtifactRegistry as OfficialArtifactRegistry,
    model::{
        repository::Format as OfficialArtifactRegistryRepositoryFormat,
        Repository as OfficialArtifactRegistryRepository,
    },
};
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use google_cloud_firestore_admin_v1::{
    client::FirestoreAdmin, model::Database as FirestoreDatabase,
};
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_longrunning::model::Operation;
use google_cloud_resourcemanager_v3::{client::Projects, model::Project as OfficialGcpProject};
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashMap, future::Future, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::OnceCell;
use uuid::Uuid;

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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRegistryRepository {
    /// Artifact Registry repository resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Package format stored in the repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ArtifactRegistryRepositoryFormat>,
    /// User-provided repository description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// User-defined repository labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
    /// Repository creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,
    /// Repository update timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,
    /// Customer-managed encryption key resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_name: Option<String>,
    /// Repository mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Cleanup policies keyed by policy ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_policies: Option<HashMap<String, Value>>,
    /// Repository size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<String>,
    /// Whether the repository satisfies physical zone separation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub satisfies_pzs: Option<bool>,
    /// Whether cleanup policies are dry-run only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_policy_dry_run: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ArtifactRegistryRepositoryFormat {
    /// Unspecified package format.
    FormatUnspecified,
    /// Docker package format.
    Docker,
    /// Maven package format.
    Maven,
    /// NPM package format.
    Npm,
    /// APT package format.
    Apt,
    /// YUM package format.
    Yum,
    /// Python package format.
    Python,
    /// Go package format.
    Go,
    /// Generic package format.
    Generic,
    /// Ruby package format.
    Ruby,
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
            .set_repository(artifact_registry_repository_to_official(repository)?)
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
            Ok(repository) => Ok(artifact_registry_repository_from_official(repository)),
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
            .map(gcp_iam_policy_from_official)
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
            .set_policy(gcp_iam_policy_to_official(iam_policy)?)
            .send()
            .await
            .map(gcp_iam_policy_from_official)
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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetPolicyOptions {
    /// Maximum IAM policy version to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_policy_version: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// User-assigned Google Cloud project ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Numeric Google Cloud project number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_number: Option<String>,
    /// Google Cloud project resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Project lifecycle state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_state: Option<String>,
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
            request = request.set_options(
                google_cloud_iam_v1::model::GetPolicyOptions::new().set_requested_policy_version(
                    options.requested_policy_version.unwrap_or_default(),
                ),
            );
        }

        request
            .send()
            .await
            .map(gcp_iam_policy_from_official)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Resource Manager get_iam_policy request failed".to_string(),
                resource_id: Some(project_id),
            })
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
            .set_policy(gcp_iam_policy_to_official(policy)?);

        request
            .send()
            .await
            .map(gcp_iam_policy_from_official)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: "Resource Manager set_iam_policy request failed".to_string(),
                resource_id: Some(project_id),
            })
    }

    async fn get_project_metadata(&self, project_id: String) -> Result<Project> {
        self.client()
            .await?
            .get_project()
            .set_name(format!("projects/{project_id}"))
            .send()
            .await
            .map(project_from_official)
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
        #[derive(Deserialize)]
        struct RoleAssignmentListResponse {
            value: Vec<RoleAssignment>,
        }

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
        let response = serde_json::from_str::<RoleAssignmentListResponse>(&response)
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleAssignment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<RoleAssignmentProperties>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignmentProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_managed_identity_resource_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub principal_id: String,
    pub principal_type: RoleAssignmentPropertiesPrincipalType,
    pub role_definition_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_on: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
pub enum RoleAssignmentPropertiesPrincipalType {
    User,
    Group,
    ServicePrincipal,
    ForeignGroup,
    Device,
}

impl Default for RoleAssignmentPropertiesPrincipalType {
    fn default() -> Self {
        Self::User
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleDefinition {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<RoleDefinitionProperties>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDefinitionProperties {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignable_scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<Permission>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_on: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_data_actions: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    /// Fully qualified Azure resource ID for the managed identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Azure region where the managed identity lives.
    pub location: String,
    /// Managed identity resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// User-assigned identity properties returned by ARM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<UserAssignedIdentityProperties>,
    /// ARM system metadata returned for the identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_data: Option<SystemData>,
    /// Resource tags attached to the managed identity.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Azure resource type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserAssignedIdentityProperties {
    /// Client ID of the user-assigned managed identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Isolation scope returned by ARM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation_scope: Option<String>,
    /// Principal/object ID of the user-assigned managed identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
    /// Tenant ID associated with the managed identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemData {
    /// Timestamp when ARM created the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Identity that created the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// Type of identity that created the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_type: Option<String>,
    /// Timestamp when ARM last modified the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_at: Option<String>,
    /// Identity that last modified the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_by: Option<String>,
    /// Type of identity that last modified the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_by_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FederatedIdentityCredential {
    /// Fully qualified Azure resource ID for the federated credential.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Federated credential resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Azure resource type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// Federated credential properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<FederatedCredentialProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FederatedCredentialProperties {
    /// The audiences that can appear in the issued token.
    pub audiences: Vec<String>,
    /// Issuer URL trusted for the federated credential.
    pub issuer: String,
    /// External identity subject trusted for the federated credential.
    pub subject: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    /// Fully qualified Azure resource ID for the registry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Managed identity configuration for the registry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Value>,
    /// Azure region where the registry lives.
    pub location: String,
    /// Container registry name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Container registry properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<RegistryProperties>,
    /// Container registry SKU.
    pub sku: Sku,
    /// ARM system metadata returned for the registry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_data: Option<Value>,
    /// Resource tags attached to the registry.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Azure resource type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryProperties {
    /// Whether the admin user is enabled.
    #[serde(default)]
    pub admin_user_enabled: bool,
    /// Whether anonymous pulls are enabled.
    #[serde(default)]
    pub anonymous_pull_enabled: bool,
    /// Registry creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_date: Option<String>,
    /// Whether a data endpoint is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_endpoint_enabled: Option<bool>,
    /// Data endpoint host names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_endpoint_host_names: Vec<String>,
    /// Registry encryption settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionProperty>,
    /// Registry login server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_server: Option<String>,
    /// Network rule bypass behavior.
    #[serde(default = "default_acr_network_rule_bypass_options")]
    pub network_rule_bypass_options: String,
    /// Registry network rule set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_rule_set: Option<NetworkRuleSet>,
    /// Registry policy settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policies: Option<Policies>,
    /// Private endpoint connections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_endpoint_connections: Vec<Value>,
    /// ARM provisioning state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
    /// Whether public network access is allowed.
    #[serde(default = "default_enabled")]
    pub public_network_access: String,
    /// Registry status payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Value>,
    /// Whether zone redundancy is enabled.
    #[serde(default = "default_disabled")]
    pub zone_redundancy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sku {
    /// Container registry SKU name.
    pub name: String,
    /// Container registry SKU tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRuleSet {
    /// Default action when no network rule matches.
    #[serde(default = "default_allow")]
    pub default_action: String,
    /// IP ACL rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ip_rules: Vec<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionProperty {
    /// Key Vault settings used for customer-managed keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_vault_properties: Option<KeyVaultProperties>,
    /// Encryption status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyVaultProperties {
    /// Key vault URI to access the encryption key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_identifier: Option<String>,
    /// Versioned key identifier used for encryption.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versioned_key_identifier: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policies {
    /// Azure AD authentication as ARM policy payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure_ad_authentication_as_arm_policy: Option<Value>,
    /// Export policy payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export_policy: Option<Value>,
    /// Quarantine policy payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quarantine_policy: Option<Value>,
    /// Retention policy payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_policy: Option<Value>,
    /// Trust policy payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_policy: Option<Value>,
}

fn default_acr_network_rule_bypass_options() -> String {
    "AzureServices".to_string()
}

fn default_enabled() -> String {
    "Enabled".to_string()
}

fn default_disabled() -> String {
    "Disabled".to_string()
}

fn default_allow() -> String {
    "Allow".to_string()
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubResource {
    /// Azure resource ID reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressSpace {
    /// Address prefixes associated with the virtual network.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub address_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualNetwork {
    /// Azure virtual network resource ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Azure region where the virtual network lives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Azure virtual network resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Virtual network properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<VirtualNetworkPropertiesFormat>,
    /// Resource tags attached to the virtual network.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Azure resource type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualNetworkPropertiesFormat {
    /// Virtual network address space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_space: Option<AddressSpace>,
    /// Subnets attached to the virtual network.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subnets: Vec<Subnet>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subnet {
    /// Azure subnet resource ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Azure subnet resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Subnet properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<SubnetPropertiesFormat>,
    /// Azure resource type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubnetPropertiesFormat {
    /// Single subnet address prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_prefix: Option<String>,
    /// Multiple subnet address prefixes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub address_prefixes: Vec<String>,
    /// NAT gateway associated with the subnet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_gateway: Option<SubResource>,
    /// Network security group associated with the subnet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_security_group: Option<SubResource>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicIpAddress {
    /// Azure public IP resource ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Azure region where the public IP lives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Azure public IP resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Public IP properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<PublicIpAddressPropertiesFormat>,
    /// Public IP SKU.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<PublicIpAddressSku>,
    /// Resource tags attached to the public IP.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Azure resource type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicIpAddressSku {
    /// Public IP SKU name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Public IP SKU tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicIpAddressPropertiesFormat {
    /// Public IP allocation method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_ip_allocation_method: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGateway {
    /// Azure NAT gateway resource ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Azure region where the NAT gateway lives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Azure NAT gateway resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// NAT gateway properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<NatGatewayPropertiesFormat>,
    /// NAT gateway SKU.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<NatGatewaySku>,
    /// Resource tags attached to the NAT gateway.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Azure resource type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGatewaySku {
    /// NAT gateway SKU name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGatewayPropertiesFormat {
    /// Public IP addresses attached to the NAT gateway.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_ip_addresses: Vec<SubResource>,
    /// Idle timeout in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_timeout_in_minutes: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSecurityGroup {
    /// Azure NSG resource ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Azure region where the NSG lives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Azure NSG resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// NSG properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<NetworkSecurityGroupPropertiesFormat>,
    /// Resource tags attached to the NSG.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Azure resource type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSecurityGroupPropertiesFormat {
    /// NSG security rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub security_rules: Vec<SecurityRule>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRule {
    /// Security rule name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Security rule properties.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<SecurityRulePropertiesFormat>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRulePropertiesFormat {
    /// Security rule description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Rule protocol, such as "*" or "Tcp".
    pub protocol: String,
    /// Source port range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_port_range: Option<String>,
    /// Destination port range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_port_range: Option<String>,
    /// Source address prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_address_prefix: Option<String>,
    /// Destination address prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_address_prefix: Option<String>,
    /// Source port ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_port_ranges: Vec<String>,
    /// Destination port ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_port_ranges: Vec<String>,
    /// Source address prefixes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_address_prefixes: Vec<String>,
    /// Destination address prefixes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_address_prefixes: Vec<String>,
    /// Source application security groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_application_security_groups: Vec<Value>,
    /// Destination application security groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_application_security_groups: Vec<Value>,
    /// Rule access.
    pub access: String,
    /// Rule priority.
    pub priority: i32,
    /// Rule direction.
    pub direction: String,
    /// Rule provisioning state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
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
        let table = AzureTableArmResource {
            id: None,
            name: Some(table_name.to_string()),
            properties: Some(AzureTableArmProperties {
                signed_identifiers: vec![],
                table_name: Some(table_name.to_string()),
            }),
            type_: None,
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
        let table = serde_json::from_str::<AzureTableArmResource>(&body)
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
        resource_group: &AzureArmResourceGroup,
    ) -> Result<AzureArmResourceGroup>;

    async fn delete_resource_group(&self, resource_group_name: &str) -> Result<()>;

    async fn get_resource_group(&self, resource_group_name: &str) -> Result<AzureArmResourceGroup>;

    async fn get_provider(&self, resource_provider_namespace: &str) -> Result<AzureArmProvider>;

    async fn register_provider(
        &self,
        resource_provider_namespace: &str,
    ) -> Result<AzureArmProvider>;
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
        resource_group: &AzureArmResourceGroup,
    ) -> Result<AzureArmResourceGroup> {
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

    async fn get_resource_group(&self, resource_group_name: &str) -> Result<AzureArmResourceGroup> {
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

    async fn get_provider(&self, resource_provider_namespace: &str) -> Result<AzureArmProvider> {
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

    async fn register_provider(
        &self,
        resource_provider_namespace: &str,
    ) -> Result<AzureArmProvider> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureArmResourceGroup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub location: String,
    #[serde(rename = "managedBy", skip_serializing_if = "Option::is_none")]
    pub managed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<AzureArmResourceGroupProperties>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureArmResourceGroupProperties {
    #[serde(rename = "provisioningState", skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureArmProvider {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(rename = "registrationPolicy", skip_serializing_if = "Option::is_none")]
    pub registration_policy: Option<String>,
    #[serde(rename = "registrationState", skip_serializing_if = "Option::is_none")]
    pub registration_state: Option<String>,
    #[serde(
        rename = "resourceTypes",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub resource_types: Vec<Value>,
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
        parameters: AzureServiceBusNamespaceProperties,
    ) -> Result<AzureServiceBusNamespace>;

    async fn get_namespace(
        &self,
        resource_group_name: String,
        namespace_name: String,
    ) -> Result<AzureServiceBusNamespace>;

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
        parameters: AzureServiceBusQueueProperties,
    ) -> Result<AzureServiceBusQueue>;

    async fn get_queue(
        &self,
        resource_group_name: String,
        namespace_name: String,
        queue_name: String,
    ) -> Result<AzureServiceBusQueue>;

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
        parameters: AzureServiceBusNamespaceProperties,
    ) -> Result<AzureServiceBusNamespace> {
        let namespace = AzureServiceBusNamespace {
            id: None,
            location: self
                .config
                .region
                .clone()
                .unwrap_or_else(|| "eastus".to_string()),
            name: Some(namespace_name.clone()),
            properties: Some(parameters),
            sku: None,
            tags: HashMap::new(),
            type_: None,
        };
        let body = serde_json::to_string(&namespace)
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
    ) -> Result<AzureServiceBusNamespace> {
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
        parameters: AzureServiceBusQueueProperties,
    ) -> Result<AzureServiceBusQueue> {
        let queue = AzureServiceBusQueue {
            id: None,
            location: None,
            name: Some(queue_name.clone()),
            properties: Some(parameters),
            type_: None,
        };
        let body = serde_json::to_string(&queue).into_alien_error().context(
            crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to serialize Azure Service Bus queue '{queue_name}' request"
                ),
                resource_id: None,
            },
        )?;
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
    ) -> Result<AzureServiceBusQueue> {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureServiceBusNamespace {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default)]
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<AzureServiceBusNamespaceProperties>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<AzureServiceBusSku>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureServiceBusNamespaceProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alternate_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_local_auth: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_tls_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub premium_messaging_partitions: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_endpoint_connections: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
    pub public_network_access: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_bus_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_redundant: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureServiceBusSku {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity: Option<i32>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureServiceBusQueue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<AzureServiceBusQueueProperties>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureServiceBusQueueProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_delete_on_idle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_details: Option<AzureServiceBusMessageCountDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_lettering_on_message_expiration: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_message_time_to_live: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_detection_history_time_window: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_batched_operations: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_express: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_partitioning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_dead_lettered_messages_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_delivery_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_message_size_in_kilobytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size_in_megabytes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_duplicate_detection: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_session: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_in_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureServiceBusMessageCountDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_message_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_letter_message_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_message_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_dead_letter_message_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_message_count: Option<i64>,
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait StorageAccountsApi: Send + Sync + std::fmt::Debug {
    async fn create_storage_account(
        &self,
        resource_group_name: &str,
        account_name: &str,
        parameters: &AzureStorageAccountArmResource,
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
    ) -> Result<AzureStorageAccountArmResource>;
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
        parameters: &AzureStorageAccountArmResource,
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
    ) -> Result<AzureStorageAccountArmResource> {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureStorageAccountArmResource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<AzureStorageAccountProperties>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<AzureStorageSku>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureStorageAccountProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_blob_public_access: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_shared_key_access: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption: Option<AzureStorageEncryption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_tls_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_acls: Option<AzureStorageNetworkRuleSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_endpoints: Option<AzureStorageAccountEndpoints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_network_access: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_endpoints: Option<AzureStorageAccountEndpoints>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_of_primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_of_secondary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_https_traffic_only: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureStorageSku {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureStorageAccountEndpoints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dfs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureStorageEncryption {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_infrastructure_encryption: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services: Option<AzureStorageEncryptionServices>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureStorageEncryptionServices {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<AzureStorageEncryptionService>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<AzureStorageEncryptionService>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<AzureStorageEncryptionService>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<AzureStorageEncryptionService>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureStorageEncryptionService {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureStorageNetworkRuleSet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypass: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_action: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ip_rules: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_access_rules: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub virtual_network_rules: Vec<Value>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureBlobContainer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<AzureBlobContainerProperties>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureBlobContainerProperties {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_access: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_time: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AzureBlobServiceProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<AzureBlobServicePropertiesData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<AzureStorageSku>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureBlobServicePropertiesData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_feed: Option<AzureBlobChangeFeed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_delete_retention_policy: Option<AzureBlobDeleteRetentionPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_retention_policy: Option<AzureBlobDeleteRetentionPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_versioning_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureBlobChangeFeed {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_in_days: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureBlobDeleteRetentionPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_permanent_delete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureKeyVaultCreateOrUpdateParameters {
    pub location: String,
    pub properties: AzureKeyVaultProperties,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureKeyVault {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub properties: AzureKeyVaultProperties,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureKeyVaultProperties {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub access_policies: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_purge_protection: Option<bool>,
    pub enable_rbac_authorization: bool,
    pub enable_soft_delete: bool,
    pub enabled_for_deployment: bool,
    pub enabled_for_disk_encryption: bool,
    pub enabled_for_template_deployment: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hsm_pool_resource_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_acls: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_endpoint_connections: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
    pub public_network_access: String,
    pub sku: AzureKeyVaultSku,
    pub soft_delete_retention_in_days: i32,
    pub tenant_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureKeyVaultSku {
    pub family: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AzureTableArmResource {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<AzureTableArmProperties>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AzureTableArmProperties {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    signed_identifiers: Vec<AzureTableSignedIdentifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    table_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AzureTableSignedIdentifier {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    access_policy: Option<AzureTableAccessPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AzureTableAccessPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expiry_time: Option<String>,
    permission: String,
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
        Ok(Arc::new(GcpIamClient::new(
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
        Ok(Arc::new(GcsClient::new(
            reqwest::Client::new(),
            config.clone(),
        )))
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
        Ok(Arc::new(AzureContainerAppsClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
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
        Ok(Arc::new(LongRunningOperationClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(config.clone()),
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

fn artifact_registry_repository_to_official(
    repository: ArtifactRegistryRepository,
) -> Result<OfficialArtifactRegistryRepository> {
    let mut official = OfficialArtifactRegistryRepository::new();

    if let Some(name) = repository.name {
        official = official.set_name(name);
    }
    if let Some(format) = repository.format {
        official = official.set_format(official_artifact_registry_format(format));
    }
    if let Some(description) = repository.description {
        official = official.set_description(description);
    }
    if let Some(labels) = repository.labels {
        official = official.set_labels(labels);
    }
    if let Some(kms_key_name) = repository.kms_key_name {
        official = official.set_kms_key_name(kms_key_name);
    }
    if let Some(cleanup_policy_dry_run) = repository.cleanup_policy_dry_run {
        official = official.set_cleanup_policy_dry_run(cleanup_policy_dry_run);
    }

    Ok(official)
}

fn artifact_registry_repository_from_official(
    repository: OfficialArtifactRegistryRepository,
) -> ArtifactRegistryRepository {
    ArtifactRegistryRepository {
        name: none_if_empty(repository.name),
        format: local_artifact_registry_format(repository.format),
        description: none_if_empty(repository.description),
        labels: if repository.labels.is_empty() {
            None
        } else {
            Some(repository.labels)
        },
        create_time: repository.create_time.map(String::from),
        update_time: repository.update_time.map(String::from),
        kms_key_name: none_if_empty(repository.kms_key_name),
        mode: Some(format!("{:?}", repository.mode)),
        cleanup_policies: if repository.cleanup_policies.is_empty() {
            None
        } else {
            Some(
                repository
                    .cleanup_policies
                    .into_keys()
                    .map(|key| (key, Value::Null))
                    .collect(),
            )
        },
        size_bytes: Some(repository.size_bytes.to_string()),
        satisfies_pzs: Some(repository.satisfies_pzs),
        cleanup_policy_dry_run: Some(repository.cleanup_policy_dry_run),
    }
}

fn official_artifact_registry_format(
    format: ArtifactRegistryRepositoryFormat,
) -> OfficialArtifactRegistryRepositoryFormat {
    match format {
        ArtifactRegistryRepositoryFormat::FormatUnspecified => {
            OfficialArtifactRegistryRepositoryFormat::Unspecified
        }
        ArtifactRegistryRepositoryFormat::Docker => {
            OfficialArtifactRegistryRepositoryFormat::Docker
        }
        ArtifactRegistryRepositoryFormat::Maven => OfficialArtifactRegistryRepositoryFormat::Maven,
        ArtifactRegistryRepositoryFormat::Npm => OfficialArtifactRegistryRepositoryFormat::Npm,
        ArtifactRegistryRepositoryFormat::Apt => OfficialArtifactRegistryRepositoryFormat::Apt,
        ArtifactRegistryRepositoryFormat::Yum => OfficialArtifactRegistryRepositoryFormat::Yum,
        ArtifactRegistryRepositoryFormat::Python => {
            OfficialArtifactRegistryRepositoryFormat::Python
        }
        ArtifactRegistryRepositoryFormat::Go => OfficialArtifactRegistryRepositoryFormat::Go,
        ArtifactRegistryRepositoryFormat::Generic => {
            OfficialArtifactRegistryRepositoryFormat::Generic
        }
        ArtifactRegistryRepositoryFormat::Ruby => OfficialArtifactRegistryRepositoryFormat::Ruby,
    }
}

fn local_artifact_registry_format(
    format: OfficialArtifactRegistryRepositoryFormat,
) -> Option<ArtifactRegistryRepositoryFormat> {
    match format {
        OfficialArtifactRegistryRepositoryFormat::Unspecified => {
            Some(ArtifactRegistryRepositoryFormat::FormatUnspecified)
        }
        OfficialArtifactRegistryRepositoryFormat::Docker => {
            Some(ArtifactRegistryRepositoryFormat::Docker)
        }
        OfficialArtifactRegistryRepositoryFormat::Maven => {
            Some(ArtifactRegistryRepositoryFormat::Maven)
        }
        OfficialArtifactRegistryRepositoryFormat::Npm => {
            Some(ArtifactRegistryRepositoryFormat::Npm)
        }
        OfficialArtifactRegistryRepositoryFormat::Apt => {
            Some(ArtifactRegistryRepositoryFormat::Apt)
        }
        OfficialArtifactRegistryRepositoryFormat::Yum => {
            Some(ArtifactRegistryRepositoryFormat::Yum)
        }
        OfficialArtifactRegistryRepositoryFormat::Python => {
            Some(ArtifactRegistryRepositoryFormat::Python)
        }
        OfficialArtifactRegistryRepositoryFormat::Go => Some(ArtifactRegistryRepositoryFormat::Go),
        OfficialArtifactRegistryRepositoryFormat::Generic => {
            Some(ArtifactRegistryRepositoryFormat::Generic)
        }
        OfficialArtifactRegistryRepositoryFormat::Ruby => {
            Some(ArtifactRegistryRepositoryFormat::Ruby)
        }
        OfficialArtifactRegistryRepositoryFormat::Kfp
        | OfficialArtifactRegistryRepositoryFormat::UnknownValue(_) => None,
        _ => None,
    }
}

fn none_if_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn gcp_iam_policy_from_official(policy: google_cloud_iam_v1::model::Policy) -> GcpIamPolicy {
    use base64::Engine;

    GcpIamPolicy {
        version: Some(policy.version),
        kind: None,
        resource_id: None,
        bindings: policy
            .bindings
            .into_iter()
            .map(|binding| alien_gcp_clients::iam::Binding {
                role: binding.role,
                members: binding.members,
                condition: binding
                    .condition
                    .map(|condition| alien_gcp_clients::iam::Expr {
                        expression: condition.expression,
                        title: if condition.title.is_empty() {
                            None
                        } else {
                            Some(condition.title)
                        },
                        description: if condition.description.is_empty() {
                            None
                        } else {
                            Some(condition.description)
                        },
                        location: if condition.location.is_empty() {
                            None
                        } else {
                            Some(condition.location)
                        },
                    }),
            })
            .collect(),
        etag: if policy.etag.is_empty() {
            None
        } else {
            Some(base64::engine::general_purpose::STANDARD.encode(policy.etag))
        },
    }
}

fn gcp_iam_policy_to_official(policy: GcpIamPolicy) -> Result<google_cloud_iam_v1::model::Policy> {
    use base64::Engine;

    let etag = policy
        .etag
        .as_deref()
        .map(|etag| {
            base64::engine::general_purpose::STANDARD
                .decode(etag)
                .into_alien_error()
                .context(crate::error::ErrorData::CloudPlatformError {
                    message: "Failed to base64-decode GCP IAM policy etag".to_string(),
                    resource_id: policy.resource_id.clone(),
                })
        })
        .transpose()?
        .unwrap_or_default();

    Ok(google_cloud_iam_v1::model::Policy::new()
        .set_version(policy.version.unwrap_or_default())
        .set_bindings(policy.bindings.into_iter().map(|binding| {
            let mut official_binding = google_cloud_iam_v1::model::Binding::new()
                .set_role(binding.role)
                .set_members(binding.members);

            if let Some(condition) = binding.condition {
                official_binding = official_binding.set_condition(
                    google_cloud_type::model::Expr::new()
                        .set_expression(condition.expression)
                        .set_title(condition.title.unwrap_or_default())
                        .set_description(condition.description.unwrap_or_default())
                        .set_location(condition.location.unwrap_or_default()),
                );
            }

            official_binding
        }))
        .set_etag(etag))
}

fn project_from_official(project: OfficialGcpProject) -> Project {
    let project_number = project
        .name
        .strip_prefix("projects/")
        .filter(|number| number.chars().all(|ch| ch.is_ascii_digit()))
        .map(ToString::to_string);

    Project {
        project_id: if project.project_id.is_empty() {
            None
        } else {
            Some(project.project_id)
        },
        project_number,
        name: if project.name.is_empty() {
            None
        } else {
            Some(project.name)
        },
        lifecycle_state: Some(format!("{:?}", project.state)),
    }
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::NOT_FOUND.as_u16())
}

fn gcp_credentials_from_alien_config(config: &GcpClientConfig) -> Result<Credentials> {
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

fn azure_credential_from_config(config: &AzureClientConfig) -> Result<Arc<dyn TokenCredential>> {
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

fn azure_management_endpoint(config: &AzureClientConfig) -> &str {
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
