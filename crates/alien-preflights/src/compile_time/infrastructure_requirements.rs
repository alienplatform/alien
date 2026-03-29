//! Infrastructure requirements preflight checks.
//!
//! Two separate checks validate that stacks targeting cloud platforms (AWS, GCP, Azure)
//! have the required infrastructure when using containers or public ingress:
//!
//! - **HorizonRequiredCheck**: Container resources on cloud platforms require a Horizon
//!   cluster for orchestration. This is available through the alien.dev platform
//!   (alien-hosted or private managers). Local and Kubernetes platforms support
//!   containers natively.
//!
//! - **DnsTlsRequiredCheck**: Resources with public ingress on cloud platforms require
//!   DNS and TLS configuration for external URLs. This is provided automatically by
//!   the alien.dev platform. Standalone managers do not currently support external
//!   URLs on cloud platforms.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, ExposeProtocol, Function, Ingress, Platform, Stack};

/// Returns true for cloud platforms that require managed infrastructure.
fn is_cloud_platform(platform: Platform) -> bool {
    matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
}

/// Returns true when no alien.dev platform API key is configured.
///
/// The presence of `ALIEN_API_KEY` indicates that the deployment is running
/// through the alien.dev platform, which provisions Horizon clusters and
/// DNS/TLS infrastructure automatically.
fn missing_platform_api_key() -> bool {
    std::env::var("ALIEN_API_KEY").is_err()
}

/// Validates that container resources on cloud platforms have Horizon cluster
/// infrastructure available.
///
/// Containers on cloud platforms (AWS, GCP, Azure) run on Horizon clusters --
/// VMs with the Horizon orchestrator that schedule and manage container replicas.
/// Horizon clusters are provisioned through the alien.dev platform (alien-hosted
/// or private managers).
///
/// Local and Kubernetes platforms support containers natively without Horizon:
/// Local uses Docker directly, Kubernetes uses native pod scheduling.
///
/// This check runs only on cloud platforms when no `ALIEN_API_KEY` is set,
/// since the API key indicates the alien.dev platform is managing infrastructure.
pub struct HorizonRequiredCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for HorizonRequiredCheck {
    fn description(&self) -> &'static str {
        "Containers on cloud platforms require a Horizon cluster"
    }

    fn should_run(&self, _stack: &Stack, platform: Platform) -> bool {
        is_cloud_platform(platform) && missing_platform_api_key()
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut container_resources = Vec::new();

        for (resource_id, entry) in stack.resources() {
            if entry.config.downcast_ref::<Container>().is_some() {
                container_resources.push(resource_id.clone());
            }
        }

        if container_resources.is_empty() {
            Ok(CheckResult::success())
        } else {
            let resource_list = container_resources.join(", ");
            Ok(CheckResult::failed(vec![format!(
                "Containers on cloud platforms require a Horizon cluster for orchestration. \
                 Found container resources: {resource_list}. \
                 Horizon is available through the alien.dev platform \
                 (alien-hosted or private managers). \
                 Local and Kubernetes platforms support containers natively without Horizon."
            )]))
        }
    }
}

