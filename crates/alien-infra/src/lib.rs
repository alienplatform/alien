mod core;
pub use core::*;

mod error;
pub use error::*;

mod function;
pub use function::*;

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

mod container_cluster;
pub use container_cluster::*;

mod container;
pub use container::*;

mod kv;
pub use kv::*;

mod service_activation;
pub use service_activation::*;

mod queue;

pub mod cloudformation;
pub use cloudformation::import_stack_state_from_cloudformation;

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

#[cfg(feature = "aws")]
pub use alien_aws_clients::AwsClientConfig;
#[cfg(feature = "azure")]
pub use alien_azure_clients::AzureClientConfig;
#[cfg(feature = "gcp")]
pub use alien_gcp_clients::GcpClientConfig;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
