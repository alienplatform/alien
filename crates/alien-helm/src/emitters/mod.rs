//! Per-resource Helm emitters, all keyed on `Platform::Kubernetes`.
//!
//! Helm is K8s-native: a single chart that runs on EKS / GKE / AKS or a
//! plain on-prem cluster. Cloud-side bindings (AWS S3, GCP Storage, …)
//! land via `infrastructure.<id>.service` so the chart's controllers can
//! consume them without needing a separate cloud SDK call. Per-cloud
//! identity (IRSA / Workload Identity / Federated Identity) is layered
//! on via the chart's `examples/<target>.yaml` files.

pub mod artifact_registry;
pub mod build;
pub mod kv;
pub mod queue;
pub mod service_account;
pub mod storage;
pub mod vault;
pub mod worker;

use crate::registry::HelmRegistry;
use alien_core::{
    ArtifactRegistry, Build, Kv, Platform, Queue, ServiceAccount, Storage, Vault, Worker,
};

/// Wire every built-in K8s Helm emitter into `registry`.
pub fn register_built_ins(registry: &mut HelmRegistry) {
    let p = Platform::Kubernetes;
    registry.register(Storage::RESOURCE_TYPE, p, storage::StorageEmitter);
    registry.register(Queue::RESOURCE_TYPE, p, queue::QueueEmitter);
    registry.register(Kv::RESOURCE_TYPE, p, kv::KvEmitter);
    registry.register(Vault::RESOURCE_TYPE, p, vault::VaultEmitter);
    registry.register(
        ArtifactRegistry::RESOURCE_TYPE,
        p,
        artifact_registry::ArtifactRegistryEmitter,
    );
    registry.register(Build::RESOURCE_TYPE, p, build::BuildEmitter);
    registry.register(Worker::RESOURCE_TYPE, p, worker::WorkerEmitter);
    registry.register(
        ServiceAccount::RESOURCE_TYPE,
        p,
        service_account::ServiceAccountEmitter,
    );
}
