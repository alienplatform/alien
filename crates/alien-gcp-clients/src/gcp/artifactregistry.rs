use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::iam::IamPolicy;
use crate::gcp::longrunning::Operation;
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// Artifact Registry service configuration
#[derive(Debug)]
pub struct ArtifactRegistryServiceConfig;

impl GcpServiceConfig for ArtifactRegistryServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://artifactregistry.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://artifactregistry.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Artifact Registry"
    }

    fn service_key(&self) -> &'static str {
        "artifactregistry"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ArtifactRegistryApi: Send + Sync + Debug {
    /// Creates a repository in the given project and location.
    async fn create_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        repository: Repository,
    ) -> Result<Operation>;

    /// Deletes a repository in the given project and location.
    async fn delete_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<Operation>;

    /// Gets a repository.
    async fn get_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<Repository>;

    /// Updates a repository.
    async fn patch_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        repository: Repository,
        update_mask: Option<String>,
    ) -> Result<Repository>;

    /// Gets the IAM policy for a repository.
    async fn get_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<IamPolicy>;

    /// Sets the IAM policy for a repository.
    async fn set_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;

    /// Gets information about a long-running operation.
    async fn get_operation(
        &self,
        project_id: String,
        location: String,
        operation_name: String,
    ) -> Result<Operation>;
}

/// Artifact Registry client for managing repositories and IAM policies
#[derive(Debug)]
pub struct ArtifactRegistryClient {
    base: GcpClientBase,
}

impl ArtifactRegistryClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        Self {
            base: GcpClientBase::new(client, config, Box::new(ArtifactRegistryServiceConfig)),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ArtifactRegistryApi for ArtifactRegistryClient {
    /// Creates a repository in the given project and location.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project
    /// * `location` - The location for the repository (e.g., "us-central1")
    /// * `repository_id` - The ID of the repository to create
    /// * `repository` - The repository configuration
    ///
    /// # Returns
    ///
    /// Returns a long-running operation for the repository creation.
    ///
    /// See: https://cloud.google.com/artifact-registry/docs/reference/rest/v1/projects.locations.repositories/create
    async fn create_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        repository: Repository,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/repositories",
            project_id, location
        );
        let query_params = vec![("repositoryId", repository_id.to_string())];

        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params),
                Some(repository),
                &repository_id,
            )
            .await
    }

    /// Deletes a repository in the given project and location.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project
    /// * `location` - The location of the repository (e.g., "us-central1")
    /// * `repository_id` - The ID of the repository to delete
    ///
    /// # Returns
    ///
    /// Returns a long-running operation for the repository deletion.
    ///
    /// See: https://cloud.google.com/artifact-registry/docs/reference/rest/v1/projects.locations.repositories/delete
    async fn delete_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/repositories/{}",
            project_id, location, repository_id
        );

        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &repository_id,
            )
            .await
    }

    /// Gets a repository.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project
    /// * `location` - The location of the repository (e.g., "us-central1")
    /// * `repository_id` - The ID of the repository
    ///
    /// # Returns
    ///
    /// Returns the repository details.
    ///
    /// See: https://cloud.google.com/artifact-registry/docs/reference/rest/v1/projects.locations.repositories/get
    async fn get_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<Repository> {
        let path = format!(
            "projects/{}/locations/{}/repositories/{}",
            project_id, location, repository_id
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &repository_id)
            .await
    }

    /// Updates a repository.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project
    /// * `location` - The location of the repository (e.g., "us-central1")
    /// * `repository_id` - The ID of the repository to update
    /// * `repository` - The updated repository configuration
    /// * `update_mask` - Optional field mask specifying which fields to update
    ///
    /// # Returns
    ///
    /// Returns the updated repository.
    ///
    /// See: https://cloud.google.com/artifact-registry/docs/reference/rest/v1/projects.locations.repositories/patch
    async fn patch_repository(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        repository: Repository,
        update_mask: Option<String>,
    ) -> Result<Repository> {
        let path = format!(
            "projects/{}/locations/{}/repositories/{}",
            project_id, location, repository_id
        );
        let mut query_params = Vec::new();

        if let Some(mask) = update_mask {
            query_params.push(("updateMask", mask));
        }

        self.base
            .execute_request(
                Method::PATCH,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Some(repository),
                &repository_id,
            )
            .await
    }

    /// Gets the IAM policy for an Artifact Registry repository.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project
    /// * `location` - The location of the repository (e.g., "us-central1")
    /// * `repository_id` - The ID of the repository
    ///
    /// # Returns
    ///
    /// Returns the current IAM policy for the specified repository.
    ///
    /// See: https://cloud.google.com/artifact-registry/docs/reference/rest/v1/projects.locations.repositories/getIamPolicy
    async fn get_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
    ) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/locations/{}/repositories/{}:getIamPolicy",
            project_id, location, repository_id
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &repository_id)
            .await
    }

    /// Sets the IAM policy for an Artifact Registry repository.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project
    /// * `location` - The location of the repository (e.g., "us-central1")
    /// * `repository_id` - The ID of the repository  
    /// * `iam_policy` - The complete IAM policy to apply to the repository
    ///
    /// # Returns
    ///
    /// Returns the updated IAM policy as applied to the repository.
    ///
    /// See: https://cloud.google.com/artifact-registry/docs/reference/rest/v1/projects.locations.repositories/setIamPolicy
    async fn set_repository_iam_policy(
        &self,
        project_id: String,
        location: String,
        repository_id: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/locations/{}/repositories/{}:setIamPolicy",
            project_id, location, repository_id
        );
        let request = SetIamPolicyRequest { policy: iam_policy };

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &repository_id)
            .await
    }

    /// Gets information about a long-running operation.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project
    /// * `location` - The location of the operation (e.g., "us-central1")
    /// * `operation_name` - The name of the operation
    ///
    /// # Returns
    ///
    /// Returns the operation details.
    ///
    /// See: https://cloud.google.com/artifact-registry/docs/reference/rest/v1/projects.locations.operations/get
    async fn get_operation(
        &self,
        project_id: String,
        location: String,
        operation_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/operations/{}",
            project_id, location, operation_name
        );

        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }
}

