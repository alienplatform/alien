//! AWS Terraform emitters.
//!
//! One sub-module per resource. Each emitter `impl crate::TfEmitter` and
//! returns `hcl::Block` / `hcl::Expression` directly (no intermediate IR).
//! Shared helpers live in [`helpers`].

pub mod artifact_registry;
pub mod build;
pub mod helpers;
pub mod kv;
pub mod network;
pub mod queue;
pub mod remote_stack_management;
pub mod service_account;
pub mod storage;
pub mod vault;
pub mod worker;

pub use artifact_registry::AwsArtifactRegistryEmitter;
pub use build::AwsBuildEmitter;
pub use kv::AwsKvEmitter;
pub use network::AwsNetworkEmitter;
pub use queue::AwsQueueEmitter;
pub use remote_stack_management::AwsRemoteStackManagementEmitter;
pub use service_account::AwsServiceAccountEmitter;
pub use storage::AwsStorageEmitter;
pub use vault::AwsVaultEmitter;
pub use worker::AwsWorkerEmitter;
