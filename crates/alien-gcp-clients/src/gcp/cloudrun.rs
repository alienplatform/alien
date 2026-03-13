use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::iam::IamPolicy;
use crate::gcp::longrunning::{Operation, OperationResult, Status};
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::Result;
use alien_error::AlienError;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// Cloud Run service configuration
#[derive(Debug)]
pub struct CloudRunServiceConfig;

impl GcpServiceConfig for CloudRunServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://run.googleapis.com/v2"
    }

    fn default_audience(&self) -> &'static str {
        "https://run.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Cloud Run"
    }

    fn service_key(&self) -> &'static str {
        "cloudrun"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CloudRunApi: Send + Sync + Debug {
    async fn create_service(
        &self,
        location: String,
        service_id: String,
        service: Service,
        validate_only: Option<bool>,
    ) -> Result<Operation>;

    async fn delete_service(
        &self,
        location: String,
        service_name: String,
        validate_only: Option<bool>,
        etag: Option<String>,
    ) -> Result<Operation>;

    async fn get_service(&self, location: String, service_name: String) -> Result<Service>;

    async fn patch_service(
        &self,
        location: String,
        service_name: String,
        service: Service,
        update_mask: Option<String>,
        validate_only: Option<bool>,
        allow_missing: Option<bool>,
    ) -> Result<Operation>;

    async fn get_service_iam_policy(
        &self,
        location: String,
        service_name: String,
    ) -> Result<IamPolicy>;

    async fn set_service_iam_policy(
        &self,
        location: String,
        service_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;

    async fn get_operation(&self, location: String, operation_name: String) -> Result<Operation>;

    async fn get_service_revision(
        &self,
        location: String,
        service_name: String,
        revision_name: String,
    ) -> Result<Revision>;
}

/// Cloud Run client for managing services and operations
#[derive(Debug)]
pub struct CloudRunClient {
    base: GcpClientBase,
    project_id: String,
}

impl CloudRunClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(CloudRunServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CloudRunApi for CloudRunClient {
    /// Creates a new Cloud Run service.
    /// See: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services/create
    async fn create_service(
        &self,
        location: String,
        service_id: String,
        service: Service,
        validate_only: Option<bool>,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/services",
            self.project_id, location
        );
        let mut query_params = vec![("serviceId", service_id.to_string())];

        if let Some(validate) = validate_only {
            query_params.push(("validateOnly", validate.to_string()));
        }

        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params),
                Some(service),
                &service_id,
            )
            .await
    }

    /// Deletes a Cloud Run service.
    /// See: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services/delete
    async fn delete_service(
        &self,
        location: String,
        service_name: String,
        validate_only: Option<bool>,
        etag: Option<String>,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/services/{}",
            self.project_id, location, service_name
        );
        let mut query_params = Vec::new();

        if let Some(validate) = validate_only {
            query_params.push(("validateOnly", validate.to_string()));
        }
        if let Some(etag_val) = etag {
            query_params.push(("etag", etag_val.to_string()));
        }

        self.base
            .execute_request(
                Method::DELETE,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Option::<()>::None,
                &service_name,
            )
            .await
    }

    /// Gets information about a Cloud Run service.
    /// See: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services/get
    async fn get_service(&self, location: String, service_name: String) -> Result<Service> {
        let path = format!(
            "projects/{}/locations/{}/services/{}",
            self.project_id, location, service_name
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &service_name)
            .await
    }

    /// Updates a Cloud Run service.
    /// See: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services/patch
    async fn patch_service(
        &self,
        location: String,
        service_name: String,
        service: Service,
        update_mask: Option<String>,
        validate_only: Option<bool>,
        allow_missing: Option<bool>,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/services/{}",
            self.project_id, location, service_name
        );
        let mut query_params = Vec::new();

        if let Some(mask) = update_mask {
            query_params.push(("updateMask", mask.to_string()));
        }
        if let Some(validate) = validate_only {
            query_params.push(("validateOnly", validate.to_string()));
        }
        if let Some(allow) = allow_missing {
            query_params.push(("allowMissing", allow.to_string()));
        }

        self.base
            .execute_request(
                Method::PATCH,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Some(service),
                &service_name,
            )
            .await
    }

    /// Gets the IAM policy for a Cloud Run service.
    /// See: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services/getIamPolicy
    async fn get_service_iam_policy(
        &self,
        location: String,
        service_name: String,
    ) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/locations/{}/services/{}:getIamPolicy",
            self.project_id, location, service_name
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &service_name)
            .await
    }

    /// Sets the IAM policy for a Cloud Run service.
    /// See: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services/setIamPolicy
    async fn set_service_iam_policy(
        &self,
        location: String,
        service_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/locations/{}/services/{}:setIamPolicy",
            self.project_id, location, service_name
        );
        let request = SetIamPolicyRequest { policy: iam_policy };

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &service_name)
            .await
    }

    /// Gets information about a long-running operation.
    /// See: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.operations/get
    async fn get_operation(&self, location: String, operation_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/operations/{}",
            self.project_id, location, operation_name
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

    /// Gets information about a service revision.
    /// See: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services.revisions/get
    async fn get_service_revision(
        &self,
        location: String,
        service_name: String,
        revision_name: String,
    ) -> Result<Revision> {
        let path = format!(
            "projects/{}/locations/{}/services/{}/revisions/{}",
            self.project_id, location, &service_name, revision_name
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &revision_name)
            .await
    }
}

