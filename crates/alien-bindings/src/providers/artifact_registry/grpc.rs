use crate::{
    error::{Error, ErrorData},
    grpc::artifact_registry_service::alien_bindings::artifact_registry::{
        artifact_registry_service_client::ArtifactRegistryServiceClient,
        AddCrossAccountAccessRequest, ArtifactRegistryPermissions as ProtoRegistryPermissions,
        AwsCrossAccountAccess as ProtoAwsCrossAccountAccess,
        ComputeServiceType as ProtoComputeServiceType, CreateRepositoryRequest,
        CrossAccountAccess as ProtoCrossAccountAccess, DeleteRepositoryRequest,
        GcpCrossAccountAccess as ProtoGcpCrossAccountAccess, GenerateCredentialsRequest,
        GetCrossAccountAccessRequest, GetRepositoryRequest, RemoveCrossAccountAccessRequest,
    },
    grpc::status_conversion::status_to_alien_error,
    traits::{
        ArtifactRegistry, ArtifactRegistryCredentials, ArtifactRegistryPermissions,
        AwsCrossAccountAccess, ComputeServiceType, CrossAccountAccess, CrossAccountPermissions,
        GcpCrossAccountAccess, RepositoryResponse,
    },
    Binding,
};

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use tonic::{transport::Channel, Request, Status};

/// gRPC implementation of the `ArtifactRegistry` trait.
///
/// This implementation communicates with an alien-runtime gRPC server
/// to manage artifact registry operations.
#[derive(Debug)]
pub struct GrpcArtifactRegistry {
    client: ArtifactRegistryServiceClient<Channel>,
    binding_name: String,
}

impl GrpcArtifactRegistry {
    /// Creates a new gRPC artifact registry instance from binding parameters.
    pub async fn new(binding_name: String, grpc_address: String) -> Result<Self, Error> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_address).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Creates a new gRPC artifact registry instance from a channel.
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self, Error> {
        let client = ArtifactRegistryServiceClient::new(channel);

        Ok(Self {
            client,
            binding_name,
        })
    }

    fn client(&self) -> ArtifactRegistryServiceClient<Channel> {
        self.client.clone()
    }

    // Helper function to convert our types to proto types
    fn convert_cross_account_access_to_proto(
        access: &CrossAccountAccess,
    ) -> ProtoCrossAccountAccess {
        match access {
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
                            account_ids: aws.account_ids.clone(),
                            allowed_service_types: proto_service_types,
                            role_arns: aws.role_arns.clone(),
                            regions: aws.regions.clone(),
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
                            project_numbers: gcp.project_numbers.clone(),
                            allowed_service_types: proto_service_types,
                            service_account_emails: gcp.service_account_emails.clone(),
                        }
                    )),
                }
            }
        }
    }

    // Helper function to convert proto types to our types
    fn convert_proto_to_cross_account_permissions(
        proto: crate::grpc::artifact_registry_service::alien_bindings::artifact_registry::CrossAccountPermissions,
    ) -> Result<CrossAccountPermissions, Error> {
        let access_proto = proto.access.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: "artifact_registry".to_string(),
                field: "access".to_string(),
                response_json: "missing access field".to_string(),
            })
        })?;

        let access = match access_proto.access {
            Some(crate::grpc::artifact_registry_service::alien_bindings::artifact_registry::cross_account_access::Access::Aws(aws)) => {
                let service_types: Vec<ComputeServiceType> = aws.allowed_service_types.iter()
                    .filter_map(|&st| match ProtoComputeServiceType::try_from(st) {
                        Ok(ProtoComputeServiceType::Function) => Some(ComputeServiceType::Function),
                        _ => None,
                    })
                    .collect();

                CrossAccountAccess::Aws(AwsCrossAccountAccess {
                    account_ids: aws.account_ids,
                    allowed_service_types: service_types,
                    role_arns: aws.role_arns,
                    regions: vec![],
                })
            }
            Some(crate::grpc::artifact_registry_service::alien_bindings::artifact_registry::cross_account_access::Access::Gcp(gcp)) => {
                let service_types: Vec<ComputeServiceType> = gcp.allowed_service_types.iter()
                    .filter_map(|&st| match ProtoComputeServiceType::try_from(st) {
                        Ok(ProtoComputeServiceType::Function) => Some(ComputeServiceType::Function),
                        _ => None,
                    })
                    .collect();

                CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                    project_numbers: gcp.project_numbers,
                    allowed_service_types: service_types,
                    service_account_emails: gcp.service_account_emails,
                })
            }
            None => {
                return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "grpc".to_string(),
                    binding_name: "artifact_registry".to_string(),
                    field: "access".to_string(),
                    response_json: "missing access variant".to_string(),
                }));
            }
        };

        Ok(CrossAccountPermissions {
            access,
            last_updated: proto.last_updated,
        })
    }
}

