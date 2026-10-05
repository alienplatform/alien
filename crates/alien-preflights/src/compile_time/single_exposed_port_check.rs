use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Daemon, ExposeProtocol, Platform, Stack, Worker, APEX_HOST_LABEL};
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
        validate_unique_host_labels(stack, &mut failures);

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

    if let Some(tunnel) = &container.tunnel {
        if !container.ports.iter().any(|port| port.port == tunnel.port) {
            failures.push(format!(
                "Container '{}': tunnel references undeclared port {}",
                container.id, tunnel.port
            ));
        }
    }

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
        (
            Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Local,
            ExposeProtocol::Tcp,
        ) => true,
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

/// A public endpoint that gets a generated hostname: `<hostLabel>.<deployment domain>`.
struct HostnameEndpoint<'a> {
    resource_id: &'a str,
    endpoint_name: &'a str,
    host_label: &'a str,
    /// The resource exists only when its `enabledWhen` input is true.
    gated: bool,
}

/// Generated hostnames share one deployment domain, so two endpoints in the same stack with
/// the same host label (the endpoint name unless `hostLabel` overrides it) would get the same
/// hostname. Only HTTP endpoints on containers and daemons get one; TCP endpoints are reached
/// through their load balancer address.
///
/// Two gated resources may be alternatives that are never enabled together, so they may share
/// a host label; the deployment rejects them if both are enabled. A collision involving an
/// ungated resource happens whenever the other resource exists.
fn validate_unique_host_labels(stack: &Stack, failures: &mut Vec<String>) {
    let mut endpoints = Vec::new();
    for (resource_id, resource_entry) in stack.resources() {
        let config = &resource_entry.config;
        let gated = resource_entry.enabled_when.is_some();
        let http_endpoints = config
            .downcast_ref::<Container>()
            .map(|container| container.public_endpoints.as_slice())
            .or_else(|| {
                config
                    .downcast_ref::<Daemon>()
                    .map(|daemon| daemon.public_endpoints.as_slice())
            })
            .unwrap_or_default();
        endpoints.extend(
            http_endpoints
                .iter()
                .filter(|endpoint| endpoint.protocol == ExposeProtocol::Http)
                .map(|endpoint| HostnameEndpoint {
                    resource_id,
                    endpoint_name: &endpoint.name,
                    host_label: endpoint.effective_host_label(),
                    gated,
                }),
        );
        if let Some(worker) = config.downcast_ref::<Worker>() {
            endpoints.extend(
                worker
                    .public_endpoints
                    .iter()
                    .map(|endpoint| HostnameEndpoint {
                        resource_id,
                        endpoint_name: &endpoint.name,
                        host_label: endpoint.effective_host_label(),
                        gated,
                    }),
            );
        }
    }

    for (index, endpoint) in endpoints.iter().enumerate() {
        let conflict = endpoints[..index].iter().find(|earlier| {
            earlier.host_label == endpoint.host_label
                && !(earlier.gated && endpoint.gated)
                // Two endpoints on one resource with the same name are reported by the
                // per-resource checks; here they would only repeat that message.
                && !(earlier.resource_id == endpoint.resource_id
                    && earlier.endpoint_name == endpoint.endpoint_name)
        });
        let Some(first) = conflict else {
            continue;
        };
        let hostname = if endpoint.host_label == APEX_HOST_LABEL {
            "the deployment's apex hostname".to_string()
        } else {
            format!("hostname '{}.<deployment domain>'", endpoint.host_label)
        };
        failures.push(format!(
            "Public endpoints '{}' on '{}' and '{}' on '{}' would both get {}. Endpoint host labels must be unique within a stack: rename one endpoint or give it a different hostLabel.",
            first.endpoint_name,
            first.resource_id,
            endpoint.endpoint_name,
            endpoint.resource_id,
            hostname,
        ));
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
        Stack, WorkerCode, WorkerPublicEndpoint,
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

    fn container_with_endpoint(id: &str, endpoint: PublicEndpoint) -> Container {
        Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: format!("example.test/{id}:latest"),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(endpoint.port)
            .public_endpoint(endpoint)
            .replicas(1)
            .permissions("test".to_string())
            .build()
    }

    fn http_endpoint(name: &str, host_label: Option<&str>) -> PublicEndpoint {
        PublicEndpoint {
            name: name.to_string(),
            port: 8080,
            protocol: ExposeProtocol::Http,
            host_label: host_label.map(str::to_string),
            wildcard_subdomains: false,
        }
    }

    fn worker_with_endpoint(id: &str, name: &str, host_label: Option<&str>) -> Worker {
        Worker::new(id.to_string())
            .code(WorkerCode::Image {
                image: format!("example.test/{id}:latest"),
            })
            .permissions("test".to_string())
            .public_endpoint(WorkerPublicEndpoint {
                name: name.to_string(),
                host_label: host_label.map(str::to_string),
                wildcard_subdomains: false,
            })
            .build()
    }

    /// The case that reached staging: two resources each exposing an endpoint named "api".
    /// `alien release` builds the stack through the build-time preflights, so the release must
    /// be refused there, naming both resources.
    #[tokio::test]
    async fn release_build_rejects_two_resources_with_the_same_endpoint_name() {
        let mut stack = Stack::new("test-stack".to_string())
            .add(
                container_with_endpoint("gateway", http_endpoint("api", None)),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                container_with_endpoint("probe", http_endpoint("api", None)),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        // Everything else about this stack is valid, so the endpoint collision is the only
        // reason the build fails.
        stack.permissions.profiles.insert(
            "test".to_string(),
            alien_core::PermissionProfile::new().global(Vec::<&str>::new()),
        );

        let error = crate::runner::PreflightRunner::new()
            .run_build_time_preflights(&stack, Platform::Aws)
            .await
            .expect_err("duplicate endpoint names must fail the build");
        let crate::error::ErrorData::ValidationFailed { results, .. } =
            error.error.expect("validation failure carries its results")
        else {
            panic!("expected a validation failure");
        };
        let messages: Vec<&String> = results
            .iter()
            .flat_map(|result| result.errors.iter())
            .collect();
        assert_eq!(
            messages,
            vec![
                "Public endpoints 'api' on 'gateway' and 'api' on 'probe' would both get hostname 'api.<deployment domain>'. Endpoint host labels must be unique within a stack: rename one endpoint or give it a different hostLabel."
            ]
        );
    }

    #[tokio::test]
    async fn host_label_override_collides_across_resource_types() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                container_with_endpoint("gateway", http_endpoint("api", None)),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                worker_with_endpoint("hooks", "webhooks", Some("api")),
                alien_core::ResourceLifecycle::Live,
            )
            .build();

        let result = SingleExposedPortCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("preflight should run");
        assert_eq!(
            result.errors,
            vec![
                "Public endpoints 'api' on 'gateway' and 'webhooks' on 'hooks' would both get hostname 'api.<deployment domain>'. Endpoint host labels must be unique within a stack: rename one endpoint or give it a different hostLabel."
            ]
        );
    }

    #[tokio::test]
    async fn apex_endpoints_on_two_resources_collide() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                container_with_endpoint("site", http_endpoint("web", Some(APEX_HOST_LABEL))),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                worker_with_endpoint("edge", "root", Some(APEX_HOST_LABEL)),
                alien_core::ResourceLifecycle::Live,
            )
            .build();

        let result = SingleExposedPortCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("preflight should run");
        assert_eq!(
            result.errors,
            vec![
                "Public endpoints 'web' on 'site' and 'root' on 'edge' would both get the deployment's apex hostname. Endpoint host labels must be unique within a stack: rename one endpoint or give it a different hostLabel."
            ]
        );
    }

    /// Two gated resources can be alternatives selected by deploy-time inputs, so the release
    /// cannot know they collide. An ungated resource always exists, so it still conflicts.
    #[tokio::test]
    async fn only_ungated_resources_make_a_host_label_conflict_certain() {
        let mut stack = Stack::new("test-stack".to_string())
            .add(
                worker_with_endpoint("primary", "api", None),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                worker_with_endpoint("secondary", "api", None),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        for id in ["primary", "secondary"] {
            stack.resources.get_mut(id).expect("resource").enabled_when =
                Some(format!("enable-{id}"));
        }

        let alternatives = SingleExposedPortCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("preflight should run");
        assert!(alternatives.success, "{:?}", alternatives.errors);

        stack
            .resources
            .get_mut("secondary")
            .expect("resource")
            .enabled_when = None;
        let result = SingleExposedPortCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("preflight should run");
        assert_eq!(
            result.errors,
            vec![
                "Public endpoints 'api' on 'primary' and 'api' on 'secondary' would both get hostname 'api.<deployment domain>'. Endpoint host labels must be unique within a stack: rename one endpoint or give it a different hostLabel."
            ]
        );
    }

    #[tokio::test]
    async fn same_endpoint_name_with_distinct_host_labels_passes() {
        let tcp_endpoint = PublicEndpoint {
            name: "api".to_string(),
            port: 5432,
            protocol: ExposeProtocol::Tcp,
            host_label: None,
            wildcard_subdomains: false,
        };
        let stack = Stack::new("test-stack".to_string())
            .add(
                container_with_endpoint("gateway", http_endpoint("api", None)),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                container_with_endpoint("probe", http_endpoint("api", Some("probe"))),
                alien_core::ResourceLifecycle::Live,
            )
            // TCP endpoints are reached through their load balancer address, not a
            // generated hostname, so their name cannot collide.
            .add(
                container_with_endpoint("database", tcp_endpoint),
                alien_core::ResourceLifecycle::Live,
            )
            .build();

        let result = SingleExposedPortCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("preflight should run");
        assert!(result.success, "{:?}", result.errors);
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
    async fn container_tcp_is_supported_on_cloud_and_local_container_platforms() {
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
            let expected = matches!(
                platform,
                Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Local
            );
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
