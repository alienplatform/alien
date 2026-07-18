use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Daemon, ExposeProtocol, Platform, Stack, Worker};
use async_trait::async_trait;

/// Validates public endpoint and daemon runtime configuration.
///
/// Endpoint naming, backend port, and trusted runtime constraints must fail
/// before controllers try to materialize cloud resources.
pub struct SingleExposedPortCheck;

#[async_trait]
impl CompileTimeCheck for SingleExposedPortCheck {
    fn description(&self) -> &'static str {
        "Validate workload public endpoints and daemon runtime options"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        // Run for all platforms - this is a universal constraint for now
        stack.resources().any(|(_, entry)| {
            entry.config.downcast_ref::<Container>().is_some()
                || entry.config.downcast_ref::<Daemon>().is_some()
                || entry.config.downcast_ref::<Worker>().is_some()
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> crate::error::Result<CheckResult> {
        let mut failures = Vec::new();

        for (_id, resource_entry) in stack.resources() {
            if let Some(container) = resource_entry.config.downcast_ref::<Container>() {
                validate_container_public_endpoints(container, platform, &mut failures);
            }
            if let Some(daemon) = resource_entry.config.downcast_ref::<Daemon>() {
                validate_daemon_public_endpoints(daemon, &mut failures);
                validate_daemon_runtime(daemon, &mut failures);
            }
            if let Some(worker) = resource_entry.config.downcast_ref::<Worker>() {
                validate_worker_public_endpoints(worker, &mut failures);
            }
        }

        if failures.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(failures))
        }
    }
}

fn validate_container_public_endpoints(
    container: &Container,
    platform: Platform,
    failures: &mut Vec<String>,
) {
    let mut endpoint_names = std::collections::BTreeSet::new();
    let mut public_backend_ports = std::collections::BTreeSet::new();
    let mut protocols = std::collections::BTreeSet::new();

    for endpoint in &container.public_endpoints {
        if let Err(error) = endpoint.validate_for_resource(&container.id) {
            failures.push(format!("Container '{}': {}", container.id, error));
        }
        if !endpoint_names.insert(endpoint.name.as_str()) {
            failures.push(format!(
                "Container '{}': duplicate public endpoint name '{}'",
                container.id, endpoint.name
            ));
        }
        if !container
            .ports
            .iter()
            .any(|port| port.port == endpoint.port)
        {
            failures.push(format!(
                "Container '{}': public endpoint '{}' references undeclared port {}",
                container.id, endpoint.name, endpoint.port
            ));
        }
        public_backend_ports.insert(endpoint.port);
        protocols.insert(endpoint.protocol);
        if !container_endpoint_supported(platform, endpoint.protocol) {
            failures.push(format!(
                "Container '{}': {} public endpoints are not supported on {}",
                container.id,
                match endpoint.protocol {
                    ExposeProtocol::Http => "HTTP",
                    ExposeProtocol::Tcp => "TCP",
                },
                platform
            ));
        }
    }

    if protocols.len() > 1 {
        failures.push(format!(
            "Container '{}' cannot mix HTTP and TCP public endpoints",
            container.id
        ));
    }

    if public_backend_ports.len() > 1 {
        failures.push(format!(
            "Container '{}' has public endpoints on multiple backend ports ({:?}), but one backend port is currently supported. Split the endpoints across separate resources or route them through one port.",
            container.id, public_backend_ports
        ));
    }
}

fn container_endpoint_supported(platform: Platform, protocol: ExposeProtocol) -> bool {
    match (platform, protocol) {
        (
            Platform::Aws
            | Platform::Gcp
            | Platform::Azure
            | Platform::Kubernetes
            | Platform::Machines
            | Platform::Local
            | Platform::Test,
            ExposeProtocol::Http,
        ) => true,
        (Platform::Aws | Platform::Gcp | Platform::Azure, ExposeProtocol::Tcp) => true,
        (_, ExposeProtocol::Tcp) => false,
    }
}

fn validate_daemon_public_endpoints(daemon: &Daemon, failures: &mut Vec<String>) {
    let mut endpoint_names = std::collections::BTreeSet::new();
    let mut public_backend_ports = std::collections::BTreeSet::new();

    for endpoint in &daemon.public_endpoints {
        if let Err(error) = endpoint.validate_for_resource(&daemon.id) {
            failures.push(format!("Daemon '{}': {}", daemon.id, error));
        }
        if !endpoint_names.insert(endpoint.name.as_str()) {
            failures.push(format!(
                "Daemon '{}': duplicate public endpoint name '{}'",
                daemon.id, endpoint.name
            ));
        }
        if endpoint.protocol != ExposeProtocol::Http {
            failures.push(format!(
                "Daemon '{}': public endpoints currently support only HTTP",
                daemon.id
            ));
        }
        public_backend_ports.insert(endpoint.port);
    }

    if public_backend_ports.len() > 1 {
        failures.push(format!(
            "Daemon '{}' has public endpoints on multiple backend ports ({:?}), but one backend port is currently supported. Split the endpoints across separate resources or route them through one port.",
            daemon.id, public_backend_ports
        ));
    }
}