// --- Data Structures ---

/// An Artifact Registry repository.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    /// The name of the repository, for example: "projects/p1/locations/us-central1/repositories/repo1".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional. The format of packages that are stored in the repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<RepositoryFormat>,

    /// Optional. The user-provided description of the repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Labels with user-defined metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Output only. The time when the repository was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// Output only. The time when the repository was last updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,

    /// The Cloud KMS resource name of the customer-managed encryption key (CMEK).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_name: Option<String>,

    /// The mode configures the repository to serve artifacts from different sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<RepositoryMode>,

    /// Optional. Cleanup policies for this repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_policies: Option<HashMap<String, CleanupPolicy>>,

    /// Output only. The size, in bytes, of all artifact storage in this repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<String>,

    /// Output only. If set, the repository satisfies physical zone separation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub satisfies_pzs: Option<bool>,

    /// Optional. Config and state for cleanup policies for this repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup_policy_dry_run: Option<bool>,

    /// Configuration specific to the repository type.
    #[serde(flatten)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_config: Option<RepositoryConfig>,
}

/// The format of packages that are stored in the repository.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RepositoryFormat {
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
    /// Kubeflow Pipelines package format.
    Kfp,
    /// Go package format.
    Go,
    /// Generic package format.
    Generic,
}

/// The mode configures the repository to serve artifacts from different sources.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RepositoryMode {
    /// Unspecified mode.
    ModeUnspecified,
    /// A standard repository storing artifacts.
    StandardRepository,
    /// A virtual repository to serve artifacts from one or more sources.
    VirtualRepository,
    /// A remote repository to serve artifacts from a remote source.
    RemoteRepository,
}

/// Repository configuration that is specific to the repository type.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum RepositoryConfig {
    /// Configuration for a Maven repository.
    MavenConfig(MavenRepositoryConfig),
    /// Configuration for a Docker repository.
    DockerConfig(DockerRepositoryConfig),
    /// Configuration for a virtual repository.
    VirtualRepositoryConfig(VirtualRepositoryConfig),
    /// Configuration for a remote repository.
    RemoteRepositoryConfig(RemoteRepositoryConfig),
}

/// Configuration for a Maven repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MavenRepositoryConfig {
    /// The repository with this flag will allow publishing the same snapshot versions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_snapshot_overwrites: Option<bool>,

    /// Version policy defines the versions that the registry will accept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_policy: Option<MavenVersionPolicy>,
}

/// Maven version policy.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MavenVersionPolicy {
    /// VERSION_POLICY_UNSPECIFIED - the version policy is not defined.
    VersionPolicyUnspecified,
    /// RELEASE - repository will accept only Release versions.
    Release,
    /// SNAPSHOT - repository will accept only Snapshot versions.
    Snapshot,
}

/// Configuration for a Docker repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DockerRepositoryConfig {
    /// The repository which enabled this flag prevents all tags from being modified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub immutable_tags: Option<bool>,
}

/// Configuration for a virtual repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct VirtualRepositoryConfig {
    /// Repositories that are upstream to this repository.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstream_policies: Vec<UpstreamPolicy>,
}

/// Defines the behavior when a repository upstream policy is encountered.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamPolicy {
    /// A reference to the repository resource, for example: "projects/p1/locations/us-central1/repositories/repo1".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// The repository resource, for example: "projects/p1/locations/us-central1/repositories/repo1".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,

    /// Entries with a greater priority value take precedence in the pull order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

