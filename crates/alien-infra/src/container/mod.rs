//! Container resource controllers.
//!
//! This module provides controllers for managing Container resources
//! across different cloud platforms. Containers are scheduled by Horizon
//! on cloud platforms, and run via Docker on Local platform.

#[cfg(feature = "aws")]
mod aws;
#[cfg(feature = "aws")]
pub use aws::*;

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::*;

#[cfg(feature = "kubernetes")]
mod kubernetes;
#[cfg(feature = "kubernetes")]
pub use kubernetes::*;

#[cfg(feature = "gcp")]
mod gcp;
#[cfg(feature = "gcp")]
pub use gcp::*;

#[cfg(feature = "azure")]
mod azure;
#[cfg(feature = "azure")]
pub use azure::*;

mod local_utils;
