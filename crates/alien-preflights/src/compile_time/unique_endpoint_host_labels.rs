use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Daemon, ExposeProtocol, Platform, Stack, Worker, APEX_HOST_LABEL};
use async_trait::async_trait;

/// Refuses stacks in which two public endpoints would get the same generated hostname.
///
/// Generated hostnames are `<hostLabel>.<deployment domain>` (the host label is the endpoint
/// name unless `hostLabel` overrides it), so two resources exposing an endpoint named "api"
/// collide in every deployment of the stack.
///
/// This is a build-time check only. Stacks released before it existed may already contain such
/// endpoints, and deployments of them must keep updating, so deployment-time and template
/// preflights do not run it.
pub struct UniqueEndpointHostLabelsCheck;

#[async_trait]
impl CompileTimeCheck for UniqueEndpointHostLabelsCheck {
    fn description(&self) -> &'static str {
        "Validate that public endpoints get distinct hostnames"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources().any(|(_, entry)| {
            entry.config.downcast_ref::<Container>().is_some()
                || entry.config.downcast_ref::<Daemon>().is_some()
                || entry.config.downcast_ref::<Worker>().is_some()
        })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> crate::error::Result<CheckResult> {
        let failures = host_label_conflicts(stack);
        if failures.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(failures))
        }
    }
}

/// A public endpoint that gets a generated hostname.
struct HostnameEndpoint<'a> {
    resource_id: &'a str,
    endpoint_name: &'a str,
    host_label: &'a str,
    /// The resource exists only when its `enabledWhen` input is true.
    gated: bool,
}

/// Only HTTP endpoints on containers and daemons, and every worker endpoint, get a generated
/// hostname; TCP endpoints are reached through their load balancer address.
///
/// Two gated resources may be alternatives that are never enabled together, so they may share a
/// host label. A collision involving an ungated resource happens whenever the other resource
/// exists.
fn host_label_conflicts(stack: &Stack) -> Vec<String> {
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

    let mut failures = Vec::new();
    for (index, endpoint) in endpoints.iter().enumerate() {
        let conflict = endpoints[..index].iter().find(|earlier| {
            earlier.host_label == endpoint.host_label
                && !(earlier.gated && endpoint.gated)
                // Two endpoints on one resource with the same name are reported by the
                // per-resource endpoint checks; here they would only repeat that message.
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
    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ErrorData, runner::PreflightRunner};
    use alien_core::{
        ContainerCode, PermissionProfile, PublicEndpoint, ResourceLifecycle, ResourceSpec,
        WorkerCode, WorkerPublicEndpoint,
    };

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

    /// Two resources each exposing an endpoint named "api", and otherwise valid.
    fn stack_with_shared_endpoint_name() -> Stack {
        let mut stack = Stack::new("test-stack".to_string())
            .add(
                container_with_endpoint("gateway", http_endpoint("api", None)),
                ResourceLifecycle::Live,
            )
            .add(
                container_with_endpoint("probe", http_endpoint("api", None)),
                ResourceLifecycle::Live,
            )
            .build();
        stack.permissions.profiles.insert(
            "test".to_string(),
            PermissionProfile::new().global(Vec::<&str>::new()),
        );
        stack
    }

    const GATEWAY_PROBE_CONFLICT: &str = "Public endpoints 'api' on 'gateway' and 'api' on 'probe' would both get hostname 'api.<deployment domain>'. Endpoint host labels must be unique within a stack: rename one endpoint or give it a different hostLabel.";

    /// `alien build` and `alien release` build the stack through the build-time preflights, so
    /// the stack is refused there, naming both resources.
    #[tokio::test]
    async fn build_refuses_two_resources_with_the_same_endpoint_name() {
        let error = PreflightRunner::new()
            .run_build_time_preflights(&stack_with_shared_endpoint_name(), Platform::Aws)
            .await
            .expect_err("duplicate endpoint names must fail the build");
        let ErrorData::ValidationFailed { results, .. } =
            error.error.expect("validation failure carries its results")
        else {
            panic!("expected a validation failure");
        };
        let messages: Vec<&String> = results
            .iter()
            .flat_map(|result| result.errors.iter())
            .collect();
        assert_eq!(messages, vec![GATEWAY_PROBE_CONFLICT]);
    }

    /// A release published before this check existed may already carry such endpoints. The
    /// checks that run while deploying it, and while rendering its setup template, must keep
    /// accepting it so its deployments can still update.
    #[tokio::test]
    async fn deployment_and_template_preflights_keep_accepting_released_stacks() {
        let stack = stack_with_shared_endpoint_name();
        let runner = PreflightRunner::new();

        let compile = runner
            .run_compile_time_checks(&stack, Platform::Aws)
            .await
            .expect("compile-time checks should run");
        assert!(compile.success, "{:?}", compile.results);
        runner
            .run_template_preflights(&stack, Platform::Aws)
            .await
            .expect("template preflights should accept the released stack");
    }

    #[tokio::test]
    async fn host_label_override_collides_across_resource_types() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                container_with_endpoint("gateway", http_endpoint("api", None)),
                ResourceLifecycle::Live,
            )
            .add(
                worker_with_endpoint("hooks", "webhooks", Some("api")),
                ResourceLifecycle::Live,
            )
            .build();

        let result = UniqueEndpointHostLabelsCheck
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
                ResourceLifecycle::Live,
            )
            .add(
                worker_with_endpoint("edge", "root", Some(APEX_HOST_LABEL)),
                ResourceLifecycle::Live,
            )
            .build();

        let result = UniqueEndpointHostLabelsCheck
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

    /// Two gated resources can be alternatives selected by deploy-time inputs, so the build
    /// cannot know they collide. An ungated resource always exists, so it still conflicts.
    #[tokio::test]
    async fn only_ungated_resources_make_a_host_label_conflict_certain() {
        let mut stack = Stack::new("test-stack".to_string())
            .add(
                worker_with_endpoint("primary", "api", None),
                ResourceLifecycle::Live,
            )
            .add(
                worker_with_endpoint("secondary", "api", None),
                ResourceLifecycle::Live,
            )
            .build();
        for id in ["primary", "secondary"] {
            stack.resources.get_mut(id).expect("resource").enabled_when =
                Some(format!("enable-{id}"));
        }

        let alternatives = UniqueEndpointHostLabelsCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("preflight should run");
        assert!(alternatives.success, "{:?}", alternatives.errors);

        stack
            .resources
            .get_mut("secondary")
            .expect("resource")
            .enabled_when = None;
        let result = UniqueEndpointHostLabelsCheck
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
                ResourceLifecycle::Live,
            )
            .add(
                container_with_endpoint("probe", http_endpoint("api", Some("probe"))),
                ResourceLifecycle::Live,
            )
            // TCP endpoints are reached through their load balancer address, not a
            // generated hostname, so their name cannot collide.
            .add(
                container_with_endpoint("database", tcp_endpoint),
                ResourceLifecycle::Live,
            )
            .build();

        let result = UniqueEndpointHostLabelsCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("preflight should run");
        assert!(result.success, "{:?}", result.errors);
    }
}
