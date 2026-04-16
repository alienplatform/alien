use std::collections::HashMap;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::OnceLock;

use crate::core::ResourceController;
use crate::error::{ErrorData, Result};
use alien_core::{
    ArtifactRegistry, AzureContainerAppsEnvironment, AzureResourceGroup, AzureServiceBusNamespace,
    AzureStorageAccount, Build, Container, ContainerCluster, Function, Kv, Network,
    RemoteStackManagement, ServiceAccount, ServiceActivation, Storage, Vault,
};
use alien_core::{Platform, ResourceDefinition, ResourceType};
use alien_error::AlienError;

/// Type for a callback that registers additional controller factories into a ResourceRegistry.
pub type RegistryExtensionCallback = Box<dyn Fn(&mut ResourceRegistry) + Send + Sync>;

/// Global registry extension callback, set by platform crates.
static REGISTRY_EXTENSION: OnceLock<RegistryExtensionCallback> = OnceLock::new();

/// Registers a callback that will be invoked by `ResourceRegistry::with_built_ins()`
/// to add additional controller factories.
///
/// Must be called before any `ResourceRegistry::with_built_ins()` is called
/// (typically at startup).
pub fn register_registry_extension(callback: RegistryExtensionCallback) {
    REGISTRY_EXTENSION.set(callback).ok();
}

/// Factory trait for creating resource controllers
pub trait ControllerFactory: Send + Sync + Debug {
    /// Creates a new instance of the controller
    fn create(&self) -> Box<dyn ResourceController>;
}

/// Factory trait for creating CloudFormation resource importers
pub trait CloudFormationImporterFactory: Send + Sync + Debug {
    /// Creates a new instance of the CloudFormation importer
    fn create(&self) -> Box<dyn crate::cloudformation::traits::CloudFormationResourceImporter>;
}

/// A factory implementation for controllers that implement Default
#[derive(Debug)]
pub struct DefaultControllerFactory<T> {
    _phantom: PhantomData<T>,
}

impl<T> DefaultControllerFactory<T> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<T> Default for DefaultControllerFactory<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ControllerFactory for DefaultControllerFactory<T>
where
    T: ResourceController + Default + 'static,
{
    fn create(&self) -> Box<dyn ResourceController> {
        Box::new(T::default())
    }
}

/// A factory implementation for CloudFormation importers that implement Default
#[derive(Debug)]
pub struct DefaultCloudFormationImporterFactory<T> {
    _phantom: PhantomData<T>,
}

impl<T> Default for DefaultCloudFormationImporterFactory<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> DefaultCloudFormationImporterFactory<T> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<T> CloudFormationImporterFactory for DefaultCloudFormationImporterFactory<T>
where
    T: crate::cloudformation::traits::CloudFormationResourceImporter
        + Default
        + Send
        + Sync
        + std::fmt::Debug
        + 'static,
{
    fn create(&self) -> Box<dyn crate::cloudformation::traits::CloudFormationResourceImporter> {
        Box::new(T::default())
    }
}

/// Default factory for infrastructure requirements providers
#[derive(Debug)]
pub struct DefaultInfraRequirementsProviderFactory<T> {
    _phantom: PhantomData<T>,
}

impl<T> Default for DefaultInfraRequirementsProviderFactory<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> DefaultInfraRequirementsProviderFactory<T> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

/// Registry for Resource types and their controllers
#[derive(Debug)]
pub struct ResourceRegistry {
    /// Maps (resource_type, platform) to controller factories
    controller_factories: HashMap<(ResourceType, Platform), Box<dyn ControllerFactory>>,
    /// Maps (resource_type, platform) to CloudFormation importer factories
    cloudformation_importer_factories:
        HashMap<(ResourceType, Platform), Box<dyn CloudFormationImporterFactory>>,
}

