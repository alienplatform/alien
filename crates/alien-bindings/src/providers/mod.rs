// Service-type based organization
pub mod artifact_registry;
pub mod build;
pub mod container;
pub mod function;
pub mod kv;
pub mod queue;
pub mod service_account;
pub mod storage;
pub mod vault;

// gRPC provider (cross-service)
#[cfg(feature = "grpc")]
pub mod grpc_provider;

pub mod utils;