// --- Data Structures ---

/// Request message for setting IAM policy.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SetIamPolicyRequest {
    /// The policy to be applied.
    pub policy: IamPolicy,
}

/// Represents a Cloud Run Service.
/// Based on: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services#Service
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    /// The fully qualified name of this Service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// User-provided description of the Service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Output only. Server assigned unique identifier for the trigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,

    /// Output only. A number that monotonically increases every time the user modifies the desired state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,

    /// Map of string keys and values that can be used to organize and categorize objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Map of string keys and values that can be used to organize and categorize objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,

    /// Output only. The creation time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// Output only. The last-modified time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,

    /// Output only. The deletion time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_time: Option<String>,

    /// Output only. For a deleted resource, the time after which it will be permamently deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,

    /// Output only. Email address of the authenticated creator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,

    /// Output only. Email address of the last authenticated modifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modifier: Option<String>,

    /// Arbitrary identifier for the API client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,

    /// Arbitrary version identifier for the API client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,

    /// Provides the ingress settings for this Service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress: Option<Ingress>,

    /// The launch stage as defined by Google Cloud Platform Launch Stages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_stage: Option<LaunchStage>,

    /// Settings for the Binary Authorization feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_authorization: Option<BinaryAuthorization>,

    /// Required. The template used to create revisions for this Service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<RevisionTemplate>,

    /// Specifies how to distribute traffic over a collection of revisions.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traffic: Vec<TrafficTarget>,

    /// Scaling settings for this Service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling: Option<ServiceScaling>,

    /// Optional. Disables IAM permission checks for callers within the project to reach this service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoker_iam_disabled: Option<bool>,

    /// Output only. The Condition of this Service, containing its readiness status.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,

    /// Output only. The generation of this Service currently serving traffic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<String>,

    /// Output only. Returns the generation last seen by the system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_condition: Option<Condition>,

    /// Output only. All URLs serving traffic for this Service.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,

    /// Output only. Name of the last created revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_ready_revision: Option<String>,

    /// Output only. Name of the latest revision that is serving traffic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_created_revision: Option<String>,

    /// Output only. Detailed status information for corresponding traffic targets.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traffic_statuses: Vec<TrafficTargetStatus>,

    /// Output only. The URI of this Service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    /// Output only. Returns true if the Service is currently being acted upon by the system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconciling: Option<bool>,

    /// Output only. A system-generated fingerprint for this version of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

