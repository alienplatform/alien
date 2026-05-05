//! GCP Terraform emitters.
//!
//! One sub-module per resource. Each emitter `impl crate::TfEmitter` and
//! returns `hcl::Block` / `hcl::Expression` directly. Shared helpers
//! (downcast, labels, IAM member binding, service-account email
//! resolution) live in [`helpers`].

pub mod artifact_registry;
pub mod build;
pub mod function;
pub mod helpers;
pub mod kv;
pub mod network;
pub mod queue;
pub mod remote_stack_management;
pub mod service_account;
pub mod service_activation;
pub mod storage;
pub mod vault;

pub use artifact_registry::GcpArtifactRegistryEmitter;
pub use build::GcpBuildEmitter;
pub use function::GcpFunctionEmitter;
pub use kv::GcpKvEmitter;
pub use network::GcpNetworkEmitter;
pub use queue::GcpQueueEmitter;
pub use remote_stack_management::GcpRemoteStackManagementEmitter;
pub use service_account::GcpServiceAccountEmitter;
pub use service_activation::GcpServiceActivationEmitter;
pub use storage::GcpStorageEmitter;
pub use vault::GcpVaultEmitter;
