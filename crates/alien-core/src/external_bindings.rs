//! External bindings for pre-existing infrastructure services.
//!
//! External bindings allow using existing infrastructure (MinIO, Kafka, Redis, etc.)
//! instead of having Alien provision cloud resources. This is required for Kubernetes
//! platform deployments and optional for cloud platforms (to override specific resources).

use std::collections::HashMap;

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::bindings::{
    ArtifactRegistryBinding, BindingValue, ContainerAppsEnvironmentBinding, KvBinding,
    QueueBinding, StorageBinding, VaultBinding,
};
use crate::error::ErrorData;
use crate::resource::ResourceOutputs;
use crate::resources::AzureContainerAppsEnvironmentOutputs;
use crate::Resource;

/// Represents a binding to pre-existing infrastructure.
///
/// The binding type must match the resource type it's applied to.
/// Validated at runtime by the executor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExternalBinding {
    /// External storage binding (S3-compatible, GCS, Blob Storage)
    Storage(StorageBinding),
    /// External queue binding (Kafka, SQS, etc.)
    Queue(QueueBinding),
    /// External KV binding (Redis, etc.)
    Kv(KvBinding),
    /// External artifact registry binding (OCI registry)
    ArtifactRegistry(ArtifactRegistryBinding),
    /// External vault binding (HashiCorp Vault, etc.)
    Vault(VaultBinding),
    /// External Azure Container Apps Environment binding (pre-existing environment)
    ContainerAppsEnvironment(ContainerAppsEnvironmentBinding),
}

/// Map from resource ID to external binding.
///
/// Validated at runtime: binding type must match resource type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(transparent)]
pub struct ExternalBindings(pub HashMap<String, ExternalBinding>);

impl ExternalBindings {
    /// Creates an empty ExternalBindings map.
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Returns true if there are no external bindings.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Checks if a binding exists for the given resource ID.
    pub fn has(&self, resource_id: &str) -> bool {
        self.0.contains_key(resource_id)
    }

    /// Gets an external binding by resource ID.
    pub fn get(&self, resource_id: &str) -> Option<&ExternalBinding> {
        self.0.get(resource_id)
    }

