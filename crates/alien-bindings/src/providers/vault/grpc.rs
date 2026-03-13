use crate::{
    error::{Error, ErrorData},
    grpc::status_conversion::status_to_alien_error,
    grpc::vault_service::alien_bindings::vault::{
        vault_service_client::VaultServiceClient, DeleteSecretRequest, GetSecretRequest,
        SetSecretRequest,
    },
    traits::{Binding, Vault},
};

use alien_error::{AlienError, Context};
use async_trait::async_trait;
use tonic::{transport::Channel, Request, Status};

/// gRPC implementation of the `Vault` trait.
///
/// This implementation communicates with an alien-runtime gRPC server
/// to manage vault operations.
#[derive(Debug)]
pub struct GrpcVault {
    client: VaultServiceClient<Channel>,
    binding_name: String,
}

impl GrpcVault {
    /// Creates a new gRPC vault instance from binding parameters.
    pub async fn new(binding_name: String, grpc_address: String) -> Result<Self, Error> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_address).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Creates a new gRPC vault instance from a channel.
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self, Error> {
        let client = VaultServiceClient::new(channel);

        Ok(Self {
            client,
            binding_name,
        })
    }

    fn client(&self) -> VaultServiceClient<Channel> {
        self.client.clone()
    }
}

impl Binding for GrpcVault {}

#[async_trait]
impl Vault for GrpcVault {
    async fn get_secret(&self, secret_name: &str) -> Result<String, Error> {
        let mut client = self.client();

        let request = GetSecretRequest {
            binding_name: self.binding_name.clone(),
            secret_name: secret_name.to_string(),
        };

        let response = client
            .get_secret(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "get_secret"))?
            .into_inner();

        Ok(response.value)
    }

    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<(), Error> {
        let mut client = self.client();

        let request = SetSecretRequest {
            binding_name: self.binding_name.clone(),
            secret_name: secret_name.to_string(),
            value: value.to_string(),
        };

        client
            .set_secret(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "set_secret"))?;

        Ok(())
    }

    async fn delete_secret(&self, secret_name: &str) -> Result<(), Error> {
        let mut client = self.client();

        let request = DeleteSecretRequest {
            binding_name: self.binding_name.clone(),
            secret_name: secret_name.to_string(),
        };

        client
            .delete_secret(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "delete_secret"))?;

        Ok(())
    }
}