impl Clone for ResourceRegistry {
    fn clone(&self) -> Self {
        // Since the trait objects cannot be cloned directly, we create a new registry with built-ins
        // This is a limitation - custom registrations will be lost on clone
        // This is acceptable for our current use case where we mainly clone built-in registries
        Self::with_built_ins()
    }
}

impl ResourceRegistry {
    /// Creates a new empty resource registry
    pub fn new() -> Self {
        Self {
            controller_factories: HashMap::new(),
            cloudformation_importer_factories: HashMap::new(),
        }
    }

    /// Starts a resource registration builder
    pub fn register<R>(&mut self, resource_type: ResourceType) -> ResourceRegistration<R>
    where
        R: ResourceDefinition + 'static,
    {
        ResourceRegistration::new(self, resource_type)
    }

    /// Gets a controller for the given resource type and platform
    pub fn get_controller(
        &self,
        resource_type: ResourceType,
        platform: Platform,
    ) -> Result<Box<dyn ResourceController>> {
        let key = (resource_type.clone(), platform);
        self.controller_factories
            .get(&key)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ControllerNotAvailable {
                    resource_type: resource_type.clone(),
                    platform,
                })
            })
            .map(|factory| factory.create())
    }

    /// Gets a CloudFormation importer for the given resource type and platform
    pub fn get_cloudformation_importer(
        &self,
        resource_type: ResourceType,
        platform: Platform,
    ) -> Result<Box<dyn crate::cloudformation::traits::CloudFormationResourceImporter>> {
        let key = (resource_type.clone(), platform);
        self.cloudformation_importer_factories
            .get(&key)
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: format!("CloudFormation importer not registered for resource type '{}' on platform {:?}", resource_type, platform),
                operation: Some("get_cloudformation_importer".to_string()),
                resource_id: None,
            }))
            .map(|factory| factory.create())
    }

    /// Registers a controller factory for a specific resource type and platform
    pub fn register_controller_factory(
        &mut self,
        resource_type: ResourceType,
        platform: Platform,
        factory: Box<dyn ControllerFactory>,
    ) {
        let key = (resource_type, platform);
        self.controller_factories.insert(key, factory);
    }

    /// Registers a CloudFormation importer factory for a specific resource type and platform
    pub fn register_cloudformation_importer_factory(
        &mut self,
        resource_type: ResourceType,
        platform: Platform,
        factory: Box<dyn CloudFormationImporterFactory>,
    ) {
        let key = (resource_type, platform);
        self.cloudformation_importer_factories.insert(key, factory);
    }

    /// Creates a default registry with built-in resource types
    pub fn with_built_ins() -> Self {
        let mut registry = Self::new();

        // Register built-in Function controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            Function::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<
                crate::function::AwsFunctionController,
            >::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            Function::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<
                crate::function::GcpFunctionController,
            >::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            Function::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::function::AzureFunctionController,
            >::new()),
        );

        #[cfg(feature = "test")]
        registry.register_controller_factory(
            Function::RESOURCE_TYPE,
            Platform::Test,
            Box::new(DefaultControllerFactory::<
                crate::function::TestFunctionController,
            >::new()),
        );

        // Register Kubernetes Function controller
        #[cfg(feature = "kubernetes")]
        registry.register_controller_factory(
            Function::RESOURCE_TYPE,
            Platform::Kubernetes,
            Box::new(DefaultControllerFactory::<
                crate::function::KubernetesFunctionController,
            >::new()),
        );

        // Register Local Function controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            Function::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<
                crate::function::LocalFunctionController,
            >::new()),
        );

        // Register built-in Storage controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            Storage::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<
                crate::storage::AwsStorageController,
            >::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            Storage::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<
                crate::storage::GcpStorageController,
            >::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            Storage::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::storage::AzureStorageController,
            >::new()),
        );

        #[cfg(feature = "test")]
        registry.register_controller_factory(
            Storage::RESOURCE_TYPE,
            Platform::Test,
            Box::new(DefaultControllerFactory::<
                crate::storage::TestStorageController,
            >::new()),
        );

        // Note: Kubernetes platform uses external bindings for Storage - no controller needed.
        // The executor handles external bindings directly (see executor.rs).

        // Register Local Storage controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            Storage::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<
                crate::storage::LocalStorageController,
            >::new()),
        );

        // Register built-in Queue controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            alien_core::Queue::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<
                crate::queue::aws::AwsQueueController,
            >::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            alien_core::Queue::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<
                crate::queue::gcp::GcpQueueController,
            >::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            alien_core::Queue::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::queue::azure::AzureQueueController,
            >::new()),
        );

        // Register Local Queue controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            alien_core::Queue::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<
                crate::queue::local::LocalQueueController,
            >::new()),
        );

        // Register built-in Build controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            Build::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<crate::build::AwsBuildController>::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            Build::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<crate::build::GcpBuildController>::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            Build::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<crate::build::AzureBuildController>::new()),
        );

        #[cfg(feature = "kubernetes")]
        registry.register_controller_factory(
            Build::RESOURCE_TYPE,
            Platform::Kubernetes,
            Box::new(DefaultControllerFactory::<
                crate::build::KubernetesBuildController,
            >::new()),
        );

        // Register built-in Service Activation controllers
        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            ServiceActivation::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<
                crate::service_activation::GcpServiceActivationController,
            >::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            ServiceActivation::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::service_activation::AzureServiceActivationController,
            >::new()),
        );

        // Register built-in Azure infrastructure requirements controllers
        registry.register_controller_factory(
            AzureResourceGroup::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::infra_requirements::AzureResourceGroupController,
            >::new()),
        );

        registry.register_controller_factory(
            AzureStorageAccount::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::infra_requirements::AzureStorageAccountController,
            >::new()),
        );

        registry.register_controller_factory(
            AzureContainerAppsEnvironment::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::infra_requirements::AzureContainerAppsEnvironmentController,
            >::new()),
        );

        registry.register_controller_factory(
            AzureServiceBusNamespace::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::infra_requirements::AzureServiceBusNamespaceController,
            >::new()),
        );

        // Register built-in RemoteStackManagement controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            RemoteStackManagement::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<
                crate::remote_stack_management::AwsRemoteStackManagementController,
            >::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            RemoteStackManagement::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<
                crate::remote_stack_management::GcpRemoteStackManagementController,
            >::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            RemoteStackManagement::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::remote_stack_management::AzureRemoteStackManagementController,
            >::new()),
        );

        // Register Test RemoteStackManagement controller
        #[cfg(feature = "test")]
        registry.register_controller_factory(
            RemoteStackManagement::RESOURCE_TYPE,
            Platform::Test,
            Box::new(DefaultControllerFactory::<
                crate::remote_stack_management::TestRemoteStackManagementController,
            >::new()),
        );

        // Register built-in CloudFormation importers for AWS
        #[cfg(feature = "aws")]
        {
            registry.register_cloudformation_importer_factory(
                Function::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::function::AwsFunctionCloudFormationImporter,
                >::new()),
            );

            registry.register_cloudformation_importer_factory(
                Storage::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::storage::AwsStorageCloudFormationImporter,
                >::new()),
            );

            registry.register_cloudformation_importer_factory(
                alien_core::Queue::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::queue::templates::AwsQueueCloudFormationImporter,
                >::new()),
            );

            registry.register_cloudformation_importer_factory(
                Build::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::build::AwsBuildCloudFormationImporter,
                >::new()),
            );

            registry.register_cloudformation_importer_factory(
                Kv::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::kv::templates::AwsKvCloudFormationImporter,
                >::new()),
            );
        }

        // Register built-in ArtifactRegistry controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            ArtifactRegistry::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<
                crate::artifact_registry::AwsArtifactRegistryController,
            >::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            ArtifactRegistry::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<
                crate::artifact_registry::GcpArtifactRegistryController,
            >::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            ArtifactRegistry::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::artifact_registry::AzureArtifactRegistryController,
            >::new()),
        );

        // Note: Kubernetes platform uses external bindings for ArtifactRegistry - no controller needed.
        // The executor handles external bindings directly (see executor.rs).

        // Register Local ArtifactRegistry controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            ArtifactRegistry::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<
                crate::artifact_registry::LocalArtifactRegistryController,
            >::new()),
        );

        // Register built-in ArtifactRegistry CloudFormation importers for AWS
        #[cfg(feature = "aws")]
        {
            registry.register_cloudformation_importer_factory(
                ArtifactRegistry::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::artifact_registry::AwsArtifactRegistryCloudFormationImporter,
                >::new()),
            );
        }

        // Register built-in ServiceAccount controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            ServiceAccount::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<
                crate::service_account::AwsServiceAccountController,
            >::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            ServiceAccount::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<
                crate::service_account::GcpServiceAccountController,
            >::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            ServiceAccount::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::service_account::AzureServiceAccountController,
            >::new()),
        );

        // Note: Kubernetes platform does NOT have a ServiceAccount controller
        // ServiceAccounts are created by Helm chart with cloud identity annotations

        // Register Local ServiceAccount controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            ServiceAccount::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<
                crate::service_account::LocalServiceAccountController,
            >::new()),
        );

        // Register Test ServiceAccount controller
        #[cfg(feature = "test")]
        registry.register_controller_factory(
            ServiceAccount::RESOURCE_TYPE,
            Platform::Test,
            Box::new(DefaultControllerFactory::<
                crate::service_account::TestServiceAccountController,
            >::new()),
        );

        // Register built-in ServiceAccount CloudFormation importers
        #[cfg(feature = "aws")]
        {
            registry.register_cloudformation_importer_factory(
                ServiceAccount::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::service_account::AwsServiceAccountCloudFormationImporter,
                >::new()),
            );
        }

        // Register built-in Network controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            Network::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<
                crate::network::AwsNetworkController,
            >::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            Network::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<
                crate::network::GcpNetworkController,
            >::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            Network::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<
                crate::network::AzureNetworkController,
            >::new()),
        );

        // Register built-in Network CloudFormation importers
        #[cfg(feature = "aws")]
        {
            registry.register_cloudformation_importer_factory(
                Network::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::network::AwsNetworkCloudFormationImporter,
                >::new()),
            );
        }

        // Register built-in RemoteStackManagement CloudFormation importers
        #[cfg(feature = "aws")]
        {
            registry.register_cloudformation_importer_factory(
                RemoteStackManagement::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::remote_stack_management::AwsRemoteStackManagementCloudFormationImporter,
                >::new()),
            );
        }

        // Register built-in Vault controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            Vault::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<crate::vault::AwsVaultController>::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            Vault::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<crate::vault::GcpVaultController>::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            Vault::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<crate::vault::AzureVaultController>::new()),
        );

        // Register Local Vault controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            Vault::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<crate::vault::LocalVaultController>::new()),
        );

        // Register Kubernetes Vault controller
        #[cfg(feature = "kubernetes")]
        registry.register_controller_factory(
            Vault::RESOURCE_TYPE,
            Platform::Kubernetes,
            Box::new(DefaultControllerFactory::<
                crate::vault::KubernetesVaultController,
            >::new()),
        );

        // Register Test Vault controller
        #[cfg(feature = "test")]
        registry.register_controller_factory(
            Vault::RESOURCE_TYPE,
            Platform::Test,
            Box::new(DefaultControllerFactory::<crate::vault::TestVaultController>::new()),
        );

        // Register built-in Vault CloudFormation importers
        #[cfg(feature = "aws")]
        {
            registry.register_cloudformation_importer_factory(
                Vault::RESOURCE_TYPE,
                Platform::Aws,
                Box::new(DefaultCloudFormationImporterFactory::<
                    crate::vault::AwsVaultCloudFormationImporter,
                >::new()),
            );
        }

        // Register built-in KV controllers
        #[cfg(feature = "aws")]
        registry.register_controller_factory(
            Kv::RESOURCE_TYPE,
            Platform::Aws,
            Box::new(DefaultControllerFactory::<crate::kv::AwsKvController>::new()),
        );

        #[cfg(feature = "gcp")]
        registry.register_controller_factory(
            Kv::RESOURCE_TYPE,
            Platform::Gcp,
            Box::new(DefaultControllerFactory::<crate::kv::GcpKvController>::new()),
        );

        #[cfg(feature = "azure")]
        registry.register_controller_factory(
            Kv::RESOURCE_TYPE,
            Platform::Azure,
            Box::new(DefaultControllerFactory::<crate::kv::AzureKvController>::new()),
        );

        // Register Local KV controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            Kv::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<crate::kv::LocalKvController>::new()),
        );

        // Register Local ContainerCluster controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            ContainerCluster::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<
                crate::container_cluster::LocalContainerClusterController,
            >::new()),
        );

        // Register Local Container controller
        #[cfg(feature = "local")]
        registry.register_controller_factory(
            Container::RESOURCE_TYPE,
            Platform::Local,
            Box::new(DefaultControllerFactory::<
                crate::container::LocalContainerController,
            >::new()),
        );

        // Register Kubernetes Container controller
        #[cfg(feature = "kubernetes")]
        registry.register_controller_factory(
            Container::RESOURCE_TYPE,
            Platform::Kubernetes,
            Box::new(DefaultControllerFactory::<
                crate::container::KubernetesContainerController,
            >::new()),
        );

        // Apply extension registrations from platform crates (if any).
        if let Some(ext) = REGISTRY_EXTENSION.get() {
            ext(&mut registry);
        }

        registry
    }
}

