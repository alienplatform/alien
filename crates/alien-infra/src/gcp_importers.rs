//! Bulk registration of GCP [`crate::ResourceImporter`] implementations.
//!
//! See [`crate::aws_importers`] for the parent doc. `compute-cluster`
//! intentionally lives in `alien-platform-controllers`.

use alien_core::{ArtifactRegistry, Build, Worker, Kv, Network, Platform, Queue, Storage, Vault};
use alien_core::{RemoteStackManagement, ServiceAccount, ServiceActivation};

use crate::artifact_registry::GcpArtifactRegistryImporter;
use crate::build::GcpBuildImporter;
use crate::worker::GcpWorkerImporter;
use crate::kv::GcpKvImporter;
use crate::network::GcpNetworkImporter;
use crate::queue::GcpQueueImporter;
use crate::remote_stack_management::GcpRemoteStackManagementImporter;
use crate::service_account::GcpServiceAccountImporter;
use crate::service_activation::GcpServiceActivationImporter;
use crate::storage::GcpStorageImporter;
use crate::vault::GcpVaultImporter;
use crate::ImporterRegistry;

/// Register every OSS GCP importer with `registry`.
pub fn register(registry: &mut ImporterRegistry) {
    registry
        .register(Storage::RESOURCE_TYPE, Platform::Gcp, GcpStorageImporter)
        .register(Kv::RESOURCE_TYPE, Platform::Gcp, GcpKvImporter)
        .register(Vault::RESOURCE_TYPE, Platform::Gcp, GcpVaultImporter)
        .register(Queue::RESOURCE_TYPE, Platform::Gcp, GcpQueueImporter)
        .register(Network::RESOURCE_TYPE, Platform::Gcp, GcpNetworkImporter)
        .register(
            ServiceAccount::RESOURCE_TYPE,
            Platform::Gcp,
            GcpServiceAccountImporter,
        )
        .register(
            RemoteStackManagement::RESOURCE_TYPE,
            Platform::Gcp,
            GcpRemoteStackManagementImporter,
        )
        .register(Build::RESOURCE_TYPE, Platform::Gcp, GcpBuildImporter)
        .register(
            ArtifactRegistry::RESOURCE_TYPE,
            Platform::Gcp,
            GcpArtifactRegistryImporter,
        )
        .register(Worker::RESOURCE_TYPE, Platform::Gcp, GcpWorkerImporter)
        .register(
            ServiceActivation::RESOURCE_TYPE,
            Platform::Gcp,
            GcpServiceActivationImporter,
        );
}
