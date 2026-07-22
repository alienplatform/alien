//! Built-in resource definitions.
//!
//! Resource type identifiers are stable strings (`"storage"`, `"queue"`, …).
//! Provider-specific resources that don't abstract over clouds live under the
//! `experimental/` namespace (e.g. `experimental/aws-opensearch`); they are
//! only registered for the platforms they support and may change in breaking
//! ways before being promoted to a portable resource.

mod worker;
pub use worker::*;

mod public_endpoint;
pub use public_endpoint::*;

mod daemon;
pub use daemon::*;

mod email;
pub use email::*;

mod storage;
pub use storage::*;

mod queue;
pub use queue::*;

mod build;
pub use build::*;

mod artifact_registry;
pub use artifact_registry::*;

mod aws_open_search;
pub use aws_open_search::*;

mod service_activation;
pub use service_activation::*;

mod azure_storage_account;
pub use azure_storage_account::*;

mod azure_resource_group;
pub use azure_resource_group::*;

mod azure_container_apps_environment;
pub use azure_container_apps_environment::*;

mod azure_service_bus_namespace;
pub use azure_service_bus_namespace::*;

mod service_account;
pub use service_account::*;

mod remote_stack_management;
pub use remote_stack_management::*;

mod vault;
pub use vault::*;

mod kv;
pub use kv::*;

mod ai;
pub use ai::*;

mod network;
pub use network::*;

mod compute_cluster;
pub use compute_cluster::*;

mod kubernetes_cluster;
pub use kubernetes_cluster::*;

mod container;
pub use container::*;

mod postgres;
pub use postgres::*;
