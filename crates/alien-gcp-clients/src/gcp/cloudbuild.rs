use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::longrunning::Operation;
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

// Service Configuration
#[derive(Debug)]
pub struct CloudBuildServiceConfig;

impl GcpServiceConfig for CloudBuildServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://cloudbuild.googleapis.com/v1"
    }
    fn default_audience(&self) -> &'static str {
        "https://cloudbuild.googleapis.com/"
    }
    fn service_name(&self) -> &'static str {
        "Cloud Build"
    }
    fn service_key(&self) -> &'static str {
        "cloudbuild"
    }
}

// API Trait
#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudBuildApi: Send + Sync + Debug {
    async fn create_build(&self, location: &str, build: Build) -> Result<Operation>;
    async fn get_build(&self, location: &str, build_id: &str) -> Result<Build>;
    async fn cancel_build(&self, location: &str, build_id: &str) -> Result<Build>;
    async fn retry_build(&self, location: &str, build_id: &str) -> Result<Operation>;
}

// Client Implementation
#[derive(Debug)]
pub struct CloudBuildClient {
    base: GcpClientBase,
    project_id: String,
}

impl CloudBuildClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(CloudBuildServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudBuildApi for CloudBuildClient {
    /// See: https://cloud.google.com/build/docs/api/reference/rest/v1/projects.locations.builds/create
    async fn create_build(&self, location: &str, build: Build) -> Result<Operation> {
        let path = format!("projects/{}/locations/{}/builds", self.project_id, location);
        self.base
            .execute_request(Method::POST, &path, None, Some(build), "build")
            .await
    }

    /// See: https://cloud.google.com/build/docs/api/reference/rest/v1/projects.locations.builds/get
    async fn get_build(&self, location: &str, build_id: &str) -> Result<Build> {
        let path = format!(
            "projects/{}/locations/{}/builds/{}",
            self.project_id, location, build_id
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, build_id)
            .await
    }

    /// See: https://cloud.google.com/build/docs/api/reference/rest/v1/projects.locations.builds/cancel
    async fn cancel_build(&self, location: &str, build_id: &str) -> Result<Build> {
        let path = format!(
            "projects/{}/locations/{}/builds/{}:cancel",
            self.project_id, location, build_id
        );
        let request: serde_json::Value = serde_json::json!({});
        self.base
            .execute_request(Method::POST, &path, None, Some(request), build_id)
            .await
    }

    /// See: https://cloud.google.com/build/docs/api/reference/rest/v1/projects.locations.builds/retry
    async fn retry_build(&self, location: &str, build_id: &str) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/builds/{}:retry",
            self.project_id, location, build_id
        );
        let request: serde_json::Value = serde_json::json!({});
        self.base
            .execute_request(Method::POST, &path, None, Some(request), build_id)
            .await
    }
}

// Data Structures
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<BuildStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[builder(default)]
    #[serde(default)]
    pub steps: Vec<BuildStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Results>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_ttl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Artifacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs_bucket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_provenance: Option<SourceProvenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_trigger_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<BuildOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substitutions: Option<HashMap<String, String>>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<Secret>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<HashMap<String, TimeSpan>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BuildStatus {
    StatusUnknown,
    Queued,
    Working,
    Success,
    Failure,
    InternalError,
    Timeout,
    Cancelled,
    Expired,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder, Default)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_source: Option<StorageSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_source: Option<RepoSource>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct StorageSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RepoSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invert_regex: Option<bool>,

    // Revision oneof
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BuildStep {
    pub name: String,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wait_for: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<BuildStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_failure: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_exit_codes: Vec<i32>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<Volume>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automap_substitutions: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BuildOptions {
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_provenance_hash: Vec<HashType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_verify_option: Option<VerifyOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_type: Option<MachineType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_size_gb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub substitution_option: Option<SubstitutionOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_substitutions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automap_substitutions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_streaming_option: Option<LogStreamingOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingMode>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_env: Vec<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<Volume>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HashType {
    None,
    Sha256,
    Md5,
    Sha512,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerifyOption {
    NotVerified,
    Verified,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubstitutionOption {
    MustMatch,
    AllowLoose,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LogStreamingOption {
    StreamDefault,
    StreamOn,
    StreamOff,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoggingMode {
    LoggingUnspecified,
    Legacy,
    GcsOnly,
    CloudLoggingOnly,
    None,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum MachineType {
    #[serde(rename = "UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "E2_MEDIUM")]
    E2Medium,
    #[serde(rename = "E2_HIGHCPU_8")]
    E2Highcpu8,
    #[serde(rename = "E2_HIGHCPU_32")]
    E2Highcpu32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<BuiltImage>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_step_images: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_manifest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_artifacts: Option<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_step_outputs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_timing: Option<TimeSpan>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BuiltImage {
    pub name: String,
    pub digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_timing: Option<TimeSpan>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TimeSpan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Artifacts {
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objects: Option<ArtifactObjects>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactObjects {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<TimeSpan>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SourceProvenance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_storage_source: Option<StorageSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_repo_source: Option<RepoSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hashes: Option<HashMap<String, FileHashes>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FileHashes {
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_hash: Vec<Hash>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Hash {
    #[serde(rename = "type")]
    pub type_: Option<HashType>,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_env: Option<HashMap<String, String>>,
}