/// Configuration for a remote repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRepositoryConfig {
    /// Specific settings for different remote repository types.
    #[serde(flatten)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_source: Option<RemoteSource>,

    /// Optional. The description of the remote source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Input only. A create/update remote repo option to avoid making a HEAD/GET request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_upstream_validation: Option<bool>,
}

/// Remote repository configuration based on the source type.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum RemoteSource {
    /// Configuration for a Docker remote repository.
    DockerRepository(DockerRepository),
    /// Configuration for a Maven remote repository.
    MavenRepository(MavenRepository),
    /// Configuration for an NPM remote repository.
    NpmRepository(NpmRepository),
    /// Configuration for a Python remote repository.
    PythonRepository(PythonRepository),
    /// Configuration for an APT remote repository.
    AptRepository(AptRepository),
    /// Configuration for a YUM remote repository.
    YumRepository(YumRepository),
}

/// Configuration for a Docker remote repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DockerRepository {
    /// Address of the remote repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_repository: Option<DockerRepositoryPublicRepository>,
}

/// Publicly available Docker repositories.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DockerRepositoryPublicRepository {
    /// Unspecified repository.
    PublicRepositoryUnspecified,
    /// Docker Hub.
    DockerHub,
}

/// Configuration for a Maven remote repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MavenRepository {
    /// Address of the remote repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_repository: Option<MavenRepositoryPublicRepository>,
}

/// Publicly available Maven repositories.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MavenRepositoryPublicRepository {
    /// Unspecified repository.
    PublicRepositoryUnspecified,
    /// Maven Central.
    MavenCentral,
}

/// Configuration for an NPM remote repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NpmRepository {
    /// Address of the remote repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_repository: Option<NpmRepositoryPublicRepository>,
}

/// Publicly available NPM repositories.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NpmRepositoryPublicRepository {
    /// Unspecified repository.
    PublicRepositoryUnspecified,
    /// NPM Registry.
    Npmjs,
}

/// Configuration for a Python remote repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct PythonRepository {
    /// Address of the remote repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_repository: Option<PythonRepositoryPublicRepository>,
}

/// Publicly available Python repositories.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PythonRepositoryPublicRepository {
    /// Unspecified repository.
    PublicRepositoryUnspecified,
    /// PyPI.
    Pypi,
}

/// Configuration for an APT remote repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AptRepository {
    /// Address of the remote repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_repository: Option<AptRepositoryPublicRepository>,
}

/// Publicly available APT repositories.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AptRepositoryPublicRepository {
    /// A common public repository base for APT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_base: Option<String>,

    /// A custom field to define a path to a GPG keyring file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_path: Option<String>,
}

/// Configuration for a YUM remote repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct YumRepository {
    /// Address of the remote repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_repository: Option<YumRepositoryPublicRepository>,
}

/// Publicly available YUM repositories.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct YumRepositoryPublicRepository {
    /// A common public repository base for YUM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_base: Option<String>,

    /// A custom field to define a path to a GPG keyring file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_path: Option<String>,
}

/// Cleanup policy for a repository.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPolicy {
    /// Policy ID supplied by the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Policy action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<CleanupPolicyAction>,

    /// Policy condition for matching versions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<CleanupPolicyCondition>,

    /// The user-provided description of the cleanup policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub most_recent_versions: Option<CleanupPolicyMostRecentVersions>,
}

/// Cleanup policy action.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CleanupPolicyAction {
    /// Action not specified.
    ActionUnspecified,
    /// Delete package versions.
    Delete,
    /// Keep package versions.
    Keep,
}

/// CleanupPolicyCondition is a set of conditions attached to a cleanup policy.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPolicyCondition {
    /// Match versions by tag status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_state: Option<CleanupPolicyConditionTagState>,

    /// Match versions by tag prefix.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag_prefixes: Vec<String>,

    /// Match versions by version name prefix.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub version_name_prefixes: Vec<String>,

    /// Match versions by package prefix.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub package_name_prefixes: Vec<String>,

    /// Match versions older than a duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub older_than: Option<String>,

    /// Match versions newer than a duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newer_than: Option<String>,
}

/// Cleanup policy condition tag state.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CleanupPolicyConditionTagState {
    /// Tag state not specified.
    TagStateUnspecified,
    /// Applies to tagged versions.
    Tagged,
    /// Applies to untagged versions.
    Untagged,
    /// Applies to all versions.
    Any,
}

/// CleanupPolicyMostRecentVersions is an alternate condition of a cleanup policy for retaining a minimum number of versions.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPolicyMostRecentVersions {
    /// Minimum number of versions to keep.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_keep_count: Option<i32>,

    /// List of package name prefixes that will apply this rule.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub package_name_prefixes: Vec<String>,

    /// Minimum number of versions to keep.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_count: Option<i32>,
}

/// Request message for setting IAM policy in Artifact Registry.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SetIamPolicyRequest {
    /// The complete policy to be applied to the resource.
    pub policy: IamPolicy,
}
