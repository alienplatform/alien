//! The Worker app protocol: gRPC services that coordinate the Alien runtime with
//! the application it manages (task delivery, lifecycle, and waitUntil draining).
//!
//! The runtime hosts these services ([`ControlGrpcServer`], [`WaitUntilGrpcServer`])
//! via [`run_grpc_server`]; the application connects as the gRPC client. The proto
//! packages are `alien_worker.control` and `alien_worker.wait_until`.

pub mod error;

pub mod control_service;
pub mod wait_until_service;

pub mod server;
pub use server::{run_grpc_server, GrpcServerHandles};

pub use control_service::ControlGrpcServer;
pub use wait_until_service::WaitUntilGrpcServer;

// Re-export control service proto types for easier access.
pub mod control {
    pub use super::control_service::alien_worker::control::*;
}

// Re-export wait_until service proto types for easier access.
pub mod wait_until {
    pub use super::wait_until_service::alien_worker::wait_until::*;
}

pub(crate) const MAX_GRPC_MESSAGE_SIZE: usize = 4 * 1024 * 1024 * 1024; // 4GB
