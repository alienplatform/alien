pub mod control_service;
pub mod wait_until_service;

pub mod server;
pub use server::{run_grpc_server, GrpcServerHandles};

// Re-export control service proto types for easier access
pub mod control {
    pub use super::control_service::alien_bindings::control::*;
}

pub(crate) const MAX_GRPC_MESSAGE_SIZE: usize = 4 * 1024 * 1024 * 1024; // 4GB