/// Represents a Revision.
/// Based on: https://cloud.google.com/run/docs/reference/rest/v2/projects.locations.services.revisions#Revision
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    /// Output only. The unique name of this Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Output only. Server assigned unique identifier for the Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,

    /// Output only. A number that monotonically increases every time the user modifies the desired state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,

    /// Map of string keys and values that can be used to organize and categorize objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Map of string keys and values that can be used to organize and categorize objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,

    /// Output only. The creation time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// Output only. The last-modified time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,

    /// Output only. For a deleted resource, the time after which it will be permamently deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_time: Option<String>,

    /// Output only. For a deleted resource, the time after which it will be permamently deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,

    /// The launch stage as defined by Google Cloud Platform Launch Stages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_stage: Option<LaunchStage>,

    /// The name of the parent service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,

    /// Scaling settings for this revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling: Option<RevisionScaling>,

    /// VPC Access configuration for this Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_access: Option<VpcAccess>,

    /// Sets the maximum number of requests that each serving instance can receive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_request_concurrency: Option<i32>,

    /// Max allowed time for an instance to respond to a request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,

    /// Email address of the IAM service account associated with the revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account: Option<String>,

    /// Holds the containers that define the unit of execution for this Revision.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<Container>,

    /// A list of Volumes to make available to containers.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<Volume>,

    /// The execution environment being used to host this Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_environment: Option<ExecutionEnvironment>,

    /// A reference to a customer managed encryption key (CMEK) to use to encrypt this container image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,

    /// Enables service mesh connectivity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_mesh: Option<ServiceMesh>,

    /// The action to take if the encryption key is revoked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_key_revocation_action: Option<EncryptionKeyRevocationAction>,

    /// If encryptionKeyRevocationAction is SHUTDOWN, the duration before shutting down all instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_key_shutdown_duration: Option<String>,

    /// Output only. Indicates whether the resource's reconciliation is still in progress.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconciling: Option<bool>,

    /// Output only. The Condition of this Revision, containing its readiness status.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,

    /// Output only. The generation of this Revision currently serving traffic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<String>,

    /// Output only. The Google Console URI to obtain logs for the Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_uri: Option<String>,

    /// Output only. Reserved for future use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub satisfies_pzs: Option<bool>,

    /// Enable session affinity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_affinity: Option<bool>,

    /// Output only. The current effective scaling settings for the revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling_status: Option<RevisionScalingStatus>,

    /// The node selector for the revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_selector: Option<NodeSelector>,

    /// Output only. Email address of the authenticated creator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,

    /// Output only. A system-generated fingerprint for this version of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,

    /// Optional. Output only. True if GPU zonal redundancy is disabled on this revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_zonal_redundancy_disabled: Option<bool>,
}

/// Effective settings for the current revision
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RevisionScalingStatus {
    /// The current number of min instances provisioned for this revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_min_instance_count: Option<i32>,
}

/// Settings for service mesh connectivity.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ServiceMesh {
    /// Enables service mesh connectivity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh: Option<String>,
}

/// The action to take if the encryption key is revoked.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EncryptionKeyRevocationAction {
    /// Unspecified action
    EncryptionKeyRevocationActionUnspecified,
    /// Shut down the instance
    Shutdown,
    /// Prevent new instances from starting
    PreventNew,
}

/// The node selector for the revision.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NodeSelector {
    /// Required. GPU accelerator type to attach to an instance.
    pub accelerator: String,
}

/// Defines the ingress settings for the Service.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Ingress {
    /// Unspecified
    IngressTrafficUnspecified,
    /// All inbound traffic is allowed.
    IngressTrafficAll,
    /// Only internal traffic is allowed.
    IngressTrafficInternal,
    /// Only internal and Cloud Load Balancing traffic is allowed.
    IngressTrafficInternalLoadBalancer,
}

/// The various stages of a Cloud Run launch.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LaunchStage {
    /// Do not use this default value.
    LaunchStageUnspecified,
    /// Early Access features are limited to a closed group of testers.
    EarlyAccess,
    /// Alpha is a limited availability test for releases before they are cleared for widespread use.
    Alpha,
    /// Beta is the testing stage preceding General Availability.
    Beta,
    /// General Availability (GA) features are open to all developers and are considered stable.
    Ga,
    /// Deprecated features are scheduled for removal.
    Deprecated,
}

