use crate::core::{Binding, Expr, IamPolicy};
use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::GcpClientConfig;
use alien_error::AlienError;
use bon::Builder;
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_iam_v1::model::{Binding as OfficialBinding, Policy as OfficialPolicy};
use google_cloud_longrunning::model::{
    operation::Result as OfficialOperationResult, Operation as OfficialOperation,
};
use google_cloud_run_v2::{
    client::Services as OfficialCloudRunServices, model::Service as OfficialService,
};
use google_cloud_type::model::Expr as OfficialExpr;
use http::StatusCode;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::OnceCell;

#[cfg(any(test, feature = "test-utils"))]
use mockall::automock;

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait CloudRunApi: Send + Sync + std::fmt::Debug {
    async fn create_service(
        &self,
        location: String,
        service_id: String,
        service: Service,
        validate_only: Option<bool>,
    ) -> CloudClientResult<Operation>;

    async fn delete_service(
        &self,
        location: String,
        service_name: String,
        validate_only: Option<bool>,
        etag: Option<String>,
    ) -> CloudClientResult<Operation>;

    async fn get_service(
        &self,
        location: String,
        service_name: String,
    ) -> CloudClientResult<Service>;

    async fn patch_service(
        &self,
        location: String,
        service_name: String,
        service: Service,
        update_mask: Option<String>,
        validate_only: Option<bool>,
        allow_missing: Option<bool>,
    ) -> CloudClientResult<Operation>;

    async fn get_service_iam_policy(
        &self,
        location: String,
        service_name: String,
    ) -> CloudClientResult<IamPolicy>;

    async fn set_service_iam_policy(
        &self,
        location: String,
        service_name: String,
        iam_policy: IamPolicy,
    ) -> CloudClientResult<IamPolicy>;

    async fn get_operation(
        &self,
        location: String,
        operation_name: String,
    ) -> CloudClientResult<Operation>;
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Server-assigned operation name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Service-specific operation metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    /// Whether the operation has completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    /// Operation result when complete.
    #[serde(flatten)]
    pub result: Option<OperationResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum OperationResult {
    Error { error: Status },
    Response { response: serde_json::Value },
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// gRPC-style error code.
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
    /// Additional structured details.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    /// Fully qualified service name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// User-provided service description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Service generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    /// Resource labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
    /// Resource annotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,
    /// Service ingress setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress: Option<Ingress>,
    /// Revision template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<RevisionTemplate>,
    /// Traffic split.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traffic: Vec<TrafficTarget>,
    /// Service-level scaling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling: Option<ServiceScaling>,
    /// Whether invoker IAM checks are disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoker_iam_disabled: Option<bool>,
    /// Readiness conditions.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    /// Observed generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<String>,
    /// Latest ready revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_ready_revision: Option<String>,
    /// Latest created revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_created_revision: Option<String>,
    /// Service URLs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
    /// Primary URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Whether reconciliation is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconciling: Option<bool>,
    /// Service etag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Ingress {
    IngressTrafficUnspecified,
    IngressTrafficAll,
    IngressTrafficInternal,
    IngressTrafficInternalLoadBalancer,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RevisionTemplate {
    /// Revision labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
    /// Revision annotations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,
    /// Revision scaling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaling: Option<RevisionScaling>,
    /// VPC access settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_access: Option<VpcAccess>,
    /// Timeout duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    /// Service account email.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account: Option<String>,
    /// Containers.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<Container>,
    /// Execution environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_environment: Option<ExecutionEnvironment>,
    /// Max concurrent requests per instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_request_concurrency: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RevisionScaling {
    /// Minimum serving instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_instance_count: Option<i32>,
    /// Maximum serving instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_count: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionEnvironment {
    ExecutionEnvironmentUnspecified,
    ExecutionEnvironmentGen1,
    ExecutionEnvironmentGen2,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct VpcAccess {
    /// Connector name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector: Option<String>,
    /// Egress mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub egress: Option<VpcEgress>,
    /// Direct VPC network interfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_interfaces: Option<Vec<NetworkInterface>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VpcEgress {
    VpcEgressUnspecified,
    AllTraffic,
    PrivateRangesOnly,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    /// VPC network name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// VPC subnetwork name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,
    /// Network tags.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    /// Container name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Container image.
    pub image: String,
    /// Environment variables.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvVar>,
    /// Resource requirements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirements>,
    /// Exposed ports.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ContainerPort>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    /// Environment variable name.
    pub name: String,
    /// Literal value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Value source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_source: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRequirements {
    /// CPU and memory limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<HashMap<String, String>>,
    /// Whether CPU is idle-throttled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_idle: Option<bool>,
    /// Whether startup CPU boost is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_cpu_boost: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ContainerPort {
    /// Port name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Container port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_port: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TrafficTarget {
    /// Traffic allocation type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<TrafficTargetAllocationType>,
    /// Revision name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Percent of traffic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<i32>,
    /// Optional tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TrafficTargetAllocationType {
    TrafficTargetAllocationTypeUnspecified,
    TrafficTargetAllocationTypeRevision,
    TrafficTargetAllocationTypeLatest,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ServiceScaling {
    /// Minimum total instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_instance_count: Option<i32>,
    /// Maximum total instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instance_count: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// Condition type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Condition state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ConditionState>,
    /// Condition message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Condition reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConditionState {
    StateUnspecified,
    ConditionPending,
    ConditionReconciling,
    ConditionFailed,
    ConditionSucceeded,
}

pub struct OfficialGcpCloudRunClient {
    config: GcpClientConfig,
    services: OnceCell<OfficialCloudRunServices>,
}

impl std::fmt::Debug for OfficialGcpCloudRunClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGcpCloudRunClient")
            .field("project_id", &self.config.project_id)
            .field("region", &self.config.region)
            .finish_non_exhaustive()
    }
}

impl OfficialGcpCloudRunClient {
    pub fn new(config: GcpClientConfig) -> Self {
        Self {
            config,
            services: OnceCell::new(),
        }
    }

    async fn services(&self) -> CloudClientResult<&OfficialCloudRunServices> {
        self.services
            .get_or_try_init(|| async { cloud_run_services_from_alien_config(&self.config).await })
            .await
    }

    fn service_resource_name(&self, location: &str, service_name: &str) -> String {
        format!(
            "projects/{}/locations/{}/services/{}",
            self.config.project_id, location, service_name
        )
    }

    fn location_resource_name(&self, location: &str) -> String {
        format!("projects/{}/locations/{location}", self.config.project_id)
    }
}

#[async_trait::async_trait]
impl CloudRunApi for OfficialGcpCloudRunClient {
    async fn create_service(
        &self,
        location: String,
        service_id: String,
        service: Service,
        validate_only: Option<bool>,
    ) -> CloudClientResult<Operation> {
        let mut request = self
            .services()
            .await?
            .create_service()
            .set_parent(self.location_resource_name(&location))
            .set_service_id(service_id.clone())
            .set_service(to_official::<_, OfficialService>(service)?);

        if let Some(validate_only) = validate_only {
            request = request.set_validate_only(validate_only);
        }

        request
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Service", &service_id))
            .map(operation_from_official)
    }

    async fn delete_service(
        &self,
        location: String,
        service_name: String,
        validate_only: Option<bool>,
        etag: Option<String>,
    ) -> CloudClientResult<Operation> {
        let mut request = self
            .services()
            .await?
            .delete_service()
            .set_name(self.service_resource_name(&location, &service_name));

        if let Some(validate_only) = validate_only {
            request = request.set_validate_only(validate_only);
        }
        if let Some(etag) = etag {
            request = request.set_etag(etag);
        }

        request
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Service", &service_name))
            .map(operation_from_official)
    }

    async fn get_service(
        &self,
        location: String,
        service_name: String,
    ) -> CloudClientResult<Service> {
        self.services()
            .await?
            .get_service()
            .set_name(self.service_resource_name(&location, &service_name))
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Service", &service_name))
            .and_then(from_official)
    }

    async fn patch_service(
        &self,
        location: String,
        service_name: String,
        mut service: Service,
        update_mask: Option<String>,
        validate_only: Option<bool>,
        allow_missing: Option<bool>,
    ) -> CloudClientResult<Operation> {
        if service.name.is_none() {
            service.name = Some(self.service_resource_name(&location, &service_name));
        }

        let mut request = self
            .services()
            .await?
            .update_service()
            .set_service(to_official::<_, OfficialService>(service)?);

        if let Some(update_mask) = update_mask {
            request = request.set_update_mask(field_mask_from_comma_separated(update_mask));
        }
        if let Some(validate_only) = validate_only {
            request = request.set_validate_only(validate_only);
        }
        if let Some(allow_missing) = allow_missing {
            request = request.set_allow_missing(allow_missing);
        }

        request
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Service", &service_name))
            .map(operation_from_official)
    }

    async fn get_service_iam_policy(
        &self,
        location: String,
        service_name: String,
    ) -> CloudClientResult<IamPolicy> {
        self.services()
            .await?
            .get_iam_policy()
            .set_resource(self.service_resource_name(&location, &service_name))
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Service", &service_name))
            .map(iam_policy_from_official)
    }

    async fn set_service_iam_policy(
        &self,
        location: String,
        service_name: String,
        iam_policy: IamPolicy,
    ) -> CloudClientResult<IamPolicy> {
        let request = google_cloud_iam_v1::model::SetIamPolicyRequest::new()
            .set_resource(self.service_resource_name(&location, &service_name))
            .set_policy(iam_policy_to_official(iam_policy)?);

        self.services()
            .await?
            .set_iam_policy()
            .with_request(request)
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Service", &service_name))
            .map(iam_policy_from_official)
    }

    async fn get_operation(
        &self,
        location: String,
        operation_name: String,
    ) -> CloudClientResult<Operation> {
        let name = if operation_name.contains('/') {
            operation_name.clone()
        } else {
            format!(
                "projects/{}/locations/{}/operations/{}",
                self.config.project_id, location, operation_name
            )
        };

        self.services()
            .await?
            .get_operation()
            .set_name(name)
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Operation", &operation_name))
            .map(operation_from_official)
    }
}

async fn cloud_run_services_from_alien_config(
    config: &GcpClientConfig,
) -> CloudClientResult<OfficialCloudRunServices> {
    let credentials = crate::core::gcp_credentials_from_alien_config(config).map_err(|error| {
        AlienError::new(CloudClientErrorData::AuthenticationError {
            message: error.to_string(),
        })
    })?;
    let mut builder = OfficialCloudRunServices::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("cloudrun"))
    {
        builder = builder.with_endpoint(endpoint.trim_end_matches('/').to_string());
    }

    builder.build().await.map_err(|error| {
        AlienError::new(CloudClientErrorData::GenericError {
            message: format!("Failed to build official GCP Cloud Run client: {error}"),
        })
    })
}

fn to_official<T, U>(value: T) -> CloudClientResult<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_json::to_value(value).map_err(conversion_error)?;
    serde_json::from_value(value).map_err(conversion_error)
}

