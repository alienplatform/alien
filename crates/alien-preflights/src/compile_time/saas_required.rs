//! SaaS-required preflight check.
//!
//! Validates that stacks targeting cloud platforms (AWS, GCP, Azure) with
//! public ingress functions or container resources have an alien.dev account
//! connected (ALIEN_API_KEY set). These features require the platform API
//! for provisioning.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Function, Ingress, Platform, Stack};

/// Ensures cloud deployments with public ingress or containers have an alien.dev account.
///
/// Public HTTPS endpoints require platform-managed infrastructure (API gateways,
/// load balancers, TLS certificates) that only the alien.dev platform provisions.
/// Container workloads require platform-managed container registries and orchestration.
///
/// This check blocks `alien build` when targeting cloud platforms without an API key,
/// if the stack uses features that require the platform.
pub struct SaasRequiredCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for SaasRequiredCheck {
    fn description(&self) -> &'static str {
        "Public ingress and cloud containers require an alien.dev account"
    }

    fn should_run(&self, _stack: &Stack, platform: Platform) -> bool {
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && std::env::var("ALIEN_API_KEY").is_err()
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut requires_saas = Vec::new();

        for (resource_id, entry) in stack.resources() {
            // Check for public ingress functions
            if let Some(function) = entry.config.downcast_ref::<Function>() {
                if function.ingress == Ingress::Public {
                    requires_saas.push(format!(
                        "Function '{}' has public HTTPS ingress",
                        resource_id,
                    ));
                }
            }

            // Check for container resources
            if entry.config.downcast_ref::<Container>().is_some() {
                requires_saas.push(format!(
                    "Resource '{}' is a container workload",
                    resource_id,
                ));
            }
        }

        if requires_saas.is_empty() {
            Ok(CheckResult::success())
        } else {
            let detail = requires_saas.join(", ");
            Ok(CheckResult::failed(vec![format!(
                "This stack requires an alien.dev account for cloud deployment: {}. \
                 Run `alien login` to connect your account, or use \
                 `alien serve --standalone` with Local/Kubernetes platforms only.",
                detail,
            )]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ContainerCode, FunctionCode, ResourceSpec,
        permissions::PermissionsConfig,
        Container, Function, Resource, ResourceEntry, ResourceLifecycle,
    };
    use indexmap::IndexMap;

    fn create_stack(resources: IndexMap<String, ResourceEntry>) -> Stack {
        Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig::default(),
        }
    }

    fn create_public_function_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Function::new(id.to_string())
                    .code(FunctionCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("default".to_string())
                    .ingress(Ingress::Public)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn create_private_function_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Function::new(id.to_string())
                    .code(FunctionCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("default".to_string())
                    .ingress(Ingress::Private)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn create_container_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Container::new(id.to_string())
                    .code(ContainerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .cpu(ResourceSpec {
                        min: "0.25".to_string(),
                        desired: "0.5".to_string(),
                    })
                    .memory(ResourceSpec {
                        min: "256Mi".to_string(),
                        desired: "512Mi".to_string(),
                    })
                    .replicas(1)
                    .permissions("default".to_string())
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    #[tokio::test]
    async fn test_public_function_fails() {
        let mut resources = IndexMap::new();
        resources.insert(
            "api-handler".to_string(),
            create_public_function_entry("api-handler"),
        );

        let stack = create_stack(resources);
        let check = SaasRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("api-handler"));
        assert!(result.errors[0].contains("public HTTPS ingress"));
    }

    #[tokio::test]
    async fn test_container_fails() {
        let mut resources = IndexMap::new();
        resources.insert(
            "web-server".to_string(),
            create_container_entry("web-server"),
        );

        let stack = create_stack(resources);
        let check = SaasRequiredCheck;
        let result = check.check(&stack, Platform::Gcp).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("web-server"));
        assert!(result.errors[0].contains("container workload"));
    }

    #[tokio::test]
    async fn test_private_function_passes() {
        let mut resources = IndexMap::new();
        resources.insert(
            "worker".to_string(),
            create_private_function_entry("worker"),
        );

        let stack = create_stack(resources);
        let check = SaasRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_empty_stack_passes() {
        let stack = create_stack(IndexMap::new());
        let check = SaasRequiredCheck;
        let result = check.check(&stack, Platform::Azure).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_should_not_run_on_kubernetes() {
        let stack = create_stack(IndexMap::new());
        let check = SaasRequiredCheck;

        assert!(!check.should_run(&stack, Platform::Kubernetes));
        assert!(!check.should_run(&stack, Platform::Local));
    }

    #[tokio::test]
    async fn test_should_run_on_cloud_platforms() {
        let stack = create_stack(IndexMap::new());
        let check = SaasRequiredCheck;

        // Only runs when ALIEN_API_KEY is not set (test env shouldn't have it)
        if std::env::var("ALIEN_API_KEY").is_err() {
            assert!(check.should_run(&stack, Platform::Aws));
            assert!(check.should_run(&stack, Platform::Gcp));
            assert!(check.should_run(&stack, Platform::Azure));
        }
    }
}
