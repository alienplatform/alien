//! Built-in AWS CloudFormation emitters.
//!
//! Wired through [`crate::registry::CfRegistry::built_in`]. Listed explicitly
//! so the set of registered `(ResourceType, Platform)` pairs is grep-able.

use crate::{
    emitters::aws::{
        AwsArtifactRegistryEmitter, AwsBuildEmitter, AwsFunctionEmitter, AwsKvEmitter,
        AwsNetworkEmitter, AwsQueueEmitter, AwsRemoteStackManagementEmitter,
        AwsServiceAccountEmitter, AwsStorageEmitter, AwsVaultEmitter,
    },
    registry::CfRegistry,
};
use alien_core::{
    ArtifactRegistry, Build, Function, Kv, Network, Platform, Queue, RemoteStackManagement,
    ResourceType, ServiceAccount, Storage, Vault,
};

pub(crate) fn register_aws(registry: &mut CfRegistry) {
    fn aws<E>(registry: &mut CfRegistry, resource_type: ResourceType, emitter: E)
    where
        E: crate::emitter::CfEmitter + 'static,
    {
        registry.register(resource_type, Platform::Aws, emitter);
    }

    aws(registry, Storage::RESOURCE_TYPE, AwsStorageEmitter);
    aws(registry, Kv::RESOURCE_TYPE, AwsKvEmitter);
    aws(registry, Queue::RESOURCE_TYPE, AwsQueueEmitter);
    aws(registry, Vault::RESOURCE_TYPE, AwsVaultEmitter);
    aws(registry, Function::RESOURCE_TYPE, AwsFunctionEmitter);
    aws(registry, Build::RESOURCE_TYPE, AwsBuildEmitter);
    aws(
        registry,
        ArtifactRegistry::RESOURCE_TYPE,
        AwsArtifactRegistryEmitter,
    );
    aws(registry, Network::RESOURCE_TYPE, AwsNetworkEmitter);
    aws(
        registry,
        ServiceAccount::RESOURCE_TYPE,
        AwsServiceAccountEmitter,
    );
    aws(
        registry,
        RemoteStackManagement::RESOURCE_TYPE,
        AwsRemoteStackManagementEmitter,
    );
}
