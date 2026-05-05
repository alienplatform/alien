//! Built-in Terraform emitter registrations.
//!
//! Wired through [`crate::TfRegistry::built_in`]. Listed explicitly so the
//! set of registered `(ResourceType, Platform)` pairs is grep-able.

use crate::registry::TfRegistry;
use alien_core::{
    ArtifactRegistry, AzureContainerAppsEnvironment, AzureResourceGroup, AzureServiceBusNamespace,
    AzureStorageAccount, Build, Function, Kv, Network, Platform, Queue, RemoteStackManagement,
    ServiceAccount, ServiceActivation, Storage, Vault,
};

pub(crate) fn register_all(registry: &mut TfRegistry) {
    register_aws(registry);
    register_gcp(registry);
    register_azure(registry);
}

fn register_aws(registry: &mut TfRegistry) {
    use crate::emitters::aws;
    let p = Platform::Aws;
    registry.register(Storage::RESOURCE_TYPE, p, aws::AwsStorageEmitter);
    registry.register(Kv::RESOURCE_TYPE, p, aws::AwsKvEmitter);
    registry.register(Queue::RESOURCE_TYPE, p, aws::AwsQueueEmitter);
    registry.register(Vault::RESOURCE_TYPE, p, aws::AwsVaultEmitter);
    registry.register(Network::RESOURCE_TYPE, p, aws::AwsNetworkEmitter);
    registry.register(
        ServiceAccount::RESOURCE_TYPE,
        p,
        aws::AwsServiceAccountEmitter,
    );
    registry.register(
        RemoteStackManagement::RESOURCE_TYPE,
        p,
        aws::AwsRemoteStackManagementEmitter,
    );
    registry.register(
        ArtifactRegistry::RESOURCE_TYPE,
        p,
        aws::AwsArtifactRegistryEmitter,
    );
    registry.register(Build::RESOURCE_TYPE, p, aws::AwsBuildEmitter);
    registry.register(Function::RESOURCE_TYPE, p, aws::AwsFunctionEmitter);
}

fn register_gcp(registry: &mut TfRegistry) {
    use crate::emitters::gcp;
    let p = Platform::Gcp;
    registry.register(Storage::RESOURCE_TYPE, p, gcp::GcpStorageEmitter);
    registry.register(Kv::RESOURCE_TYPE, p, gcp::GcpKvEmitter);
    registry.register(Queue::RESOURCE_TYPE, p, gcp::GcpQueueEmitter);
    registry.register(Vault::RESOURCE_TYPE, p, gcp::GcpVaultEmitter);
    registry.register(Network::RESOURCE_TYPE, p, gcp::GcpNetworkEmitter);
    registry.register(
        ServiceAccount::RESOURCE_TYPE,
        p,
        gcp::GcpServiceAccountEmitter,
    );
    registry.register(
        RemoteStackManagement::RESOURCE_TYPE,
        p,
        gcp::GcpRemoteStackManagementEmitter,
    );
    registry.register(
        ArtifactRegistry::RESOURCE_TYPE,
        p,
        gcp::GcpArtifactRegistryEmitter,
    );
    registry.register(Build::RESOURCE_TYPE, p, gcp::GcpBuildEmitter);
    registry.register(Function::RESOURCE_TYPE, p, gcp::GcpFunctionEmitter);
    registry.register(
        ServiceActivation::RESOURCE_TYPE,
        p,
        gcp::GcpServiceActivationEmitter,
    );
}

fn register_azure(registry: &mut TfRegistry) {
    use crate::emitters::azure;
    let p = Platform::Azure;

    // Main resources — one emitter per Alien resource type.
    registry.register(Storage::RESOURCE_TYPE, p, azure::AzureStorageEmitter);
    registry.register(Kv::RESOURCE_TYPE, p, azure::AzureKvEmitter);
    registry.register(Queue::RESOURCE_TYPE, p, azure::AzureQueueEmitter);
    registry.register(Vault::RESOURCE_TYPE, p, azure::AzureVaultEmitter);
    registry.register(Network::RESOURCE_TYPE, p, azure::AzureNetworkEmitter);
    registry.register(
        ServiceAccount::RESOURCE_TYPE,
        p,
        azure::AzureServiceAccountEmitter,
    );
    registry.register(
        RemoteStackManagement::RESOURCE_TYPE,
        p,
        azure::AzureRemoteStackManagementEmitter,
    );
    registry.register(
        ArtifactRegistry::RESOURCE_TYPE,
        p,
        azure::AzureArtifactRegistryEmitter,
    );
    registry.register(Build::RESOURCE_TYPE, p, azure::AzureBuildEmitter);
    registry.register(Function::RESOURCE_TYPE, p, azure::AzureFunctionEmitter);
    registry.register(
        ServiceActivation::RESOURCE_TYPE,
        p,
        azure::AzureServiceActivationEmitter,
    );

    // Auxiliary resources — preflight-injected, one shot per stack.
    // These are not part of the user-facing resource catalogue but the
    // generator needs an emitter for every resource type that lands in
    // the stack (otherwise `TfRegistry::require` returns
    // `ImportRegistrationMissing`).
    registry.register(
        AzureResourceGroup::RESOURCE_TYPE,
        p,
        azure::AzureResourceGroupEmitter,
    );
    registry.register(
        AzureStorageAccount::RESOURCE_TYPE,
        p,
        azure::AzureStorageAccountEmitter,
    );
    registry.register(
        AzureContainerAppsEnvironment::RESOURCE_TYPE,
        p,
        azure::AzureContainerAppsEnvironmentEmitter,
    );
    registry.register(
        AzureServiceBusNamespace::RESOURCE_TYPE,
        p,
        azure::AzureServiceBusNamespaceEmitter,
    );
}
