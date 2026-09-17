//! GCP infrastructure controllers.

macro_rules! provider_module {
    ($name:ident) => {
        pub mod $name {
            pub mod gcp;
            pub mod gcp_import;
            pub use gcp::*;
            pub use gcp_import::*;
        }
    };
}

provider_module!(ai);
provider_module!(artifact_registry);
provider_module!(kv);
provider_module!(network);
provider_module!(queue);
provider_module!(remote_bindings);
provider_module!(remote_stack_management);
provider_module!(service_account);
provider_module!(service_activation);
provider_module!(vault);

pub mod build {
    #[cfg(test)]
    pub mod fixtures;
    pub mod gcp;
    pub mod gcp_import;
    pub use gcp::*;
    pub use gcp_import::*;
}

pub mod key {
    pub mod gcp;
    pub use gcp::*;
}

pub mod sandbox {
    pub mod gcp_agent_platform_engine;
    pub mod gcp_agent_platform_template;
    pub mod gcp_import;
    pub use gcp_agent_platform_engine::*;
    pub use gcp_agent_platform_template::*;
    pub use gcp_import::*;
}

pub mod storage {
    #[cfg(test)]
    pub mod fixtures;
    pub mod gcp;
    pub mod gcp_import;
    pub use gcp::*;
    pub use gcp_import::*;
}

pub mod worker {
    #[cfg(test)]
    pub mod fixtures;
    pub mod gcp;
    pub mod gcp_import;
    pub mod readiness_probe;
    pub use gcp::*;
    pub use gcp_import::*;
    pub use readiness_probe::*;
}
pub use worker::*;

pub mod core {
    pub use crate::{GcpServiceProvider as PlatformServiceProvider, ResourcePermissionsHelper};
    #[cfg(test)]
    pub use alien_infra::{
        controller_test, deserialize_controller, MockPlatformServiceProvider, ResourceRegistry,
        StackExecutor,
    };
    pub use alien_infra_core::*;
}

pub mod error {
    pub use alien_infra_core::{ErrorData, Result};
}

pub mod import {
    #[cfg(test)]
    pub use alien_infra::ImporterRegistry;
    pub use alien_infra_core::ResourceImporter;
}

pub mod import_helpers {
    pub use alien_infra_core::{make_imported_state, make_imported_state_with_status};
}

pub use error::Result;

mod resource_permissions_helper;
pub use resource_permissions_helper::ResourcePermissionsHelper;

#[cfg(test)]
pub use alien_infra::MockPlatformServiceProvider;

use std::sync::Arc;

use alien_gcp_clients::{
    agent_platform::AgentPlatformApi, artifactregistry::ArtifactRegistryApi,
    cloud_kms::CloudKmsApi, cloudbuild::CloudBuildApi, cloudrun::CloudRunApi,
    cloudscheduler::CloudSchedulerApi, compute::ComputeApi as GcpComputeApi,
    firestore::FirestoreApi, gcs::GcsApi, iam::IamApi as GcpIamApi, model_garden::ModelGardenApi,
    pubsub::PubSubApi, resource_manager::ResourceManagerApi, service_usage::ServiceUsageApi,
    GcpClientConfig,
};

/// Provider-specific client factory used by GCP controllers.
pub trait GcpServiceProvider: Send + Sync {
    fn get_gcp_iam_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpIamApi>>;
    fn get_gcp_cloudbuild_client(&self, config: &GcpClientConfig)
        -> Result<Arc<dyn CloudBuildApi>>;
    fn get_gcp_cloudrun_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn CloudRunApi>>;
    fn get_gcp_resource_manager_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ResourceManagerApi>>;
    fn get_gcp_service_usage_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ServiceUsageApi>>;
    fn get_gcp_model_garden_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ModelGardenApi>>;
    fn get_gcp_gcs_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcsApi>>;
    fn get_gcp_artifact_registry_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn ArtifactRegistryApi>>;
    fn get_gcp_firestore_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn FirestoreApi>>;
    fn get_gcp_pubsub_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn PubSubApi>>;
    fn get_gcp_compute_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn GcpComputeApi>>;
    fn get_gcp_cloud_scheduler_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn CloudSchedulerApi>>;
    fn get_gcp_cloud_kms_client(&self, config: &GcpClientConfig) -> Result<Arc<dyn CloudKmsApi>>;
    fn get_gcp_agent_platform_client(
        &self,
        config: &GcpClientConfig,
    ) -> Result<Arc<dyn AgentPlatformApi>>;
}

#[cfg(test)]
macro_rules! impl_test_gcp_service_provider {
    ($(($method:ident, $api:ty)),* $(,)?) => {
        impl GcpServiceProvider for alien_infra::MockPlatformServiceProvider {
            $(
                fn $method(&self, config: &GcpClientConfig) -> Result<Arc<$api>> {
                    alien_infra::PlatformServiceProvider::$method(self, config)
                }
            )*
        }
    };
}

#[cfg(test)]
impl_test_gcp_service_provider!(
    (get_gcp_iam_client, dyn GcpIamApi),
    (get_gcp_cloudbuild_client, dyn CloudBuildApi),
    (get_gcp_cloudrun_client, dyn CloudRunApi),
    (get_gcp_resource_manager_client, dyn ResourceManagerApi),
    (get_gcp_service_usage_client, dyn ServiceUsageApi),
    (get_gcp_model_garden_client, dyn ModelGardenApi),
    (get_gcp_gcs_client, dyn GcsApi),
    (get_gcp_artifact_registry_client, dyn ArtifactRegistryApi),
    (get_gcp_firestore_client, dyn FirestoreApi),
    (get_gcp_pubsub_client, dyn PubSubApi),
    (get_gcp_compute_client, dyn GcpComputeApi),
    (get_gcp_cloud_scheduler_client, dyn CloudSchedulerApi),
    (get_gcp_cloud_kms_client, dyn CloudKmsApi),
    (get_gcp_agent_platform_client, dyn AgentPlatformApi),
);

#[cfg(test)]
pub trait GcpControllerTestBuilderExt {
    fn gcp_service_provider(self, provider: Arc<alien_infra::MockPlatformServiceProvider>) -> Self;
}

#[cfg(test)]
impl GcpControllerTestBuilderExt for alien_infra::controller_test::SingleControllerExecutorBuilder {
    fn gcp_service_provider(self, provider: Arc<alien_infra::MockPlatformServiceProvider>) -> Self {
        self.service_provider(provider.clone())
            .service::<dyn GcpServiceProvider>(provider)
    }
}
