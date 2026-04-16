use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};
use async_trait::async_trait;

/// Validates that containers have at most one exposed port.
///
/// Containers can have multiple ports, but currently only one can be exposed publicly.
/// This limitation exists because controllers create a single load balancer per container.
/// This check ensures the configuration is valid before deployment.
pub struct SingleExposedPortCheck;

#[async_trait]
impl CompileTimeCheck for SingleExposedPortCheck {
    fn description(&self) -> &'static str {
        "Validate containers have at most one exposed port"
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
                let exposed_ports: Vec<_> = container
                    .ports
                    .iter()
                    .filter(|p| p.expose.is_some())
                    .collect();

                if exposed_ports.len() > 1 {
                    let ports_list: Vec<u16> = exposed_ports.iter().map(|p| p.port).collect();
                    failures.push(format!(
                        "Container '{}' has {} exposed ports ({:?}), but only one exposed port is currently supported. Remove expose configuration from all but one port.",
                        container.id,
                        exposed_ports.len(),
                        ports_list
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
    use alien_core::{ContainerCode, ContainerPort, ExposeProtocol, ResourceSpec, Stack};

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
            .expose_port(8080, ExposeProtocol::Http)
            .port(9090)
            .replicas(1)
            .permissions("test".to_string())
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .build();

        let check = SingleExposedPortCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(result.success, "Should pass with one exposed port");
    }

    #[tokio::test]
    async fn test_multiple_exposed_ports_fails() {
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
            .expose_port(8080, ExposeProtocol::Http)
            .port(9090)
            .expose_port(9090, ExposeProtocol::Tcp)
            .replicas(1)
            .permissions("test".to_string())
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .build();

        let check = SingleExposedPortCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success, "Should fail with multiple exposed ports");
        assert!(result.errors.iter().any(|e| e.contains("2 exposed ports")));
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
