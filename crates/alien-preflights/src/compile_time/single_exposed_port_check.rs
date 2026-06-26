use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};
use async_trait::async_trait;

/// Validates that container public endpoints use at most one backend port.
///
/// Containers can have multiple ports, but currently only one can be exposed publicly.
/// This limitation exists because controllers create a single load balancer per container.
/// This check ensures the configuration is valid before deployment.
pub struct SingleExposedPortCheck;

#[async_trait]
impl CompileTimeCheck for SingleExposedPortCheck {
    fn description(&self) -> &'static str {
        "Validate container public endpoints use at most one backend port"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Run for all platforms - this is a universal constraint for now
        stack
            .resources()
            .any(|(_, entry)| entry.config.downcast_ref::<Container>().is_some())
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> crate::error::Result<CheckResult> {
        let mut failures = Vec::new();

        for (_id, resource_entry) in stack.resources() {
            if let Some(container) = resource_entry.config.downcast_ref::<Container>() {
                let public_backend_ports = container
                    .public_endpoints
                    .iter()
                    .map(|endpoint| endpoint.port)
                    .collect::<std::collections::BTreeSet<_>>();

                if public_backend_ports.len() > 1 {
                    failures.push(format!(
                        "Container '{}' has public endpoints on multiple backend ports ({:?}), but one backend port is currently supported. Split the endpoints across separate resources or route them through one port.",
                        container.id,
                        public_backend_ports
                    ));
                }
            }
        }

        if failures.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(failures))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ContainerCode, ExposeProtocol, PublicEndpoint, ResourceSpec, Stack};

    #[tokio::test]
    async fn test_single_exposed_port_passes() {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .public_endpoint(PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .public_endpoint(PublicEndpoint {
                name: "shares".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: true,
            })
            .port(9090)
            .replicas(1)
            .permissions("test".to_string())
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .build();

        let check = SingleExposedPortCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success, "Should pass with one backend port");
    }

    #[tokio::test]
    async fn test_multiple_backend_ports_fails() {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .public_endpoint(PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .port(9090)
            .public_endpoint(PublicEndpoint {
                name: "admin".to_string(),
                port: 9090,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .replicas(1)
            .permissions("test".to_string())
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .build();

        let check = SingleExposedPortCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success, "Should fail with multiple backend ports");
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("multiple backend ports")));
    }

    #[tokio::test]
    async fn test_no_exposed_ports_passes() {
        let container = Container::new("internal".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .replicas(1)
            .permissions("test".to_string())
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .build();

        let check = SingleExposedPortCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success, "Should pass with no exposed ports");
    }
}
