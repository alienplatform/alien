//! The Worker app protocol between `alien-worker-runtime` and a Worker
//! application: task delivery, lifecycle, and `waitUntil` draining.
//!
//! Generated clients and message types are always available. The Worker runtime
//! enables the `server` feature to host the Control and WaitUntil services.

/// Generated Control service client, server stub, and message types.
pub mod control {
    tonic::include_proto!("alien_worker.control");

    #[cfg(feature = "server")]
    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("alien_worker.control_descriptor");
}

/// Generated WaitUntil service client, server stub, and message types.
pub mod wait_until {
    tonic::include_proto!("alien_worker.wait_until");

    #[cfg(feature = "server")]
    pub(crate) const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("alien_worker.wait_until_descriptor");
}

#[cfg(feature = "server")]
mod control_service;
#[cfg(feature = "server")]
pub mod error;
#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
mod wait_until_service;

#[cfg(feature = "server")]
pub use control_service::ControlGrpcServer;
#[cfg(feature = "server")]
pub use server::{run_grpc_server, GrpcServerHandles};
#[cfg(feature = "server")]
pub use wait_until_service::WaitUntilGrpcServer;

#[cfg(feature = "server")]
pub(crate) const MAX_GRPC_MESSAGE_SIZE: usize = 4 * 1024 * 1024 * 1024; // 4GB