impl Default for ResourceRegistry {
    fn default() -> Self {
        Self::with_built_ins()
    }
}

/// Builder for registering resource types and their controllers
pub struct ResourceRegistration<'a, R> {
    registry: &'a mut ResourceRegistry,
    resource_type: ResourceType,
    _phantom: PhantomData<R>,
}

impl<'a, R> ResourceRegistration<'a, R>
where
    R: ResourceDefinition + 'static,
{
    fn new(registry: &'a mut ResourceRegistry, resource_type: ResourceType) -> Self {
        Self {
            registry,
            resource_type,
            _phantom: PhantomData,
        }
    }

    /// Registers a controller for the given platform
    pub fn with_controller<C>(self, platform: Platform) -> Self
    where
        C: ResourceController + Default + 'static,
    {
        self.registry.register_controller_factory(
            self.resource_type.clone(),
            platform,
            Box::new(DefaultControllerFactory::<C>::new()),
        );
        self
    }

    /// Registers a custom controller factory for the given platform
    pub fn with_controller_factory(
        self,
        platform: Platform,
        factory: Box<dyn ControllerFactory>,
    ) -> Self {
        self.registry
            .register_controller_factory(self.resource_type.clone(), platform, factory);
        self
    }

    /// Registers a CloudFormation importer for the given platform
    pub fn with_cloudformation_importer<I>(self, platform: Platform) -> Self
    where
        I: crate::cloudformation::traits::CloudFormationResourceImporter + Default + 'static,
    {
        self.registry.register_cloudformation_importer_factory(
            self.resource_type.clone(),
            platform,
            Box::new(DefaultCloudFormationImporterFactory::<I>::new()),
        );
        self
    }

    /// Registers a custom CloudFormation importer factory for the given platform
    pub fn with_cloudformation_importer_factory(
        self,
        platform: Platform,
        factory: Box<dyn CloudFormationImporterFactory>,
    ) -> Self {
        self.registry.register_cloudformation_importer_factory(
            self.resource_type.clone(),
            platform,
            factory,
        );
        self
    }
}