/// Validates that resources with public ingress on cloud platforms have DNS and
/// TLS configuration available.
///
/// Public HTTPS endpoints on cloud platforms need DNS records and TLS certificates
/// to serve external traffic. The alien.dev platform provisions this automatically
/// (domain assignment, certificate issuance, DNS configuration).
///
/// This applies to:
/// - Functions with `Ingress::Public`
/// - Containers with exposed HTTP ports (`expose: Http`)
///
/// Standalone managers do not currently support external URLs on cloud platforms.
/// Local platform assigns localhost URLs, Kubernetes uses Ingress/Service resources.
///
/// This check runs only on cloud platforms when no `ALIEN_API_KEY` is set.
pub struct DnsTlsRequiredCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for DnsTlsRequiredCheck {
    fn description(&self) -> &'static str {
        "External URLs on cloud platforms require DNS and TLS configuration"
    }

    fn should_run(&self, _stack: &Stack, platform: Platform) -> bool {
        is_cloud_platform(platform) && missing_platform_api_key()
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut public_resources = Vec::new();

        for (resource_id, entry) in stack.resources() {
            // Check for public ingress functions
            if let Some(function) = entry.config.downcast_ref::<Function>() {
                if function.ingress == Ingress::Public {
                    public_resources
                        .push(format!("function '{}' (public HTTPS ingress)", resource_id,));
                }
            }

            // Check for containers with exposed HTTP ports
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                let has_exposed_http = container
                    .ports
                    .iter()
                    .any(|p| p.expose == Some(ExposeProtocol::Http));
                if has_exposed_http {
                    public_resources
                        .push(format!("container '{}' (exposed HTTP port)", resource_id,));
                }
            }
        }

        if public_resources.is_empty() {
            Ok(CheckResult::success())
        } else {
            let resource_list = public_resources.join(", ");
            Ok(CheckResult::failed(vec![format!(
                "External URLs on cloud platforms require DNS and TLS configuration. \
                 Found resources with public ingress: {resource_list}. \
                 This is provided automatically by the alien.dev platform. \
                 For standalone managers, external URLs are not currently supported \
                 on cloud platforms."
            )]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        permissions::PermissionsConfig, Container, ContainerCode, Function, FunctionCode, Resource,
        ResourceEntry, ResourceLifecycle, ResourceSpec,
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

    fn create_container_with_exposed_http(id: &str) -> ResourceEntry {
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
                    .expose_port(8080, ExposeProtocol::Http)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn create_container_internal_only(id: &str) -> ResourceEntry {
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
                    .port(8080)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    // --- HorizonRequiredCheck tests ---

    #[tokio::test]
    async fn test_horizon_check_container_fails() {
        let mut resources = IndexMap::new();
        resources.insert(
            "web-server".to_string(),
            create_container_entry("web-server"),
        );

        let stack = create_stack(resources);
        let check = HorizonRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("web-server"));
        assert!(result.errors[0].contains("Horizon cluster"));
    }

    #[tokio::test]
    async fn test_horizon_check_no_containers_passes() {
        let mut resources = IndexMap::new();
        resources.insert(
            "api-handler".to_string(),
            create_public_function_entry("api-handler"),
        );

        let stack = create_stack(resources);
        let check = HorizonRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_horizon_check_empty_stack_passes() {
        let stack = create_stack(IndexMap::new());
        let check = HorizonRequiredCheck;
        let result = check.check(&stack, Platform::Azure).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_horizon_check_not_run_on_local_or_kubernetes() {
        let stack = create_stack(IndexMap::new());
        let check = HorizonRequiredCheck;

        assert!(!check.should_run(&stack, Platform::Kubernetes));
        assert!(!check.should_run(&stack, Platform::Local));
    }

    #[tokio::test]
    async fn test_horizon_check_runs_on_cloud_platforms() {
        let stack = create_stack(IndexMap::new());
        let check = HorizonRequiredCheck;

        // Only runs when ALIEN_API_KEY is not set (test env shouldn't have it)
        if std::env::var("ALIEN_API_KEY").is_err() {
            assert!(check.should_run(&stack, Platform::Aws));
            assert!(check.should_run(&stack, Platform::Gcp));
            assert!(check.should_run(&stack, Platform::Azure));
        }
    }

    #[tokio::test]
    async fn test_horizon_check_multiple_containers() {
        let mut resources = IndexMap::new();
        resources.insert("api".to_string(), create_container_entry("api"));
        resources.insert("worker".to_string(), create_container_entry("worker"));

        let stack = create_stack(resources);
        let check = HorizonRequiredCheck;
        let result = check.check(&stack, Platform::Gcp).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("api"));
        assert!(result.errors[0].contains("worker"));
    }

    // --- DnsTlsRequiredCheck tests ---

    #[tokio::test]
    async fn test_dns_tls_check_public_function_fails() {
        let mut resources = IndexMap::new();
        resources.insert(
            "api-handler".to_string(),
            create_public_function_entry("api-handler"),
        );

        let stack = create_stack(resources);
        let check = DnsTlsRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("api-handler"));
        assert!(result.errors[0].contains("public HTTPS ingress"));
        assert!(result.errors[0].contains("DNS and TLS"));
    }

    #[tokio::test]
    async fn test_dns_tls_check_container_exposed_http_fails() {
        let mut resources = IndexMap::new();
        resources.insert(
            "web-app".to_string(),
            create_container_with_exposed_http("web-app"),
        );

        let stack = create_stack(resources);
        let check = DnsTlsRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result.errors[0].contains("web-app"));
        assert!(result.errors[0].contains("exposed HTTP port"));
        assert!(result.errors[0].contains("DNS and TLS"));
    }

    #[tokio::test]
    async fn test_dns_tls_check_private_function_passes() {
        let mut resources = IndexMap::new();
        resources.insert(
            "worker".to_string(),
            create_private_function_entry("worker"),
        );

        let stack = create_stack(resources);
        let check = DnsTlsRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_dns_tls_check_container_internal_only_passes() {
        let mut resources = IndexMap::new();
        resources.insert(
            "backend".to_string(),
            create_container_internal_only("backend"),
        );

        let stack = create_stack(resources);
        let check = DnsTlsRequiredCheck;
        let result = check.check(&stack, Platform::Gcp).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_dns_tls_check_empty_stack_passes() {
        let stack = create_stack(IndexMap::new());
        let check = DnsTlsRequiredCheck;
        let result = check.check(&stack, Platform::Azure).await.unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_dns_tls_check_not_run_on_local_or_kubernetes() {
        let stack = create_stack(IndexMap::new());
        let check = DnsTlsRequiredCheck;

        assert!(!check.should_run(&stack, Platform::Kubernetes));
        assert!(!check.should_run(&stack, Platform::Local));
    }

    #[tokio::test]
    async fn test_dns_tls_check_runs_on_cloud_platforms() {
        let stack = create_stack(IndexMap::new());
        let check = DnsTlsRequiredCheck;

        // Only runs when ALIEN_API_KEY is not set (test env shouldn't have it)
        if std::env::var("ALIEN_API_KEY").is_err() {
            assert!(check.should_run(&stack, Platform::Aws));
            assert!(check.should_run(&stack, Platform::Gcp));
            assert!(check.should_run(&stack, Platform::Azure));
        }
    }

    #[tokio::test]
    async fn test_dns_tls_check_mixed_public_and_private() {
        let mut resources = IndexMap::new();
        resources.insert(
            "api-handler".to_string(),
            create_public_function_entry("api-handler"),
        );
        resources.insert(
            "worker".to_string(),
            create_private_function_entry("worker"),
        );

        let stack = create_stack(resources);
        let check = DnsTlsRequiredCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        // Should mention the public function but not the private one
        assert!(result.errors[0].contains("api-handler"));
        assert!(!result.errors[0].contains("worker"));
    }
}
