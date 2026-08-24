//! GCP Terraform emitters.
//!
//! One sub-module per resource. Each emitter `impl crate::TfEmitter` and
//! returns `hcl::Block` / `hcl::Expression` directly. Shared helpers
//! (downcast, labels, IAM member binding, service-account email
//! resolution) live in [`helpers`].

pub mod ai;
pub mod artifact_registry;
pub mod build;
pub mod helpers;
pub mod key;
pub mod kv;
pub mod network;
pub mod queue;
pub mod remote_bindings;
pub mod remote_stack_management;
pub mod sandbox;
pub mod service_account;
pub mod service_activation;
pub mod storage;
pub mod vault;
pub mod worker;

pub use ai::GcpAiEmitter;
pub use artifact_registry::GcpArtifactRegistryEmitter;
pub use build::GcpBuildEmitter;
pub use key::GcpKeyEmitter;
pub use kv::GcpKvEmitter;
pub use network::GcpNetworkEmitter;
pub use queue::GcpQueueEmitter;
pub use remote_bindings::GcpRemoteBindingsEmitter;
pub use remote_stack_management::GcpRemoteStackManagementEmitter;
pub use sandbox::GcpAgentPlatformSandboxEmitter;
pub use service_account::GcpServiceAccountEmitter;
pub use service_activation::GcpServiceActivationEmitter;
pub use storage::GcpStorageEmitter;
pub use vault::GcpVaultEmitter;
pub use worker::GcpWorkerEmitter;