/// Settings for Binary Authorization.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BinaryAuthorization {
    /// Optional. If present, indicates to use Breakglass using this justification.
    /// If useDefault is False, then it must be empty.
    /// For more information on breakglass, see https://cloud.google.com/binary-authorization/docs/using-breakglass
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakglass_justification: Option<String>,

    /// Union field binauthz_method. binauthz_method can be only one of the following:
    #[serde(flatten)]
    pub binauthz_method: Option<BinaryAuthorizationMethod>,
}

/// Union field representing the binary authorization method.
/// Only one of the variants can be specified.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum BinaryAuthorizationMethod {
    /// Optional. If True, indicates to use the default project's binary authorization policy.
    /// If False, binary authorization will be disabled.
    UseDefault(bool),

    /// Optional. The path to a binary authorization policy.
    /// Format: projects/{project}/platforms/cloudRun/{policy-name}
    Policy(String),
}

/// RevisionTemplate describes the data a revision should have when created from a template.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RevisionTemplate {
    /// The unique name for the revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,

    /// Map of string keys and values that can be used to organize and categorize objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Map of string keys and values that can be used to organize and categorize objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,

    /// Scaling settings for this Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling: Option<RevisionScaling>,

    /// VPC Access configuration to use for this Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_access: Option<VpcAccess>,

    /// Max allowed time for an instance to respond to a request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,

    /// Email address of the IAM service account associated with the revision of the service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account: Option<String>,

    /// Holds the containers that define the unit of execution for this Revision.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<Container>,

    /// A list of Volumes to make available to containers.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<Volume>,

    /// The sandbox environment to host this Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_environment: Option<ExecutionEnvironment>,

    /// A reference to a customer managed encryption key (CMEK) to use to encrypt this container image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,

    /// The maximum number of requests that can be made to this container instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_request_concurrency: Option<i32>,
}

/// Holds a single traffic routing entry for the Service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TrafficTarget {
    /// The allocation type for this traffic target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<TrafficTargetAllocationType>,

    /// Revision to which to send this portion of traffic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,

    /// Specifies percent of the traffic to this Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<i32>,

    /// Indicates a string to be part of the URI to exclusively reference this target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// The allocation type for a traffic target.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TrafficTargetAllocationType {
    /// Unspecified type.
    TrafficTargetAllocationTypeUnspecified,
    /// Allocates a revision based on the revision-name.
    TrafficTargetAllocationTypeRevision,
    /// Allocates a revision based on the latest ready revision.
    TrafficTargetAllocationTypeLatest,
}

/// Represents the observed state of a single TrafficTarget entry.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TrafficTargetStatus {
    /// The allocation type for this traffic target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<TrafficTargetAllocationType>,

    /// Revision to which this traffic is sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,

    /// Percent specifies percent of the traffic to this Revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<i32>,

    /// Tag assigned to this traffic allocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,

    /// Displays the target URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

/// Service scaling configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ServiceScaling {
    /// Optional. total min instances for the service. This number of instances is divided among all revisions with specified traffic based on the percent of traffic they are receiving.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_instance_count: Option<i32>,

    /// Optional. The scaling mode for the service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling_mode: Option<ScalingMode>,

    /// Optional. total max instances for the service. This number of instances is divided among all revisions with specified traffic based on the percent of traffic they are receiving.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_count: Option<i32>,

    /// Optional. total instance count for the service in manual scaling mode. This number of instances is divided among all revisions with specified traffic based on the percent of traffic they are receiving.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_instance_count: Option<i32>,
}

/// The scaling mode for the service. If not provided, it defaults to AUTOMATIC.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScalingMode {
    /// Unspecified.
    ScalingModeUnspecified,
    /// Scale based on traffic between min and max instances.
    Automatic,
    /// Scale to exactly min instances and ignore max instances.
    Manual,
}

/// Defines a status condition for a resource.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// type is used to communicate the status of the reconciliation process.
    /// See also: https://github.com/knative/serving/blob/main/docs/spec/errors.md#error-conditions-and-reporting
    /// Types common to all resources include: * "Ready": True when the Resource is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// State of the condition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ConditionState>,

    /// Human readable message indicating details about the current status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Last time the condition transitioned from one status to another.
    /// Uses RFC 3339, where generated output will always be Z-normalized and uses 0, 3, 6 or 9 fractional digits.
    /// Examples: "2014-10-02T15:01:23Z", "2014-10-02T15:01:23.045123456Z" or "2014-10-02T15:01:23+05:30".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_transition_time: Option<String>,

    /// How to interpret failures of this condition, one of Error, Warning, Info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<ConditionSeverity>,

    /// The reason for this condition. Depending on the condition type, it will populate one of these fields.
    /// Successful conditions cannot have a reason.
    #[serde(flatten)]
    pub reason: Option<ConditionReason>,
}

