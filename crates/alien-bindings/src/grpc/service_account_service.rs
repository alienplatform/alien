#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::{traits::ServiceAccount as AlienServiceAccount, BindingsProviderApi};
use async_trait::async_trait;
use std::sync::Arc;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod service_account {
        tonic::include_proto!("alien_bindings.service_account");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.service_account_descriptor");
    }
}

use alien_bindings::service_account::{
    service_account_info::Info,
    service_account_service_server::{ServiceAccountService, ServiceAccountServiceServer},
    AwsServiceAccountInfo as GrpcAwsServiceAccountInfo,
    AzureServiceAccountInfo as GrpcAzureServiceAccountInfo,
    GcpServiceAccountInfo as GrpcGcpServiceAccountInfo, GetInfoRequest, GetInfoResponse,
    ImpersonateRequest, ImpersonateResponse, ServiceAccountInfo as GrpcServiceAccountInfo,
};

use crate::traits::{ImpersonationRequest, ServiceAccountInfo};

pub struct ServiceAccountGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl ServiceAccountGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> ServiceAccountServiceServer<Self> {
        ServiceAccountServiceServer::new(self)
    }

    async fn get_service_account_binding(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn AlienServiceAccount>, Status> {
        self.provider
            .load_service_account(binding_name)
            .await
            .map_err(alien_error_to_status)
    }
}

#[async_trait]
impl ServiceAccountService for ServiceAccountGrpcServer {
    async fn get_info(
        &self,
        request: Request<GetInfoRequest>,
    ) -> Result<Response<GetInfoResponse>, Status> {
        let req_inner = request.into_inner();
        let service_account = self
            .get_service_account_binding(&req_inner.binding_name)
            .await?;

        let info = service_account
            .get_info()
            .await
            .map_err(alien_error_to_status)?;

        // Convert from trait types to protobuf
        let grpc_info = match info {
            ServiceAccountInfo::Aws(aws) => GrpcServiceAccountInfo {
                info: Some(Info::Aws(GrpcAwsServiceAccountInfo {
                    role_name: aws.role_name,
                    role_arn: aws.role_arn,
                })),
            },
            ServiceAccountInfo::Gcp(gcp) => GrpcServiceAccountInfo {
                info: Some(Info::Gcp(GrpcGcpServiceAccountInfo {
                    email: gcp.email,
                    unique_id: gcp.unique_id,
                })),
            },
            ServiceAccountInfo::Azure(azure) => GrpcServiceAccountInfo {
                info: Some(Info::Azure(GrpcAzureServiceAccountInfo {
                    client_id: azure.client_id,
                    resource_id: azure.resource_id,
                    principal_id: azure.principal_id,
                })),
            },
        };

        Ok(Response::new(GetInfoResponse {
            info: Some(grpc_info),
        }))
    }

    async fn impersonate(
        &self,
        request: Request<ImpersonateRequest>,
    ) -> Result<Response<ImpersonateResponse>, Status> {
        let req_inner = request.into_inner();
        let service_account = self
            .get_service_account_binding(&req_inner.binding_name)
            .await?;

        let impersonation_request = ImpersonationRequest {
            session_name: req_inner.session_name,
            duration_seconds: req_inner.duration_seconds,
            scopes: if req_inner.scopes.is_empty() {
                None
            } else {
                Some(req_inner.scopes)
            },
        };

        let client_config = service_account
            .impersonate(impersonation_request)
            .await
            .map_err(alien_error_to_status)?;

        // Serialize ClientConfig to JSON
        let client_config_json = serde_json::to_string(&client_config)
            .map_err(|e| Status::internal(format!("Failed to serialize ClientConfig: {}", e)))?;

        Ok(Response::new(ImpersonateResponse { client_config_json }))
    }
}
