#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::traits::{
    AwsCrossAccountAccess, ComputeServiceType, CrossAccountAccess, CrossAccountPermissions,
    GcpCrossAccountAccess,
};
use crate::{
    error::ErrorData, ArtifactRegistry as AlienArtifactRegistry, ArtifactRegistryPermissions,
    BindingsProviderApi,
};
use alien_error::AlienError;
use async_trait::async_trait;
use std::sync::Arc;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod artifact_registry {
        tonic::include_proto!("alien_bindings.artifact_registry");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.artifact_registry_descriptor");
    }
}

use alien_bindings::artifact_registry::{
    artifact_registry_service_server::{ArtifactRegistryService, ArtifactRegistryServiceServer},
    AddCrossAccountAccessRequest, AddCrossAccountAccessResponse,
    ArtifactRegistryPermissions as ProtoRegistryPermissions,
    AwsCrossAccountAccess as ProtoAwsCrossAccountAccess,
    ComputeServiceType as ProtoComputeServiceType, CreateRepositoryRequest,
    CreateRepositoryResponse, Credentials as ProtoCredentials,
    CrossAccountAccess as ProtoCrossAccountAccess,
    CrossAccountPermissions as ProtoCrossAccountPermissions, DeleteRepositoryRequest,
    DeleteRepositoryResponse, GcpCrossAccountAccess as ProtoGcpCrossAccountAccess,
    GenerateCredentialsRequest, GenerateCredentialsResponse, GetCrossAccountAccessRequest,
    GetCrossAccountAccessResponse, GetRepositoryRequest, GetRepositoryResponse,
    RemoveCrossAccountAccessRequest, RemoveCrossAccountAccessResponse, RepositoryResult,
};

pub struct ArtifactRegistryGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl ArtifactRegistryGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> ArtifactRegistryServiceServer<Self> {
        ArtifactRegistryServiceServer::new(self)
    }

    async fn get_artifact_registry_binding(
        &self,
        binding_name: &str,
    ) -> Result<Arc<dyn AlienArtifactRegistry>, Status> {
        self.provider
            .load_artifact_registry(binding_name)
            .await
            .map_err(alien_error_to_status)
    }

    // Helper function to convert proto types to our types
    fn convert_proto_to_cross_account_access(
        proto: ProtoCrossAccountAccess,
    ) -> Result<CrossAccountAccess, Status> {
        match proto.access {
            Some(crate::grpc::artifact_registry_service::alien_bindings::artifact_registry::cross_account_access::Access::Aws(aws)) => {
                let service_types: Vec<ComputeServiceType> = aws.allowed_service_types.iter()
                    .filter_map(|&st| match ProtoComputeServiceType::try_from(st) {
                        Ok(ProtoComputeServiceType::Function) => Some(ComputeServiceType::Function),
                        _ => None,
                    })
                    .collect();

                Ok(CrossAccountAccess::Aws(AwsCrossAccountAccess {
                    account_ids: aws.account_ids,
                    allowed_service_types: service_types,
                    role_arns: aws.role_arns,
                }))
            }
            Some(crate::grpc::artifact_registry_service::alien_bindings::artifact_registry::cross_account_access::Access::Gcp(gcp)) => {
                let service_types: Vec<ComputeServiceType> = gcp.allowed_service_types.iter()
                    .filter_map(|&st| match ProtoComputeServiceType::try_from(st) {
                        Ok(ProtoComputeServiceType::Function) => Some(ComputeServiceType::Function),
                        _ => None,
                    })
                    .collect();

                Ok(CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                    project_numbers: gcp.project_numbers,
                    allowed_service_types: service_types,
                    service_account_emails: gcp.service_account_emails,
                }))
            }
            None => {
                Err(alien_error_to_status(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "grpc".to_string(),
                    binding_name: "artifact_registry".to_string(),
                    field: "access".to_string(),
                    response_json: "missing access variant".to_string(),
                })))
            }
        }
    }

    // Helper function to convert our types to proto types
    fn convert_cross_account_permissions_to_proto(
        permissions: CrossAccountPermissions,
    ) -> ProtoCrossAccountPermissions {
        let proto_access = match permissions.access {
            CrossAccountAccess::Aws(aws) => {
                let proto_service_types: Vec<i32> = aws
                    .allowed_service_types
                    .iter()
                    .map(|st| match st {
                        ComputeServiceType::Function => ProtoComputeServiceType::Function as i32,
                    })
                    .collect();

                ProtoCrossAccountAccess {
                    access: Some(crate::grpc::artifact_registry_service::alien_bindings::artifact_registry::cross_account_access::Access::Aws(
                        ProtoAwsCrossAccountAccess {
                            account_ids: aws.account_ids,
                            allowed_service_types: proto_service_types,
                            role_arns: aws.role_arns,
                        }
                    )),
                }
            }
            CrossAccountAccess::Gcp(gcp) => {
                let proto_service_types: Vec<i32> = gcp
                    .allowed_service_types
                    .iter()
                    .map(|st| match st {
                        ComputeServiceType::Function => ProtoComputeServiceType::Function as i32,
                    })
                    .collect();

                ProtoCrossAccountAccess {
                    access: Some(crate::grpc::artifact_registry_service::alien_bindings::artifact_registry::cross_account_access::Access::Gcp(
                        ProtoGcpCrossAccountAccess {
                            project_numbers: gcp.project_numbers,
                            allowed_service_types: proto_service_types,
                            service_account_emails: gcp.service_account_emails,
                        }
                    )),
                }
            }
        };

        ProtoCrossAccountPermissions {
            access: Some(proto_access),
            last_updated: permissions.last_updated,
        }
    }
}

