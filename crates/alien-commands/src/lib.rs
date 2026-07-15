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

/// Resolve manager-relative URLs in a leased command envelope against the
/// trusted commands endpoint used to acquire that lease.
///
/// The manager cannot know which address is reachable from a deployment's
/// network boundary, so lease responses use root-relative URLs for manager
/// endpoints. Cloud-presigned absolute URLs remain unchanged.
pub fn resolve_envelope_urls(envelope: &mut Envelope, base: &url::Url) {
    let origin = base.origin().ascii_serialization();
    let resolve = |target: &mut String| {
        if target.starts_with('/') {
            *target = format!("{origin}{target}");
        }
    };

    resolve(&mut envelope.response_handling.submit_response_url);
    if let alien_core::presigned::PresignedRequestBackend::Http { url, .. } =
        &mut envelope.response_handling.storage_upload_request.backend
    {
        resolve(url);
    }
    if let alien_core::commands_types::BodySpec::Storage {
        storage_get_request: Some(request),
        ..
    } = &mut envelope.params
    {
        if let alien_core::presigned::PresignedRequestBackend::Http { url, .. } =
            &mut request.backend
        {
            resolve(url);
        }
    }
}
