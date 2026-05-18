//! Azure Terraform emitters.
//!
//! One sub-module per Alien resource type, plus four auxiliary modules
//! ([`resource_group`], [`storage_account`], [`container_apps_environment`],
//! [`service_bus_namespace`]) for the Azure-specific parent infrastructure
//! the preflight pipeline injects. Each emitter `impl crate::TfEmitter`
//! and returns `hcl::Block` / `hcl::Expression` directly. Shared helpers
//! (downcast, tags, IAM principal_id resolution, role-definition
//! emission) live in [`helpers`].
//!
//! Per-resource design notes cover storage-account naming convergence,
//! cross-tenant federated-identity trust, and AKS overlay activation.

pub mod artifact_registry;
pub mod build;
pub mod container_apps_environment;
pub mod helpers;
pub mod kv;
pub mod network;
pub mod queue;
pub mod remote_stack_management;
pub mod resource_group;
pub mod service_account;
pub mod service_activation;
pub mod service_bus_namespace;
pub mod storage;
pub mod storage_account;
pub mod vault;
pub mod worker;

pub use artifact_registry::AzureArtifactRegistryEmitter;
pub use build::AzureBuildEmitter;
pub use container_apps_environment::AzureContainerAppsEnvironmentEmitter;
pub use kv::AzureKvEmitter;
pub use network::AzureNetworkEmitter;
pub use queue::AzureQueueEmitter;
pub use remote_stack_management::AzureRemoteStackManagementEmitter;
pub use resource_group::AzureResourceGroupEmitter;
pub use service_account::AzureServiceAccountEmitter;
pub use service_activation::AzureServiceActivationEmitter;
pub use service_bus_namespace::AzureServiceBusNamespaceEmitter;
pub use storage::AzureStorageEmitter;
pub use storage_account::AzureStorageAccountEmitter;
pub use vault::AzureVaultEmitter;
pub use worker::AzureWorkerEmitter;
