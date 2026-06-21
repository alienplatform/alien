use crate::core::IamPolicy;
use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::GcpClientConfig;
use alien_error::AlienError;
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
pub use google_cloud_longrunning::model::{operation::Result as OperationResult, Operation};
pub use google_cloud_run_v2::{
    client::Services as OfficialCloudRunServices,
    model::{
        condition::State as ConditionState, vpc_access::NetworkInterface, vpc_access::VpcEgress,
        Condition, Container, ContainerPort, EnvVar, ExecutionEnvironment,
        IngressTraffic as Ingress, ResourceRequirements, RevisionScaling, RevisionTemplate,
        Service, ServiceScaling, TrafficTarget, TrafficTargetAllocationType, VpcAccess,
    },
};
use http::StatusCode;
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
            .set_service(service);

        if let Some(validate_only) = validate_only {
            request = request.set_validate_only(validate_only);
        }

        request
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Service", &service_id))
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
        if service.name.is_empty() {
            service.name = self.service_resource_name(&location, &service_name);
        }

        let mut request = self.services().await?.update_service().set_service(service);

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
    }

    async fn set_service_iam_policy(
        &self,
        location: String,
        service_name: String,
        iam_policy: IamPolicy,
    ) -> CloudClientResult<IamPolicy> {
        let request = google_cloud_iam_v1::model::SetIamPolicyRequest::new()
            .set_resource(self.service_resource_name(&location, &service_name))
            .set_policy(iam_policy);

        self.services()
            .await?
            .set_iam_policy()
            .with_request(request)
            .send()
            .await
            .map_err(|error| cloud_run_error(error, "Service", &service_name))
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
