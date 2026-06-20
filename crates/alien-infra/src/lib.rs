mod core;
pub use core::*;

mod error;
pub use error::*;

mod worker;
pub use worker::*;

mod daemon;
#[cfg(any(feature = "kubernetes", feature = "local"))]
pub use daemon::*;

mod storage;
pub use storage::*;

mod build;
pub use build::*;

mod artifact_registry;
pub use artifact_registry::*;

mod infra_requirements;
pub use infra_requirements::*;

mod service_account;
pub use service_account::*;

mod remote_stack_management;
pub use remote_stack_management::*;

mod vault;
pub use vault::*;

mod network;
pub use network::*;

mod compute_cluster;
#[cfg(feature = "local")]
pub use compute_cluster::*;

mod kubernetes_cluster;

#[cfg(feature = "kubernetes")]
mod kubernetes_client;
#[cfg(feature = "kubernetes")]
mod kubernetes_cluster_heartbeat;
#[cfg(feature = "kubernetes")]
mod kubernetes_public_endpoint;
#[cfg(feature = "kubernetes")]
mod kubernetes_registry;
#[cfg(feature = "kubernetes")]
mod kubernetes_workload_heartbeat;

mod container;
#[cfg(any(feature = "kubernetes", feature = "local"))]
pub use container::*;

mod kv;
#[cfg(feature = "aws")]
pub use kv::AwsKvImporter;
#[cfg(feature = "local")]
pub use kv::LocalKvController;
pub use kv::{AwsKvController, AzureKvController, AzureKvImporter, GcpKvController, GcpKvImporter};

mod service_activation;
pub use service_activation::{
    AzureServiceActivationController, AzureServiceActivationImporter,
    GcpServiceActivationController, GcpServiceActivationImporter,
};

mod queue;

mod import;
pub use import::*;

pub mod import_helpers;

#[cfg(feature = "aws")]
mod aws_importers;
#[cfg(feature = "aws")]
pub mod aws_sdk;
#[cfg(feature = "azure")]
mod azure_importers;
#[cfg(feature = "gcp")]
mod gcp_importers;

mod remote_access_resolver;
pub use remote_access_resolver::*;

// Re-export from alien-client-config for backwards compatibility
pub use alien_client_config::ClientConfigExt;
pub use alien_core::{ClientConfig, ImpersonationConfig};

#[cfg(feature = "kubernetes")]
mod kubeconfig;
#[cfg(feature = "kubernetes")]
pub use kubeconfig::*;

// Test utilities
#[cfg(any(feature = "test-utils", doc, test))]
pub use core::controller_test;
#[cfg(any(feature = "test-utils", doc, test))]
pub use core::MockPlatformServiceProvider;

#[cfg(feature = "azure")]
pub use alien_azure_clients::AzureClientConfig;
#[cfg(feature = "aws")]
pub use alien_core::AwsClientConfig;
#[cfg(feature = "gcp")]
pub use alien_gcp_clients::GcpClientConfig;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
