//! Bulk registration of AWS [`crate::ResourceImporter`] implementations.
//!
//! Called by [`crate::ImporterRegistry::built_in`]. Each `(ResourceType,
//! Platform::Aws)` pair maps to one of the per-resource importers under
//! `crate::<resource>::Aws<Resource>Importer`.
//!
//! `compute-cluster` is intentionally absent — that controller lives in
//! `alien-platform-controllers` (per the OSS / platform split) and is added
//! by `register_platform_importers` at boot.

#[cfg(feature = "kubernetes")]
use alien_core::KubernetesCluster;
use alien_core::{
    Ai, ArtifactRegistry, AwsOpenSearch, Build, Email, Kv, Network, Platform, Queue, Storage, Vault,
    Worker,
};
use alien_core::{RemoteStackManagement, ServiceAccount};

use crate::ai::AwsAiImporter;
use crate::artifact_registry::AwsArtifactRegistryImporter;
use crate::build::AwsBuildImporter;
use crate::email::AwsEmailImporter;
#[cfg(feature = "kubernetes")]
use crate::kubernetes_cluster::KubernetesClusterImporter;
use crate::kv::AwsKvImporter;
use crate::network::AwsNetworkImporter;
use crate::open_search::AwsOpenSearchImporter;
use crate::queue::AwsQueueImporter;
use crate::remote_stack_management::AwsRemoteStackManagementImporter;
use crate::service_account::AwsServiceAccountImporter;
use crate::storage::AwsStorageImporter;
use crate::vault::AwsVaultImporter;
use crate::worker::AwsWorkerImporter;
use crate::ImporterRegistry;

/// Register every OSS AWS importer with `registry`.
pub fn register(registry: &mut ImporterRegistry) {
    registry
        .register(Ai::RESOURCE_TYPE, Platform::Aws, AwsAiImporter)
        .register(Storage::RESOURCE_TYPE, Platform::Aws, AwsStorageImporter)
        .register(Kv::RESOURCE_TYPE, Platform::Aws, AwsKvImporter)
        .register(Vault::RESOURCE_TYPE, Platform::Aws, AwsVaultImporter)
        .register(Queue::RESOURCE_TYPE, Platform::Aws, AwsQueueImporter)
        .register(Network::RESOURCE_TYPE, Platform::Aws, AwsNetworkImporter)
        .register(
            ServiceAccount::RESOURCE_TYPE,
            Platform::Aws,
            AwsServiceAccountImporter,
        )
        .register(
            RemoteStackManagement::RESOURCE_TYPE,
            Platform::Aws,
            AwsRemoteStackManagementImporter,
        )
        .register(Build::RESOURCE_TYPE, Platform::Aws, AwsBuildImporter)
        .register(
            ArtifactRegistry::RESOURCE_TYPE,
            Platform::Aws,
            AwsArtifactRegistryImporter,
        )
        .register(Worker::RESOURCE_TYPE, Platform::Aws, AwsWorkerImporter)
        .register(Email::RESOURCE_TYPE, Platform::Aws, AwsEmailImporter)
        .register(
            AwsOpenSearch::RESOURCE_TYPE,
            Platform::Aws,
            AwsOpenSearchImporter,
        );
    #[cfg(feature = "kubernetes")]
    registry.register(
        KubernetesCluster::RESOURCE_TYPE,
        Platform::Aws,
        KubernetesClusterImporter,
    );
}
