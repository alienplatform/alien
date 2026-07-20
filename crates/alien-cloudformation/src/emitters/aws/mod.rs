//! AWS CloudFormation emitters.
//!
//! One file per resource type. Shared helpers live in [`helpers`].
//!
//! Built-ins are wired through [`crate::CfRegistry::built_in`]; plugins
//! register additional implementations against the same registry.

pub mod artifact_registry;
pub mod build;
pub mod email;
pub mod helpers;
pub mod kubernetes_cluster;
pub mod kv;
pub mod network;
pub mod open_search;
pub mod queue;
pub mod remote_stack_management;
pub mod service_account;
pub mod storage;
pub mod vault;
pub mod worker;

pub use artifact_registry::AwsArtifactRegistryEmitter;
pub use build::AwsBuildEmitter;
pub use email::AwsEmailEmitter;
pub use kubernetes_cluster::AwsKubernetesClusterEmitter;
pub use kv::AwsKvEmitter;
pub use network::AwsNetworkEmitter;
pub use open_search::AwsOpenSearchEmitter;
pub use queue::AwsQueueEmitter;
pub use remote_stack_management::AwsRemoteStackManagementEmitter;
pub use service_account::AwsServiceAccountEmitter;
pub use storage::AwsStorageEmitter;
pub use vault::AwsVaultEmitter;
pub use worker::AwsWorkerEmitter;