fn from_official<T, U>(value: T) -> CloudClientResult<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_json::to_value(value).map_err(conversion_error)?;
    serde_json::from_value(value).map_err(conversion_error)
}

fn conversion_error(error: serde_json::Error) -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::GenericError {
        message: format!("Failed to convert GCP Cloud Run model: {error}"),
    })
}

fn operation_from_official(operation: OfficialOperation) -> Operation {
    let result = operation.result.map(|result| match result {
        OfficialOperationResult::Error(error) => OperationResult::Error {
            error: Status {
                code: error.code,
                message: error.message,
                details: error.details.iter().map(any_to_json).collect(),
            },
        },
        OfficialOperationResult::Response(response) => OperationResult::Response {
            response: any_to_json(&response),
        },
        _ => OperationResult::Response {
            response: serde_json::Value::Null,
        },
    });

    Operation {
        name: if operation.name.is_empty() {
            None
        } else {
            Some(operation.name)
        },
        metadata: operation.metadata.as_ref().map(any_to_json),
        done: Some(operation.done),
        result,
    }
}

fn any_to_json(any: &wkt::Any) -> serde_json::Value {
    serde_json::to_value(any).unwrap_or_else(|_| {
        serde_json::json!({
            "typeUrl": any.type_url(),
        })
    })
}

