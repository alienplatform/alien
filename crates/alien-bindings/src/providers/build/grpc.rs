use crate::{
    error::{Error, ErrorData},
    grpc::build_service::alien_bindings::build::{
        build_service_client::BuildServiceClient, BuildConfig as ProtoBuildConfig,
        ComputeType as ProtoComputeType, GetBuildStatusRequest, StartBuildRequest,
        StopBuildRequest,
    },
    grpc::status_conversion::status_to_alien_error,
    traits::Build,
};
use alien_core::{BuildConfig, BuildExecution, BuildStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use tonic::{transport::Channel, Request, Status};

/// gRPC implementation of the `Build` trait.
///
/// This implementation communicates with an alien-runtime gRPC server
/// to manage build operations.
#[derive(Debug)]
pub struct GrpcBuild {
    client: BuildServiceClient<Channel>,
    binding_name: String,
}

impl GrpcBuild {
    /// Creates a new gRPC build instance from binding parameters.
    pub async fn new(binding_name: String, grpc_address: String) -> Result<Self, Error> {
        let channel = crate::providers::grpc_provider::create_grpc_channel(grpc_address).await?;
        Self::new_from_channel(channel, binding_name).await
    }

    /// Creates a new gRPC build instance from a channel.
    pub async fn new_from_channel(channel: Channel, binding_name: String) -> Result<Self, Error> {
        let client = BuildServiceClient::new(channel);

        Ok(Self {
            client,
            binding_name,
        })
    }

    fn client(&self) -> BuildServiceClient<Channel> {
        self.client.clone()
    }
}

fn map_alien_config_to_proto(config: BuildConfig) -> ProtoBuildConfig {
    let compute_type = match config.compute_type {
        alien_core::ComputeType::Small => ProtoComputeType::Small,
        alien_core::ComputeType::Medium => ProtoComputeType::Medium,
        alien_core::ComputeType::Large => ProtoComputeType::Large,
        alien_core::ComputeType::XLarge => ProtoComputeType::Xlarge,
    };

    // Convert monitoring configuration if present
    let monitoring = if let Some(alien_monitoring) = config.monitoring {
        Some(
            crate::grpc::build_service::alien_bindings::build::MonitoringConfig {
                endpoint: alien_monitoring.endpoint,
                headers: alien_monitoring.headers,
                logs_uri: alien_monitoring.logs_uri,
                tls_enabled: alien_monitoring.tls_enabled,
                tls_verify: alien_monitoring.tls_verify,
            },
        )
    } else {
        None
    };

    ProtoBuildConfig {
        script: config.script,
        environment: config.environment,
        compute_type: compute_type.into(),
        timeout_seconds: Some(config.timeout_seconds as i32),
        monitoring,
    }
}

fn map_proto_execution_to_alien(
    execution: crate::grpc::build_service::alien_bindings::build::BuildExecution,
) -> BuildExecution {
    let status = match execution.status() {
        crate::grpc::build_service::alien_bindings::build::BuildStatus::Unspecified => {
            BuildStatus::Failed
        }
        crate::grpc::build_service::alien_bindings::build::BuildStatus::Queued => {
            BuildStatus::Queued
        }
        crate::grpc::build_service::alien_bindings::build::BuildStatus::Running => {
            BuildStatus::Running
        }
        crate::grpc::build_service::alien_bindings::build::BuildStatus::Succeeded => {
            BuildStatus::Succeeded
        }
        crate::grpc::build_service::alien_bindings::build::BuildStatus::Failed => {
            BuildStatus::Failed
        }
        crate::grpc::build_service::alien_bindings::build::BuildStatus::Cancelled => {
            BuildStatus::Cancelled
        }
        crate::grpc::build_service::alien_bindings::build::BuildStatus::TimedOut => {
            BuildStatus::TimedOut
        }
    };

    BuildExecution {
        id: execution.id,
        status,
        start_time: execution.start_time,
        end_time: execution.end_time,
    }
}

#[async_trait]
impl Build for GrpcBuild {
    async fn start_build(&self, config: BuildConfig) -> Result<BuildExecution, Error> {
        let mut client = self.client();

        let proto_config = map_alien_config_to_proto(config);
        let request = StartBuildRequest {
            binding_name: self.binding_name.clone(),
            config: Some(proto_config),
        };

        let response = client
            .start_build(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "start_build"))?
            .into_inner();

        let execution = response.execution.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: self.binding_name.clone(),
                field: "execution".to_string(),
                response_json: "missing execution field".to_string(),
            })
        })?;

        Ok(map_proto_execution_to_alien(execution))
    }

    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution, Error> {
        let mut client = self.client();

        let request = GetBuildStatusRequest {
            binding_name: self.binding_name.clone(),
            build_id: build_id.to_string(),
        };

        let response = client
            .get_build_status(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "get_build_status"))?
            .into_inner();

        let execution = response.execution.ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "grpc".to_string(),
                binding_name: self.binding_name.clone(),
                field: "execution".to_string(),
                response_json: "missing execution field".to_string(),
            })
        })?;

        Ok(map_proto_execution_to_alien(execution))
    }

    async fn stop_build(&self, build_id: &str) -> Result<(), Error> {
        let mut client = self.client();

        let request = StopBuildRequest {
            binding_name: self.binding_name.clone(),
            build_id: build_id.to_string(),
        };

        client
            .stop_build(Request::new(request))
            .await
            .map_err(|e| status_to_alien_error(e, "stop_build"))?;

        Ok(())
    }
}

impl crate::traits::Binding for GrpcBuild {}