/// Union field for condition reasons
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ConditionReason {
    /// Output only. A common (service-level) reason for this condition.
    Common { reason: CommonReason },
    /// Output only. A reason for the revision condition.
    Revision { revision_reason: RevisionReason },
    /// Output only. A reason for the execution condition.
    Execution { execution_reason: ExecutionReason },
}

/// Represents the possible Condition states.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConditionState {
    /// The default value. This value is used if the state is omitted.
    StateUnspecified,
    /// Transient state: Reconciliation has not started yet.
    ConditionPending,
    /// Transient state: reconciliation is still in progress.
    ConditionReconciling,
    /// Terminal state: Reconciliation did not succeed.
    ConditionFailed,
    /// Terminal state: Reconciliation completed successfully.
    ConditionSucceeded,
}

/// Represents the severity of the condition failures.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConditionSeverity {
    /// Unspecified severity
    SeverityUnspecified,
    /// Error severity.
    Error,
    /// Warning severity.
    Warning,
    /// Info severity.
    Info,
}

/// Reasons common to all types of conditions.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommonReason {
    /// Default value.
    CommonReasonUndefined,
    /// Reason unknown. Further details will be in message.
    Unknown,
    /// Revision creation process failed.
    RevisionFailed,
    /// Timed out waiting for completion.
    ProgressDeadlineExceeded,
    /// The container image path is incorrect.
    ContainerMissing,
    /// Insufficient permissions on the container image.
    ContainerPermissionDenied,
    /// Container image is not authorized by policy.
    ContainerImageUnauthorized,
    /// Container image policy authorization check failed.
    ContainerImageAuthorizationCheckFailed,
    /// Insufficient permissions on encryption key.
    EncryptionKeyPermissionDenied,
    /// Permission check on encryption key failed.
    EncryptionKeyCheckFailed,
    /// At least one Access check on secrets failed.
    SecretsAccessCheckFailed,
    /// Waiting for operation to complete.
    WaitingForOperation,
    /// System will retry immediately.
    ImmediateRetry,
    /// System will retry later; current attempt failed.
    PostponedRetry,
    /// An internal error occurred. Further information may be in the message.
    Internal,
    /// User-provided VPC network was not found.
    VpcNetworkNotFound,
}

/// Reasons specific to Revision resource.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RevisionReason {
    /// Default value.
    RevisionReasonUndefined,
    /// Revision in Pending state.
    Pending,
    /// Revision is in Reserve state.
    Reserve,
    /// Revision is Retired.
    Retired,
    /// Revision is being retired.
    Retiring,
    /// Revision is being recreated.
    Recreating,
    /// There was a health check error.
    HealthCheckContainerError,
    /// Health check failed due to user error from customized path of the container. System will retry.
    CustomizedPathResponsePending,
    /// A revision with minInstanceCount > 0 was created and is reserved, but it was not configured to serve traffic, so it's not live.
    /// This can also happen momentarily during traffic migration.
    MinInstancesNotProvisioned,
    /// The maximum allowed number of active revisions has been reached.
    ActiveRevisionLimitReached,
    /// There was no deployment defined. This value is no longer used, but Services created in older versions of the API might contain this value.
    NoDeployment,
    /// A revision's container has no port specified since the revision is of a manually scaled service with 0 instance count
    HealthCheckSkipped,
    /// A revision with minInstanceCount > 0 was created and is waiting for enough instances to begin a traffic migration.
    MinInstancesWarming,
}

