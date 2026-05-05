//! AWS CloudFormation emitters.
//!
//! One file per resource type. Shared helpers live in [`helpers`].
//!
//! Built-ins are wired through [`crate::CfRegistry::built_in`]; plugins
//! register additional implementations against the same registry.

pub mod artifact_registry;
pub mod build;
pub mod function;
pub mod helpers;
pub mod kv;
pub mod network;
pub mod queue;
pub mod remote_stack_management;
pub mod service_account;
pub mod storage;
pub mod vault;

pub use artifact_registry::AwsArtifactRegistryEmitter;
pub use build::AwsBuildEmitter;
pub use function::AwsFunctionEmitter;
pub use kv::AwsKvEmitter;
pub use network::AwsNetworkEmitter;
pub use queue::AwsQueueEmitter;
pub use remote_stack_management::AwsRemoteStackManagementEmitter;
pub use service_account::AwsServiceAccountEmitter;
pub use storage::AwsStorageEmitter;
pub use vault::AwsVaultEmitter;