impl Binding for GrpcArtifactRegistry {}

#[async_trait]
impl ArtifactRegistry for GrpcArtifactRegistry {
    async fn create_repository(&self, repo_name: &str) -> Result<RepositoryResponse, Error> {
        let mut client = self.client();

        let request = CreateRepositoryRequest {
            binding_name: self.binding_name.clone(),
            repo_name: repo_name.to_string(),
        };

        let response = client
            .create_repository(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "create_repository"))?
            .into_inner();

        let result = response.result.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: self.binding_name.clone(),
                field: "result".to_string(),
                response_json: "missing result field".to_string(),
            })
        })?;

        Ok(RepositoryResponse {
            name: result.name,
            uri: result.uri,
            created_at: result.created_at,
        })
    }

    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse, Error> {
        let mut client = self.client();

        let request = GetRepositoryRequest {
            binding_name: self.binding_name.clone(),
            repo_id: repo_id.to_string(),
        };

        let response = client
            .get_repository(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "get_repository"))?
            .into_inner();

        let result = response.result.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: self.binding_name.clone(),
                field: "result".to_string(),
                response_json: "missing result field".to_string(),
            })
        })?;

        Ok(RepositoryResponse {
            name: result.name,
            uri: result.uri,
            created_at: result.created_at,
        })
    }

    async fn add_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<(), Error> {
        let mut client = self.client();

        let proto_access = Self::convert_cross_account_access_to_proto(&access);

        let request = AddCrossAccountAccessRequest {
            binding_name: self.binding_name.clone(),
            repo_id: repo_id.to_string(),
            access: Some(proto_access),
        };

        client
            .add_cross_account_access(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "add_cross_account_access"))?;

        Ok(())
    }

    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<(), Error> {
        let mut client = self.client();

        let proto_access = Self::convert_cross_account_access_to_proto(&access);

        let request = RemoveCrossAccountAccessRequest {
            binding_name: self.binding_name.clone(),
            repo_id: repo_id.to_string(),
            access: Some(proto_access),
        };

        client
            .remove_cross_account_access(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "remove_cross_account_access"))?;

        Ok(())
    }

    async fn get_cross_account_access(
        &self,
        repo_id: &str,
    ) -> Result<CrossAccountPermissions, Error> {
        let mut client = self.client();

        let request = GetCrossAccountAccessRequest {
            binding_name: self.binding_name.clone(),
            repo_id: repo_id.to_string(),
        };

        let response = client
            .get_cross_account_access(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "get_cross_account_access"))?
            .into_inner();

        let permissions = response.permissions.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: self.binding_name.clone(),
                field: "permissions".to_string(),
                response_json: "missing permissions field".to_string(),
            })
        })?;

        Self::convert_proto_to_cross_account_permissions(permissions)
    }

    async fn generate_credentials(
        &self,
        repo_id: &str,
        permissions: ArtifactRegistryPermissions,
        ttl_seconds: Option<u32>,
    ) -> Result<ArtifactRegistryCredentials, Error> {
        let mut client = self.client();

        let proto_permissions = match permissions {
            ArtifactRegistryPermissions::Pull => ProtoRegistryPermissions::Pull,
            ArtifactRegistryPermissions::PushPull => ProtoRegistryPermissions::PushPull,
        };

        let request = GenerateCredentialsRequest {
            binding_name: self.binding_name.clone(),
            repo_id: repo_id.to_string(),
            permissions: proto_permissions.into(),
            ttl_seconds,
        };

        let response = client
            .generate_credentials(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "generate_credentials"))?
            .into_inner();

        let credentials = response.credentials.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: self.binding_name.clone(),
                field: "credentials".to_string(),
                response_json: "missing credentials field".to_string(),
            })
        })?;

        Ok(ArtifactRegistryCredentials {
            username: credentials.username,
            password: credentials.password,
            expires_at: credentials.expires_at,
        })
    }

    async fn delete_repository(&self, repo_id: &str) -> Result<(), Error> {
        let mut client = self.client();

        let request = DeleteRepositoryRequest {
            binding_name: self.binding_name.clone(),
            repo_id: repo_id.to_string(),
        };

        client
            .delete_repository(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "delete_repository"))?;

        Ok(())
    }
}
