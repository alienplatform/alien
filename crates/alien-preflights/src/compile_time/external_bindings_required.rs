//! External bindings validation check for Kubernetes platform.
//!
//! On Kubernetes platform, most infrastructure resources require external bindings
//! since Alien doesn't provision cloud resources. However, some resources have
//! native Kubernetes controllers (e.g., Vault uses K8s Secrets).

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{validate_binding_type, ExternalBindings, Platform, Stack};

/// Infrastructure resource types that require external bindings on Kubernetes.
///
/// Note: Vault is NOT in this list - it has a native controller that creates
/// Kubernetes Secret resources. External bindings are optional for vault
/// (only needed if using AWS Secrets Manager, GCP Secret Manager, etc.).
const INFRASTRUCTURE_RESOURCE_TYPES: &[&str] = &["storage", "queue", "kv", "artifact_registry"];

/// Ensures infrastructure resources have external bindings on Kubernetes platform.
///
/// Kubernetes platform does NOT provision most infrastructure. Resources like Storage,
/// Queue, and KV must have external bindings pointing to pre-existing services
/// (S3, MinIO, SQS, Redis, etc.).
///
/// Vault is an exception - it has a native controller that uses K8s Secrets by default.
/// External bindings for vault are optional (only if you want AWS/GCP/Azure secrets).
///
/// This check validates:
/// 1. Required infrastructure resources have a corresponding external binding
/// 2. The binding type matches the resource type
pub struct ExternalBindingsRequiredCheck {
    external_bindings: ExternalBindings,
}

impl ExternalBindingsRequiredCheck {
    /// Creates a new check with the given external bindings.
    pub fn new(external_bindings: ExternalBindings) -> Self {
        Self { external_bindings }
    }
}

#[async_trait::async_trait]
impl CompileTimeCheck for ExternalBindingsRequiredCheck {
    fn description(&self) -> &'static str {
        "Kubernetes platform requires external bindings for infrastructure resources"
    }

    fn should_run(&self, _stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Kubernetes
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut missing = Vec::new();
        let mut mismatched = Vec::new();

        for (resource_id, entry) in stack.resources() {
            let resource_type = entry.config.resource_type();
            let resource_type_str = resource_type.as_ref();

            // Check if this is an infrastructure resource
            if !INFRASTRUCTURE_RESOURCE_TYPES.contains(&resource_type_str) {
                continue;
            }

            // Check if external binding exists
            match self.external_bindings.get(resource_id) {
                None => {
                    missing.push(resource_id.clone());
                }
                Some(binding) => {
                    // Validate binding type matches resource type
                    if validate_binding_type(&entry.config, binding).is_err() {
                        mismatched.push(resource_id.clone());
                    }
                }
            }
        }

        let mut errors = Vec::new();

        if !missing.is_empty() {
            errors.push(format!(
                "Kubernetes platform requires external bindings for infrastructure resources. \
                 Missing bindings for: {}. \
                 Provide external bindings via Helm values or operator configuration.",
                missing.join(", ")
            ));
        }

        if !mismatched.is_empty() {
            errors.push(format!(
                "External binding type mismatch for: {}. \
                 The binding type must match the resource type.",
                mismatched.join(", ")
            ));
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        bindings::{KvBinding, StorageBinding},
        permissions::PermissionsConfig,
        ExternalBinding, Kv, Resource, ResourceEntry, ResourceLifecycle, Storage,
    };
    use indexmap::IndexMap;

    fn create_stack(resources: IndexMap<String, ResourceEntry>) -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    fn create_storage_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(Storage::new(id.to_string()).build()),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn create_kv_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(Kv::new(id.to_string()).build()),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    #[tokio::test]
    async fn test_missing_external_bindings() {
        let mut resources = IndexMap::new();
        resources.insert(
            "data-storage".to_string(),
            create_storage_entry("data-storage"),
        );
        resources.insert("cache".to_string(), create_kv_entry("cache"));

        let stack = create_stack(resources);
        let external_bindings = ExternalBindings::new(); // Empty
        let check = ExternalBindingsRequiredCheck::new(external_bindings);

        let result = check.check(&stack, Platform::Kubernetes).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("data-storage"));
        assert!(result.errors[0].contains("cache"));
    }

    #[tokio::test]
    async fn test_all_bindings_provided() {
        let mut resources = IndexMap::new();
        resources.insert(
            "data-storage".to_string(),
            create_storage_entry("data-storage"),
        );
        resources.insert("cache".to_string(), create_kv_entry("cache"));

        let stack = create_stack(resources);

        let mut external_bindings = ExternalBindings::new();
        external_bindings.insert(
            "data-storage",
            ExternalBinding::Storage(StorageBinding::s3("my-bucket")),
        );
        external_bindings.insert(
            "cache",
            ExternalBinding::Kv(KvBinding::redis("redis://localhost:6379")),
        );

        let check = ExternalBindingsRequiredCheck::new(external_bindings);
        let result = check.check(&stack, Platform::Kubernetes).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_binding_type_mismatch() {
        let mut resources = IndexMap::new();
        resources.insert(
            "data-storage".to_string(),
            create_storage_entry("data-storage"),
        );

        let stack = create_stack(resources);

        let mut external_bindings = ExternalBindings::new();
        // Wrong type: providing KV binding for a Storage resource
        external_bindings.insert(
            "data-storage",
            ExternalBinding::Kv(KvBinding::redis("redis://localhost:6379")),
        );

        let check = ExternalBindingsRequiredCheck::new(external_bindings);
        let result = check.check(&stack, Platform::Kubernetes).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("type mismatch"));
    }

    #[tokio::test]
    async fn test_check_not_run_on_aws() {
        let mut resources = IndexMap::new();
        resources.insert(
            "data-storage".to_string(),
            create_storage_entry("data-storage"),
        );

        let stack = create_stack(resources);
        let external_bindings = ExternalBindings::new();
        let check = ExternalBindingsRequiredCheck::new(external_bindings);

        // Should not run on AWS
        assert!(!check.should_run(&stack, Platform::Aws));
        // Should run on Kubernetes
        assert!(check.should_run(&stack, Platform::Kubernetes));
    }
}