    /// Gets a storage binding for the given resource ID.
    /// Returns an error if the binding exists but is not a Storage type.
    pub fn get_storage(&self, id: &str) -> crate::error::Result<Option<&StorageBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Storage(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "storage".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets a queue binding for the given resource ID.
    /// Returns an error if the binding exists but is not a Queue type.
    pub fn get_queue(&self, id: &str) -> crate::error::Result<Option<&QueueBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Queue(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "queue".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets a KV binding for the given resource ID.
    /// Returns an error if the binding exists but is not a Kv type.
    pub fn get_kv(&self, id: &str) -> crate::error::Result<Option<&KvBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Kv(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "kv".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets an artifact registry binding for the given resource ID.
    /// Returns an error if the binding exists but is not an ArtifactRegistry type.
    pub fn get_artifact_registry(
        &self,
        id: &str,
    ) -> crate::error::Result<Option<&ArtifactRegistryBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::ArtifactRegistry(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "artifact_registry".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets a vault binding for the given resource ID.
    /// Returns an error if the binding exists but is not a Vault type.
    pub fn get_vault(&self, id: &str) -> crate::error::Result<Option<&VaultBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Vault(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "vault".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets a container apps environment binding for the given resource ID.
    /// Returns an error if the binding exists but is not a ContainerAppsEnvironment type.
    pub fn get_container_apps_environment(
        &self,
        id: &str,
    ) -> crate::error::Result<Option<&ContainerAppsEnvironmentBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::ContainerAppsEnvironment(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "azure_container_apps_environment".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Inserts an external binding for a resource.
    pub fn insert(&mut self, resource_id: impl Into<String>, binding: ExternalBinding) {
        self.0.insert(resource_id.into(), binding);
    }
}

impl ExternalBinding {
    /// Returns the type name of this binding variant.
    pub fn binding_type(&self) -> &'static str {
        match self {
            ExternalBinding::Storage(_) => "storage",
            ExternalBinding::Queue(_) => "queue",
            ExternalBinding::Kv(_) => "kv",
            ExternalBinding::ArtifactRegistry(_) => "artifact_registry",
            ExternalBinding::Vault(_) => "vault",
            ExternalBinding::ContainerAppsEnvironment(_) => "azure_container_apps_environment",
        }
    }

    /// Converts this external binding into resource outputs that dependent resources
    /// can read via `get_resource_outputs()`.
    ///
    /// Infrastructure bindings (Container Apps Environment) produce typed outputs so that
    /// dependent resources like functions and builds can read the environment's name,
    /// resource ID, and resource group. Application-level bindings (Storage, Queue, KV, etc.)
    /// return `None` — they are consumed via `remote_binding_params` and environment variables
    /// rather than `get_resource_outputs()`.
    pub fn to_resource_outputs(&self) -> Option<ResourceOutputs> {
        match self {
            ExternalBinding::ContainerAppsEnvironment(binding) => {
                // Extract concrete values from BindingValue wrappers.
                // External bindings for pre-provisioned resources always use concrete values.
                let environment_name = match &binding.environment_name {
                    BindingValue::Value(v) => v.clone(),
                    _ => return None,
                };
                let resource_id = match &binding.resource_id {
                    BindingValue::Value(v) => v.clone(),
                    _ => return None,
                };
                let resource_group_name = match &binding.resource_group_name {
                    BindingValue::Value(v) => v.clone(),
                    _ => return None,
                };
                let default_domain = match &binding.default_domain {
                    BindingValue::Value(v) => v.clone(),
                    _ => return None,
                };
                let static_ip = binding.static_ip.as_ref().and_then(|v| match v {
                    BindingValue::Value(v) => Some(v.clone()),
                    _ => None,
                });

                Some(ResourceOutputs::new(AzureContainerAppsEnvironmentOutputs {
                    environment_name,
                    resource_id,
                    resource_group_name,
                    default_domain,
                    static_ip,
                }))
            }
            // Application-level bindings are consumed via remote_binding_params, not outputs
            _ => None,
        }
    }
}

/// Validates that an external binding type matches the resource type.
pub fn validate_binding_type(
    resource: &Resource,
    binding: &ExternalBinding,
) -> crate::error::Result<()> {
    let resource_type = resource.resource_type();
    let resource_type_str = resource_type.as_ref();

    let valid = match (resource_type_str, binding) {
        ("storage", ExternalBinding::Storage(_)) => true,
        ("queue", ExternalBinding::Queue(_)) => true,
        ("kv", ExternalBinding::Kv(_)) => true,
        ("artifact_registry", ExternalBinding::ArtifactRegistry(_)) => true,
        ("vault", ExternalBinding::Vault(_)) => true,
        ("azure_container_apps_environment", ExternalBinding::ContainerAppsEnvironment(_)) => true,
        _ => false,
    };

    if !valid {
        return Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
            resource_id: resource.id().to_string(),
            expected: resource_type_str.to_string(),
            actual: binding.binding_type().to_string(),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{KvBinding, StorageBinding};

    #[test]
    fn test_external_bindings_storage() {
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "data-storage",
            ExternalBinding::Storage(StorageBinding::s3("my-bucket")),
        );

        assert!(bindings.has("data-storage"));
        assert!(bindings.get_storage("data-storage").unwrap().is_some());
        assert!(bindings.get_queue("data-storage").is_err()); // Wrong type
    }

    #[test]
    fn test_external_bindings_kv() {
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "cache",
            ExternalBinding::Kv(KvBinding::redis("redis://localhost:6379")),
        );

        assert!(bindings.has("cache"));
        assert!(bindings.get_kv("cache").unwrap().is_some());
        assert!(bindings.get_storage("cache").is_err()); // Wrong type
    }

    #[test]
    fn test_external_bindings_serialization() {
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "data",
            ExternalBinding::Storage(StorageBinding::s3("test-bucket")),
        );

        let json = serde_json::to_string(&bindings).unwrap();
        let deserialized: ExternalBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(bindings, deserialized);
    }
}
