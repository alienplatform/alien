pub(crate) mod artifact_registry_service;
pub(crate) mod build_service;
pub(crate) mod container_service;
pub mod control_service;
pub(crate) mod function_service;
pub(crate) mod kv_service;
pub(crate) mod queue_service;
pub(crate) mod service_account_service;
pub(crate) mod status_conversion;
pub(crate) mod storage_service;
pub(crate) mod storage_utils;
pub(crate) mod vault_service;
pub mod wait_until_service;

pub mod server;
pub use server::{run_grpc_server, GrpcServerHandles};

// Re-export control service proto types for easier access
pub mod control {
    pub use super::control_service::alien_bindings::control::*;
}

pub(crate) const MAX_GRPC_MESSAGE_SIZE: usize = 4 * 1024 * 1024 * 1024; // 4GB
