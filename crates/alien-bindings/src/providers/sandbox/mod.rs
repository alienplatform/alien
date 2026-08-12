//! Sandbox binding providers.
//!
//! Per-cloud backends land with their controllers. Local is here because it speaks the same
//! authenticated transport the cloud backends do, so it exercises the real path rather than a
//! shortcut.

#[cfg(any(feature = "aws", feature = "kubernetes"))]
pub mod agent_protocol;

#[cfg(feature = "aws")]
pub mod aws;

#[cfg(feature = "azure")]
pub mod azure;

#[cfg(feature = "gcp")]
pub mod gcp;

#[cfg(feature = "kubernetes")]
pub mod kubernetes;

#[cfg(feature = "local")]
pub mod local;