#[async_trait]
impl ArtifactRegistryService for ArtifactRegistryGrpcServer {
    async fn create_repository(
        &self,
        request: Request<CreateRepositoryRequest>,
    ) -> Result<Response<CreateRepositoryResponse>, Status> {
        let req_inner = request.into_inner();
        let registry = self
            .get_artifact_registry_binding(&req_inner.binding_name)
            .await?;

        let response = registry
            .create_repository(&req_inner.repo_name)
            .await
            .map_err(alien_error_to_status)?;

        let proto_result = RepositoryResult {
            name: response.name,
            uri: response.uri,
            created_at: response.created_at,
        };

        Ok(Response::new(CreateRepositoryResponse {
            result: Some(proto_result),
        }))
    }

    async fn get_repository(
        &self,
        request: Request<GetRepositoryRequest>,
    ) -> Result<Response<GetRepositoryResponse>, Status> {
        let req_inner = request.into_inner();
        let registry = self
            .get_artifact_registry_binding(&req_inner.binding_name)
            .await?;

        let response = registry
            .get_repository(&req_inner.repo_id)
            .await
            .map_err(alien_error_to_status)?;

        let proto_result = RepositoryResult {
            name: response.name,
            uri: response.uri,
            created_at: response.created_at,
        };

        Ok(Response::new(GetRepositoryResponse {
            result: Some(proto_result),
        }))
    }

    async fn add_cross_account_access(
        &self,
        request: Request<AddCrossAccountAccessRequest>,
    ) -> Result<Response<AddCrossAccountAccessResponse>, Status> {
        let req_inner = request.into_inner();
        let registry = self
            .get_artifact_registry_binding(&req_inner.binding_name)
            .await?;

        let access_proto = req_inner.access.ok_or_else(|| {
            alien_error_to_status(AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: req_inner.binding_name.clone(),
                field: "access".to_string(),
                response_json: "missing access field".to_string(),
            }))
        })?;

        let access = Self::convert_proto_to_cross_account_access(access_proto)?;

        registry
            .add_cross_account_access(&req_inner.repo_id, access)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(AddCrossAccountAccessResponse {}))
    }

    async fn remove_cross_account_access(
        &self,
        request: Request<RemoveCrossAccountAccessRequest>,
    ) -> Result<Response<RemoveCrossAccountAccessResponse>, Status> {
        let req_inner = request.into_inner();
        let registry = self
            .get_artifact_registry_binding(&req_inner.binding_name)
            .await?;

        let access_proto = req_inner.access.ok_or_else(|| {
            alien_error_to_status(AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: req_inner.binding_name.clone(),
                field: "access".to_string(),
                response_json: "missing access field".to_string(),
            }))
        })?;

        let access = Self::convert_proto_to_cross_account_access(access_proto)?;

        registry
            .remove_cross_account_access(&req_inner.repo_id, access)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(RemoveCrossAccountAccessResponse {}))
    }

    async fn get_cross_account_access(
        &self,
        request: Request<GetCrossAccountAccessRequest>,
    ) -> Result<Response<GetCrossAccountAccessResponse>, Status> {
        let req_inner = request.into_inner();
        let registry = self
            .get_artifact_registry_binding(&req_inner.binding_name)
            .await?;

        let permissions = registry
            .get_cross_account_access(&req_inner.repo_id)
            .await
            .map_err(alien_error_to_status)?;

        let proto_permissions = Self::convert_cross_account_permissions_to_proto(permissions);

        Ok(Response::new(GetCrossAccountAccessResponse {
            permissions: Some(proto_permissions),
        }))
    }

    async fn generate_credentials(
        &self,
        request: Request<GenerateCredentialsRequest>,
    ) -> Result<Response<GenerateCredentialsResponse>, Status> {
        let req_inner = request.into_inner();
        let registry = self
            .get_artifact_registry_binding(&req_inner.binding_name)
            .await?;

        let permissions = match ProtoRegistryPermissions::try_from(req_inner.permissions)
            .unwrap_or(ProtoRegistryPermissions::Pull)
        {
            ProtoRegistryPermissions::Pull => ArtifactRegistryPermissions::Pull,
            ProtoRegistryPermissions::PushPull => ArtifactRegistryPermissions::PushPull,
        };

        let credentials = registry
            .generate_credentials(&req_inner.repo_id, permissions, req_inner.ttl_seconds)
            .await
            .map_err(alien_error_to_status)?;

        let proto_credentials = ProtoCredentials {
            username: credentials.username,
            password: credentials.password,
            expires_at: credentials.expires_at,
        };

        Ok(Response::new(GenerateCredentialsResponse {
            credentials: Some(proto_credentials),
        }))
    }

    async fn delete_repository(
        &self,
        request: Request<DeleteRepositoryRequest>,
    ) -> Result<Response<DeleteRepositoryResponse>, Status> {
        let req_inner = request.into_inner();
        let registry = self
            .get_artifact_registry_binding(&req_inner.binding_name)
            .await?;

        registry
            .delete_repository(&req_inner.repo_id)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(DeleteRepositoryResponse {}))
    }
}