fn iam_policy_to_official(policy: IamPolicy) -> CloudClientResult<OfficialPolicy> {
    use base64::Engine;

    let etag = policy
        .etag
        .as_deref()
        .map(|etag| {
            base64::engine::general_purpose::STANDARD
                .decode(etag)
                .map_err(|error| {
                    AlienError::new(CloudClientErrorData::GenericError {
                        message: format!("Failed to base64-decode GCP IAM policy etag: {error}"),
                    })
                })
        })
        .transpose()?
        .unwrap_or_default();

    Ok(OfficialPolicy::new()
        .set_version(policy.version.unwrap_or_default())
        .set_bindings(policy.bindings.into_iter().map(binding_to_official))
        .set_etag(etag))
}

fn iam_policy_from_official(policy: OfficialPolicy) -> IamPolicy {
    use base64::Engine;

    IamPolicy {
        version: Some(policy.version),
        bindings: policy
            .bindings
            .into_iter()
            .map(binding_from_official)
            .collect(),
        etag: if policy.etag.is_empty() {
            None
        } else {
            Some(base64::engine::general_purpose::STANDARD.encode(policy.etag))
        },
        kind: None,
        resource_id: None,
    }
}

fn binding_to_official(binding: Binding) -> OfficialBinding {
    let mut official_binding = OfficialBinding::new()
        .set_role(binding.role)
        .set_members(binding.members);

    if let Some(condition) = binding.condition {
        official_binding = official_binding.set_condition(
            OfficialExpr::new()
                .set_expression(condition.expression)
                .set_title(condition.title.unwrap_or_default())
                .set_description(condition.description.unwrap_or_default())
                .set_location(condition.location.unwrap_or_default()),
        );
    }

    official_binding
}

