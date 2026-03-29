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
//! - **runtime**: Command envelope processing for alien-runtime
//! - **openapi**: OpenAPI schema generation support

pub mod error;
pub mod types;

pub use error::{Error, ErrorData, Result};
pub use types::*;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "runtime")]
pub mod runtime;

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

#[cfg(feature = "runtime")]
pub use runtime::{decode_params, parse_envelope, submit_response};

/// Default inline size limit in bytes (150 KB)
/// This is the most conservative platform limit (Azure Service Bus Standard at 256KB)
/// with headroom for base64 encoding (~4/3 inflation) and envelope metadata.
pub const INLINE_MAX_BYTES: usize = 150_000;

/// Protocol version identifier
pub const PROTOCOL_VERSION: &str = "arc.v1";
