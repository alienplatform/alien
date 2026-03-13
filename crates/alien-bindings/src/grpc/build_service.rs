#![cfg(feature = "grpc")]

use crate::grpc::status_conversion::alien_error_to_status;
use crate::{error::ErrorData, BindingsProviderApi, Build as AlienBuild};
use alien_error::AlienError;
use async_trait::async_trait;
use std::sync::Arc;
use tonic::{Request, Response, Status};

// Module for the generated gRPC code.
pub mod alien_bindings {
    pub mod build {
        tonic::include_proto!("alien_bindings.build");
        pub const FILE_DESCRIPTOR_SET: &[u8] =
            tonic::include_file_descriptor_set!("alien_bindings.build_descriptor");
    }
}

use alien_bindings::build::{
    build_service_server::{BuildService, BuildServiceServer},
    BuildConfig as ProtoBuildConfig, BuildExecution as ProtoBuildExecution,
    BuildStatus as ProtoBuildStatus, ComputeType as ProtoComputeType, GetBuildStatusRequest,
    GetBuildStatusResponse, StartBuildRequest, StartBuildResponse, StopBuildRequest,
    StopBuildResponse,
};

pub struct BuildGrpcServer {
    provider: Arc<dyn BindingsProviderApi>,
}

impl BuildGrpcServer {
    pub fn new(provider: Arc<dyn BindingsProviderApi>) -> Self {
        Self { provider }
    }

    pub fn into_service(self) -> BuildServiceServer<Self> {
        BuildServiceServer::new(self)
    }

    async fn get_build_binding(&self, binding_name: &str) -> Result<Arc<dyn AlienBuild>, Status> {
        self.provider
            .load_build(binding_name)
            .await
            .map_err(alien_error_to_status)
    }
}

#[async_trait]
impl BuildService for BuildGrpcServer {
    async fn start_build(
        &self,
        request: Request<StartBuildRequest>,
    ) -> Result<Response<StartBuildResponse>, Status> {
        let req_inner = request.into_inner();
        let build = self.get_build_binding(&req_inner.binding_name).await?;

        let config = req_inner
            .config
            .ok_or_else(|| Status::invalid_argument("Build config is required"))?;

        let alien_config = map_proto_build_config_to_alien(config)?;

        let execution = build
            .start_build(alien_config)
            .await
            .map_err(alien_error_to_status)?;

        let proto_execution = map_alien_build_execution_to_proto(execution);

        Ok(Response::new(StartBuildResponse {
            execution: Some(proto_execution),
        }))
    }

    async fn get_build_status(
        &self,
        request: Request<GetBuildStatusRequest>,
    ) -> Result<Response<GetBuildStatusResponse>, Status> {
        let req_inner = request.into_inner();
        let build = self.get_build_binding(&req_inner.binding_name).await?;

        let execution = build
            .get_build_status(&req_inner.build_id)
            .await
            .map_err(alien_error_to_status)?;

        let proto_execution = map_alien_build_execution_to_proto(execution);

        Ok(Response::new(GetBuildStatusResponse {
            execution: Some(proto_execution),
        }))
    }

    async fn stop_build(
        &self,
        request: Request<StopBuildRequest>,
    ) -> Result<Response<StopBuildResponse>, Status> {
        let req_inner = request.into_inner();
        let build = self.get_build_binding(&req_inner.binding_name).await?;

        build
            .stop_build(&req_inner.build_id)
            .await
            .map_err(alien_error_to_status)?;

        Ok(Response::new(StopBuildResponse {}))
    }
}

// Helper functions for type conversions and error mapping

fn map_proto_build_config_to_alien(
    config: ProtoBuildConfig,
) -> Result<alien_core::BuildConfig, Status> {
    let compute_type = match config.compute_type() {
        ProtoComputeType::Unspecified => alien_core::ComputeType::Small,
        ProtoComputeType::Small => alien_core::ComputeType::Small,
        ProtoComputeType::Medium => alien_core::ComputeType::Medium,
        ProtoComputeType::Large => alien_core::ComputeType::Large,
        ProtoComputeType::Xlarge => alien_core::ComputeType::XLarge,
    };

    // Convert monitoring configuration if present
    let monitoring = if let Some(proto_monitoring) = config.monitoring {
        Some(alien_core::MonitoringConfig {
            endpoint: proto_monitoring.endpoint,
            headers: proto_monitoring.headers,
            logs_uri: proto_monitoring.logs_uri,
            tls_enabled: proto_monitoring.tls_enabled,
            tls_verify: proto_monitoring.tls_verify,
        })
    } else {
        None
    };

    Ok(alien_core::BuildConfig {
        image: "ubuntu:20.04".to_string(), // Default image, should be configurable in the future
        script: config.script,
        environment: config.environment,
        compute_type,
        timeout_seconds: config.timeout_seconds.unwrap_or(300) as u32,
        monitoring,
    })
}

fn map_alien_build_execution_to_proto(
    execution: alien_core::BuildExecution,
) -> ProtoBuildExecution {
    let status = match execution.status {
        alien_core::BuildStatus::Queued => ProtoBuildStatus::Queued,
        alien_core::BuildStatus::Running => ProtoBuildStatus::Running,
        alien_core::BuildStatus::Succeeded => ProtoBuildStatus::Succeeded,
        alien_core::BuildStatus::Failed => ProtoBuildStatus::Failed,
        alien_core::BuildStatus::Cancelled => ProtoBuildStatus::Cancelled,
        alien_core::BuildStatus::TimedOut => ProtoBuildStatus::TimedOut,
    };

    ProtoBuildExecution {
        id: execution.id,
        status: status.into(),
        start_time: execution.start_time,
        end_time: execution.end_time,
    }
}