fn validate_worker_public_endpoints(worker: &Worker, failures: &mut Vec<String>) {
    let mut endpoint_names = std::collections::BTreeSet::new();

    for endpoint in &worker.public_endpoints {
        if let Err(error) = endpoint.validate_for_resource(&worker.id) {
            failures.push(format!("Worker '{}': {}", worker.id, error));
        }
        if !endpoint_names.insert(endpoint.name.as_str()) {
            failures.push(format!(
                "Worker '{}': duplicate public endpoint name '{}'",
                worker.id, endpoint.name
            ));
        }
    }
}

fn validate_daemon_runtime(daemon: &Daemon, failures: &mut Vec<String>) {
    let Some(runtime) = &daemon.runtime else {
        return;
    };

    if let Some(pid_namespace) = &runtime.pid_namespace {
        if pid_namespace != "host" && pid_namespace != "private" {
            failures.push(format!(
                "Daemon '{}': runtime.pidNamespace must be 'host' or 'private'",
                daemon.id
            ));
        }
    }

    if let Some(network_mode) = &runtime.network_mode {
        if network_mode != "host" && network_mode != "appnet" {
            failures.push(format!(
                "Daemon '{}': runtime.networkMode must be 'host' or 'appnet'",
                daemon.id
            ));
        }
    }

    if let Some(user) = &runtime.user {
        let valid = match user.split_once(':') {
            Some((uid, gid)) => {
                !uid.is_empty()
                    && !gid.is_empty()
                    && uid.chars().all(|c| c.is_ascii_digit())
                    && gid.chars().all(|c| c.is_ascii_digit())
            }
            None => !user.is_empty() && user.chars().all(|c| c.is_ascii_digit()),
        };
        if !valid {
            failures.push(format!(
                "Daemon '{}': runtime.user must be a numeric uid or uid:gid",
                daemon.id
            ));
        }
    }

    for mount in &runtime.mounts {
        if mount.source.is_empty() || mount.target.is_empty() {
            failures.push(format!(
                "Daemon '{}': runtime.mounts source and target must be non-empty",
                daemon.id
            ));
        } else if !mount.source.starts_with('/') || !mount.target.starts_with('/') {
            failures.push(format!(
                "Daemon '{}': runtime.mounts source and target must be absolute paths",
                daemon.id
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ContainerCode, DaemonCode, DaemonRuntime, DaemonRuntimeMount, PublicEndpoint, ResourceSpec,
        Stack,
    };

    fn container_with_endpoint_protocol(protocol: ExposeProtocol) -> Container {
        Container::new("gateway".to_string())
            .code(ContainerCode::Image {
                image: "example.test/gateway:latest".to_string(),
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
                protocol,
                host_label: None,
                wildcard_subdomains: false,
            })
            .replicas(1)
            .permissions("test".to_string())
            .build()
    }

    #[tokio::test]
    async fn container_http_is_supported_on_every_deployable_platform() {
        for &platform in Platform::DEPLOYABLE {
            let stack = Stack::new("test-stack".to_string())
                .add(
                    container_with_endpoint_protocol(ExposeProtocol::Http),
                    alien_core::ResourceLifecycle::Live,
                )
                .build();

            let result = SingleExposedPortCheck
                .check(&stack, platform)
                .await
                .expect("preflight should run");
            assert!(result.success, "HTTP should be supported on {platform}");
        }
    }

    #[tokio::test]
    async fn container_tcp_is_supported_only_on_cloud_container_platforms() {
        for &platform in Platform::DEPLOYABLE {
            let stack = Stack::new("test-stack".to_string())
                .add(
                    container_with_endpoint_protocol(ExposeProtocol::Tcp),
                    alien_core::ResourceLifecycle::Live,
                )
                .build();

            let result = SingleExposedPortCheck
                .check(&stack, platform)
                .await
                .expect("preflight should run");
            let expected = matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure);
            assert_eq!(
                result.success, expected,
                "unexpected TCP support on {platform}"
            );
        }
    }

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
                name: "wildcard".to_string(),
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

    #[tokio::test]
    async fn test_daemon_endpoint_and_runtime_validation_fails() {
        let daemon = Daemon::new("gateway".to_string())
            .code(DaemonCode::Image {
                image: "test:latest".to_string(),
            })
            .public_endpoint(PublicEndpoint {
                name: "api".to_string(),
                port: 8080,
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            })
            .public_endpoint(PublicEndpoint {
                name: "admin".to_string(),
                port: 9090,
                protocol: ExposeProtocol::Tcp,
                host_label: None,
                wildcard_subdomains: false,
            })
            .runtime(DaemonRuntime {
                privileged: Some(true),
                pid_namespace: Some("host".to_string()),
                network_mode: Some("bridge".to_string()),
                mounts: vec![DaemonRuntimeMount {
                    source: "relative".to_string(),
                    target: "/host".to_string(),
                    options: None,
                }],
                user: Some("root".to_string()),
            })
            .permissions("test".to_string())
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(daemon, alien_core::ResourceLifecycle::Live)
            .build();

        let check = SingleExposedPortCheck;
        let result = check.check(&stack, Platform::Aws).await.unwrap();

        assert!(!result.success);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("multiple backend ports")));
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("support only HTTP")));
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("runtime.networkMode")));
        assert!(result.errors.iter().any(|e| e.contains("runtime.user")));
        assert!(result.errors.iter().any(|e| e.contains("absolute paths")));
    }
}