/// Reasons specific to Execution resource.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionReason {
    /// Default value.
    ExecutionReasonUndefined,
    /// Internal system error getting execution status. System will retry.
    JobStatusServicePollingError,
    /// A task reached its retry limit and the last attempt failed due to the user container exiting with a non-zero exit code.
    NonZeroExitCode,
    /// The execution was cancelled by users.
    Cancelled,
    /// The execution is in the process of being cancelled.
    Cancelling,
    /// The execution was deleted.
    Deleted,
}

/// Settings for revision-level scaling settings.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RevisionScaling {
    /// Minimum number of serving instances that this resource should have.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_instance_count: Option<i32>,

    /// Maximum number of serving instances that this resource should have.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_count: Option<i32>,
}

/// VPC Access settings.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct VpcAccess {
    /// VPC Access connector name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector: Option<String>,

    /// Traffic VPC egress settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub egress: Option<VpcEgress>,

    /// Direct VPC egress settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_interfaces: Option<Vec<NetworkInterface>>,
}

/// VPC egress settings.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VpcEgress {
    /// Unspecified
    VpcEgressUnspecified,
    /// All outbound traffic is routed through the VPC connector.
    AllTraffic,
    /// Only private IP ranges are routed through the VPC connector.
    PrivateRangesOnly,
}

/// Direct VPC egress settings.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    /// The VPC network that the Cloud Run resource will be able to send traffic to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// The VPC subnetwork that the Cloud Run resource will get IPs from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// Network tags applied to this Cloud Run resource.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// A single application container.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    /// Name of the container specified as a DNS_LABEL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// URL of the Container image in Google Container Registry or Google Artifact Registry.
    pub image: String,

    /// Entrypoint array.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,

    /// Arguments to the entrypoint.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// List of environment variables to set in the container.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,

    /// Compute Resource requirements by this container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,

    /// List of ports to expose from the container.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ContainerPort>,

    /// Volume to mount into the container's filesystem.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volume_mounts: Vec<VolumeMount>,

    /// Container's working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,

    /// Periodic probe of container liveness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveness_probe: Option<Probe>,

    /// Periodic probe of container service readiness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_probe: Option<Probe>,

    /// Container dependencies.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,

    /// Output only. URI of the base container image that was used to build this container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_image_uri: Option<String>,

    /// Output only. Build information of the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_info: Option<BuildInfo>,
}

/// EnvVar represents an environment variable present in a Container.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    /// Name of the environment variable.
    pub name: String,

    /// Variable references $(VAR_NAME) are expanded using the previous defined environment variables in the container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    /// Source for the environment variable's value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_source: Option<EnvVarSource>,
}

/// EnvVarSource represents a source for the value of an EnvVar.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct EnvVarSource {
    /// Selects a key of a secret in the pod's namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key_ref: Option<SecretKeySelector>,
}

/// SecretKeySelector selects a key of a Secret.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SecretKeySelector {
    /// The name of the secret in the pod's namespace to select from.
    pub secret: String,

    /// The key of the secret to select from.
    pub key: String,

    /// Specify whether the Secret or its key must be defined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// ResourceRequirements describes the compute resource requirements.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRequirements {
    /// Only memory and CPU are supported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<HashMap<String, String>>,

    /// Determines whether CPU should be throttled or not outside of requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_idle: Option<bool>,

    /// Determines whether CPU should be boosted on startup of a new container instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_cpu_boost: Option<bool>,
}

/// ContainerPort represents a network port in a single container.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ContainerPort {
    /// If specified, this must be an IANA_SVC_NAME and unique within the pod.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Number of port to expose on the pod's IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_port: Option<i32>,
}

/// VolumeMount describes a mounting of a Volume within a container.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct VolumeMount {
    /// This must match the Name of a Volume.
    pub name: String,

    /// Path within the container at which the volume should be mounted.
    pub mount_path: String,
}

/// Volume represents a named volume in a container.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// Required. Volume's name.
    pub name: String,

    /// Union field volume_type. volume_type can be only one of the following:
    #[serde(flatten)]
    pub volume_type: VolumeType,
}

