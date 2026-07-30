//! Bulk registration of Azure [`crate::ResourceImporter`] implementations.
//!
//! See [`crate::aws_importers`] for the parent doc. `compute-cluster`
//! intentionally lives in `alien-platform-controllers`.
//!
//! Azure has additional **auxiliary** resources that the preflight stamps
//! into the stack — `azure_resource_group`, `azure_storage_account`,
//! `azure_container_apps_environment`, `azure_service_bus_namespace` — which
//! get their own importers as well. They never appear in the user-authored
//! stack but the typed payloads must round-trip the same as the rest.

#[cfg(feature = "kubernetes")]
use alien_core::KubernetesCluster;
use alien_core::{
    Ai, ArtifactRegistry, AzureContainerAppsEnvironment, AzureResourceGroup,
    AzureServiceBusNamespace, AzureStorageAccount, Build, Kv, Network, Platform, Queue,
    RemoteStackManagement, ServiceAccount, ServiceActivation, Storage, Vault, Worker,
};

use crate::ai::AzureAiImporter;
use crate::artifact_registry::AzureArtifactRegistryImporter;
use crate::build::AzureBuildImporter;
#[cfg(feature = "kubernetes")]
use crate::kubernetes_cluster::KubernetesClusterImporter;
use crate::kv::AzureKvImporter;
use crate::network::AzureNetworkImporter;
use crate::queue::AzureQueueImporter;
use crate::remote_stack_management::AzureRemoteStackManagementImporter;
use crate::service_account::AzureServiceAccountImporter;
use crate::service_activation::AzureServiceActivationImporter;
use crate::storage::{
    AzureContainerAppsEnvironmentImporter, AzureResourceGroupImporter,
    AzureServiceBusNamespaceImporter, AzureStorageAccountImporter, AzureStorageImporter,
};
use crate::vault::AzureVaultImporter;
use crate::worker::AzureWorkerImporter;
use crate::ImporterRegistry;

/// Register every OSS Azure importer with `registry`.
pub fn register(registry: &mut ImporterRegistry) {
    registry
        // Main resources
        .register(Ai::RESOURCE_TYPE, Platform::Azure, AzureAiImporter)
        .register(
            Storage::RESOURCE_TYPE,
            Platform::Azure,
            AzureStorageImporter,
        )
        .register(Kv::RESOURCE_TYPE, Platform::Azure, AzureKvImporter)
        .register(Vault::RESOURCE_TYPE, Platform::Azure, AzureVaultImporter)
        .register(Queue::RESOURCE_TYPE, Platform::Azure, AzureQueueImporter)
        .register(
            Network::RESOURCE_TYPE,
            Platform::Azure,
            AzureNetworkImporter,
        )
        .register(
            ServiceAccount::RESOURCE_TYPE,
            Platform::Azure,
            AzureServiceAccountImporter,
        )
        .register(
            RemoteStackManagement::RESOURCE_TYPE,
            Platform::Azure,
            AzureRemoteStackManagementImporter,
        )
        .register(Build::RESOURCE_TYPE, Platform::Azure, AzureBuildImporter)
        .register(
            ArtifactRegistry::RESOURCE_TYPE,
            Platform::Azure,
            AzureArtifactRegistryImporter,
        )
        .register(Worker::RESOURCE_TYPE, Platform::Azure, AzureWorkerImporter)
        .register(
            ServiceActivation::RESOURCE_TYPE,
            Platform::Azure,
            AzureServiceActivationImporter,
        )
        // Auxiliary preflight-injected resources
        .register(
            AzureResourceGroup::RESOURCE_TYPE,
            Platform::Azure,
            AzureResourceGroupImporter,
        )
        .register(
            AzureStorageAccount::RESOURCE_TYPE,
            Platform::Azure,
            AzureStorageAccountImporter,
        )
        .register(
            AzureContainerAppsEnvironment::RESOURCE_TYPE,
            Platform::Azure,
            AzureContainerAppsEnvironmentImporter,
        )
        .register(
            AzureServiceBusNamespace::RESOURCE_TYPE,
            Platform::Azure,
            AzureServiceBusNamespaceImporter,
        );
    #[cfg(feature = "kubernetes")]
    registry.register(
        KubernetesCluster::RESOURCE_TYPE,
        Platform::Azure,
        KubernetesClusterImporter,
    );
}
