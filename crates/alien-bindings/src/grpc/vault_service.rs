#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::{traits::Vault as AlienVault, BindingsProviderApi};
use async_trait::async_trait;
use std::sync::Arc;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod vault {
        tonic::include_proto!("alien_bindings.vault");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.vault_descriptor");
    }
}

use alien_bindings::vault::{
    vault_service_server::{VaultService, VaultServiceServer},
    DeleteSecretRequest, DeleteSecretResponse, GetSecretRequest, GetSecretResponse,
    SetSecretRequest, SetSecretResponse,
};

pub struct VaultGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl VaultGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> VaultServiceServer<Self> {
        VaultServiceServer::new(self)
    }

    async fn get_vault_binding(&self, binding_name: &str) -> Result<Arc<dyn AlienVault>, Status> {
        self.provider
            .load_vault(binding_name)
            .await
            .map_err(alien_error_to_status)
    }
}

#[async_trait]
impl VaultService for VaultGrpcServer {
    async fn get_secret(
        &self,
        request: Request<GetSecretRequest>,
    ) -> Result<Response<GetSecretResponse>, Status> {
        let req_inner = request.into_inner();
        let vault = self.get_vault_binding(&req_inner.binding_name).await?;

        let value = vault
            .get_secret(&req_inner.secret_name)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(GetSecretResponse { value }))
    }

    async fn set_secret(
        &self,
        request: Request<SetSecretRequest>,
    ) -> Result<Response<SetSecretResponse>, Status> {
        let req_inner = request.into_inner();
        let vault = self.get_vault_binding(&req_inner.binding_name).await?;

        vault
            .set_secret(&req_inner.secret_name, &req_inner.value)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(SetSecretResponse {}))
    }

    async fn delete_secret(
        &self,
        request: Request<DeleteSecretRequest>,
    ) -> Result<Response<DeleteSecretResponse>, Status> {
        let req_inner = request.into_inner();
        let vault = self.get_vault_binding(&req_inner.binding_name).await?;

        vault
            .delete_secret(&req_inner.secret_name)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(DeleteSecretResponse {}))
    }
}
