//! # alien-commands
//!
//! Commands protocol implementation for Alien.
//!
//! This crate provides a transport-agnostic protocol for sending commands
//! to customer-side agents without requiring inbound connections.
//!
//! ## Features
//!
//! - **Core types**: Always available protocol types and serialization
//! - **server**: Command server implementation for managers
//! - **runtime**: Command envelope processing for alien-worker-runtime
//! - **receiver**: App-owned pull command receiver for Containers/Daemons
//! - **openapi**: OpenAPI schema generation support

pub mod error;
pub mod types;

pub use error::{Error, ErrorData, Result};
pub use types::*;

#[cfg(any(feature = "server", feature = "dispatchers"))]
pub mod dispatchers;

#[cfg(feature = "server")]
pub mod server;

#[cfg(any(feature = "runtime", feature = "receiver"))]
pub mod runtime;

#[cfg(feature = "receiver")]
pub mod receiver;

#[cfg(feature = "test-utils")]
pub mod test_utils;

// Re-export commonly used types
pub use types::{
    BodySpec, CommandResponse, CommandState, CommandStatusResponse, CreateCommandRequest,
    CreateCommandResponse, Envelope, LeaseInfo, LeaseRequest, LeaseResponse, ResponseHandling,
    StorageUpload, SubmitResponseRequest, UploadCompleteRequest, UploadCompleteResponse,
};

#[cfg(feature = "server")]
pub use server::{create_axum_router, CommandRegistry, CommandServer, InMemoryCommandRegistry};

#[cfg(any(feature = "runtime", feature = "receiver"))]
pub use runtime::{
    command_budget, decode_params, parse_envelope, submit_response, LeaseClient,
    LEASE_SAFETY_MARGIN,
};

// NB: `receiver::Context` is intentionally NOT re-exported at the crate root —
// it would collide with `alien_error::Context` (the error-chaining trait).
// Import it as `alien_commands::receiver::Context`.
#[cfg(feature = "receiver")]
pub use receiver::{Receiver, ShutdownHandle};

/// Default inline size limit in bytes (150 KB)
/// This is the most conservative platform limit (Azure Service Bus Standard at 256KB)
/// with headroom for base64 encoding (~4/3 inflation) and envelope metadata.
pub const INLINE_MAX_BYTES: usize = 150_000;

/// Protocol version identifier
pub const PROTOCOL_VERSION: &str = "arc.v1";