/// Union field representing the volume type. Only one volume type can be specified.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum VolumeType {
    /// Secret represents a secret that should populate this volume.
    Secret(SecretVolumeSource),

    /// For Cloud SQL volumes, contains the specific instances that should be mounted.
    CloudSqlInstance(CloudSqlInstance),

    /// Ephemeral storage used to provide a working directory for the container.
    EmptyDir(EmptyDirVolumeSource),

    /// Represents an NFS mount.
    Nfs(NFSVolumeSource),

    /// Represents a volume backed by a Cloud Storage bucket using Cloud Storage FUSE.
    Gcs(GCSVolumeSource),
}

/// The contents of the target Secret's Data field will be presented in a volume as files.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SecretVolumeSource {
    /// The name of the secret in Cloud Secret Manager.
    pub secret: String,

    /// If unspecified, the volume will expose a file with the same name as the secret name.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<VersionToPath>,

    /// Integer representation of mode bits to use on created files by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_mode: Option<i32>,
}

/// VersionToPath maps a specific version of a secret to a relative file to mount to.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct VersionToPath {
    /// The relative path of the secret in the container.
    pub path: String,

    /// The Cloud Secret Manager secret version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Integer representation of mode bits to use on this file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<i32>,
}

/// Represents a Cloud SQL instance.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CloudSqlInstance {
    /// The Cloud SQL instance connection names in the format project:location:instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<String>,
}

/// Represents an empty directory for a pod.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct EmptyDirVolumeSource {
    /// The medium on which the data is stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medium: Option<EmptyDirMedium>,

    /// Limit on the storage of the empty directory volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_limit: Option<String>,
}

/// The medium on which the data is stored.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EmptyDirMedium {
    /// When not specified, falls back to the default implementation which is currently in memory.
    MediumUnspecified,
    /// Explicitly set the EmptyDir to be in memory. Uses tmpfs.
    Memory,
}

/// Represents an NFS volume.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NFSVolumeSource {
    /// Hostname or IP address of the NFS server.
    pub server: String,

    /// Path that is exported by the NFS server.
    pub path: String,

    /// If true, the NFS volume will be mounted as read-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
}

/// Represents a Google Cloud Storage bucket mount.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GCSVolumeSource {
    /// Cloud Storage Bucket name.
    pub bucket: String,

    /// If true, the GCS volume will be mounted as read-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,

    /// A list of additional flags to pass to the gcsfuse CLI.
    /// Options should be specified without the leading "--".
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mount_options: Vec<String>,
}

/// The sandbox environment to host this Revision.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionEnvironment {
    /// Unspecified
    ExecutionEnvironmentUnspecified,
    /// Uses the First Generation execution environment.
    ExecutionEnvironmentGen1,
    /// Uses Second Generation execution environment.
    ExecutionEnvironmentGen2,
}

/// Probe describes a health check to be performed against a container.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    /// Number of seconds after the container has started before the probe is initiated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_delay_seconds: Option<i32>,

    /// Number of seconds after which the probe times out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,

    /// How often (in seconds) to perform the probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_seconds: Option<i32>,

    /// Minimum consecutive failures for the probe to be considered failed after having succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_threshold: Option<i32>,

    /// The action taken to determine the health of a container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_type: Option<ProbeType>,
}

/// The action taken to determine the health of a container.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ProbeType {
    HttpGet { http_get: HttpGetAction },
    TcpSocket { tcp_socket: TcpSocketAction },
    Grpc { grpc: GrpcAction },
}

/// HTTPGetAction describes an action based on HTTP Get requests.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HttpGetAction {
    /// Path to access on the HTTP server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Custom headers to set in the request.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http_headers: Vec<HttpHeader>,

    /// Port number to access on the container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
}

/// HTTPHeader describes a custom header to be used in HTTP probes.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct HttpHeader {
    /// The header field name.
    pub name: String,

    /// The header field value.
    pub value: String,
}

/// TCPSocketAction describes an action based on opening a socket.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TcpSocketAction {
    /// Port number to access on the container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
}

/// GRPCAction describes an action involving a GRPC port.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GrpcAction {
    /// Port number of the gRPC service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,

    /// Service is the name of the service to place in the gRPC HealthCheckRequest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

/// Build information of the image.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    /// Output only. Entry point of the function when the image is a Cloud Run function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_target: Option<String>,

    /// Output only. Source code location of the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_location: Option<String>,
}
