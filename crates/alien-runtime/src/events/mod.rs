//! Platform-specific event parsing for alien-runtime
//!
//! This module contains event parsing logic that converts platform-specific
//! event formats (AWS Lambda events, GCP CloudEvents, Azure Dapr events) into
//! standardized alien-core event types.

#[cfg(feature = "aws")]
pub mod aws;
#[cfg(feature = "aws")]
pub use aws::*;

#[cfg(feature = "gcp")]
pub mod gcp;
#[cfg(feature = "gcp")]
pub use gcp::*;

#[cfg(feature = "azure")]
pub mod azure;
#[cfg(feature = "azure")]
pub use azure::*;