fn binding_from_official(binding: OfficialBinding) -> Binding {
    Binding {
        role: binding.role,
        members: binding.members,
        condition: binding.condition.map(|condition| Expr {
            expression: condition.expression,
            title: empty_string_to_none(condition.title),
            description: empty_string_to_none(condition.description),
            location: empty_string_to_none(condition.location),
        }),
    }
}

fn empty_string_to_none(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn field_mask_from_comma_separated(update_mask: String) -> wkt::FieldMask {
    wkt::FieldMask::default().set_paths(
        update_mask
            .split(',')
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(ToString::to_string),
    )
}

fn cloud_run_error(
    error: google_cloud_gax::error::Error,
    resource_type: &str,
    resource_name: &str,
) -> AlienError<CloudClientErrorData> {
    if gax_error_is_not_found(&error) {
        return AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        });
    }

    if gax_error_is_conflict(&error) {
        return AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
            message: error.to_string(),
        });
    }

    if gax_error_is_permission_denied(&error) {
        return AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        });
    }

    AlienError::new(CloudClientErrorData::GenericError {
        message: error.to_string(),
    })
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::NOT_FOUND.as_u16())
}

fn gax_error_is_conflict(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::AlreadyExists)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::CONFLICT.as_u16())
}

fn gax_error_is_permission_denied(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::PermissionDenied)
        || error
            .http_status_code()
            .is_some_and(|code| code == StatusCode::FORBIDDEN.as_u16())
}
