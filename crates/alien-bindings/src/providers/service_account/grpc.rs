use crate::{
    error::{Error, ErrorData},
    grpc::service_account_service::alien_bindings::service_account::{
        service_account_service_client::ServiceAccountServiceClient, GetInfoRequest,
        ImpersonateRequest as GrpcImpersonateRequest,
    },
    grpc::status_conversion::status_to_alien_error,
    traits::{
        AwsServiceAccountInfo, AzureServiceAccountInfo, Binding, GcpServiceAccountInfo,
        ImpersonationRequest, ServiceAccount, ServiceAccountInfo,
    },
};

use alien_core::ClientConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use tonic::{transport::Channel, Request, Status};

/// gRPC implementation of the `ServiceAccount` trait.
///
/// This implementation communicates with an alien-runtime gRPC server
/// to manage service account operations.
#[derive(Debug)]
pub struct GrpcServiceAccount {
    client: ServiceAccountServiceClient<Channel>,
    binding_name: String,
}

impl GrpcServiceAccount {
    /// Creates a new gRPC service account instance from binding parameters.
    pub async fn new(binding_name: String, grpc_address: String) -> Result<Self, Error> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_address).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Creates a new gRPC service account instance from a channel.
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self, Error> {
        let client = ServiceAccountServiceClient::new(channel);

        Ok(Self {
            client,
            binding_name,
        })
    }

    fn client(&self) -> ServiceAccountServiceClient<Channel> {
        self.client.clone()
    }
}

impl Binding for GrpcServiceAccount {}

#[async_trait]
impl ServiceAccount for GrpcServiceAccount {
    async fn get_info(&self) -> Result<ServiceAccountInfo, Error> {
        let mut client = self.client();

        let request = GetInfoRequest {
            binding_name: self.binding_name.clone(),
        };

        let response = client
            .get_info(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "get_info"))?
            .into_inner();

        let info = response.info.ok_or_else(|| {
            AlienError::new(ErrorData::Other {
                message: "Service account info is missing from response".to_string(),
            })
        })?;

        // Convert from protobuf to trait types
        use crate::grpc::service_account_service::alien_bindings::service_account::service_account_info::Info;

        let service_account_info = match info.info {
            Some(Info::Aws(aws)) => ServiceAccountInfo::Aws(AwsServiceAccountInfo {
                role_name: aws.role_name,
                role_arn: aws.role_arn,
            }),
            Some(Info::Gcp(gcp)) => ServiceAccountInfo::Gcp(GcpServiceAccountInfo {
                email: gcp.email,
                unique_id: gcp.unique_id,
            }),
            Some(Info::Azure(azure)) => ServiceAccountInfo::Azure(AzureServiceAccountInfo {
                client_id: azure.client_id,
                resource_id: azure.resource_id,
                principal_id: azure.principal_id,
            }),
            None => {
                return Err(AlienError::new(ErrorData::Other {
                    message: "Service account info variant is missing".to_string(),
                }))
            }
        };

        Ok(service_account_info)
    }

    async fn impersonate(&self, request: ImpersonationRequest) -> Result<ClientConfig, Error> {
        let mut client = self.client();

        let grpc_request = GrpcImpersonateRequest {
            binding_name: self.binding_name.clone(),
            session_name: request.session_name,
            duration_seconds: request.duration_seconds,
            scopes: request.scopes.unwrap_or_default(),
        };

        let response = client
            .impersonate(Request::new(grpc_request))
            .await
            .map_err(|e| status_to_alien_error(e, "impersonate"))?
            .into_inner();

        // Deserialize ClientConfig from JSON
        let client_config: ClientConfig = serde_json::from_str(&response.client_config_json)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to deserialize ClientConfig from gRPC response".to_string(),
            })?;

        Ok(client_config)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
