//! Container binding implementations
//!
//! This module provides container implementations for different platforms:
//! - Managed cloud: For containers on AWS/GCP/Azure
//! - Local: For containers running in Docker during local development
//! - Kubernetes: For containers running as Kubernetes Services

mod horizon;
mod local;

#[cfg(feature = "kubernetes")]
mod kubernetes;

pub use horizon::HorizonContainer;
pub use local::LocalContainer;

#[cfg(feature = "kubernetes")]
pub use kubernetes::KubernetesContainer;
