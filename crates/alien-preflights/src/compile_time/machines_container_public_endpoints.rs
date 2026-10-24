use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};
use async_trait::async_trait;

/// Refuses public endpoints on containers in machines deployments.
///
/// Container replicas on machines listen on host ports Horizon assigns, and the endpoint's DNS
/// record lists every host, so nothing serves the endpoint's HTTPS address. Daemons run on the
/// host network and serve their declared port, so they keep their public endpoints.
///
/// This is a build-time check only. Stacks released before it existed may already contain such
/// containers, and deployments of them must keep updating, so deployment-time and template
/// preflights do not run it.
pub struct MachinesContainerPublicEndpointsCheck;

#[async_trait]
impl CompileTimeCheck for MachinesContainerPublicEndpointsCheck {
    fn code(&self) -> Option<&'static str> {
        Some("MACHINES_CONTAINER_PUBLIC_ENDPOINT")
    }

    fn description(&self) -> &'static str {
        "Machines deployments serve public endpoints only from daemons"
    }

    fn should_run(&self, _stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Machines
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> crate::error::Result<CheckResult> {
        let errors: Vec<String> = stack
            .resources()
            .filter_map(|(resource_id, entry)| {
                let container = entry.config.downcast_ref::<Container>()?;
                (!container.public_endpoints.is_empty()).then(|| {
                    format!(
                        "Public endpoints on container '{resource_id}' are not supported on platform 'machines'. Use a daemon for public endpoints on machines deployments."
                    )
                })
            })
            .collect();

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
    use crate::{error::ErrorData, runner::PreflightRunner};
    use alien_core::{
        ContainerCode, Daemon, DaemonCode, ExposeProtocol, PermissionProfile, PublicEndpoint,
        ResourceLifecycle, ResourceSpec,
    };

    fn http_endpoint() -> PublicEndpoint {
        PublicEndpoint {
            name: "web".to_string(),
            port: 8080,
            protocol: ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        }
    }

    fn container(id: &str) -> Container {
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
            .port(8080)
            .replicas(1)
            .permissions("test".to_string())
            .build()
    }

    fn stack_with(container: Container) -> Stack {
        let mut stack = Stack::new("test-stack".to_string())
            .add(container, ResourceLifecycle::Live)
            .build();
        stack.permissions.profiles.insert(
            "test".to_string(),
            PermissionProfile::new().global(Vec::<&str>::new()),
        );
        stack
    }

    fn stack_with_public_container() -> Stack {
        let mut web = container("web");
        web.public_endpoints.push(http_endpoint());
        stack_with(web)
    }

    const WEB_REFUSED: &str = "Public endpoints on container 'web' are not supported on platform 'machines'. Use a daemon for public endpoints on machines deployments.";

    /// `alien build` and `alien release` run the build-time preflights, so a new machines
    /// release with a public container is refused there.
    #[tokio::test]
    async fn build_refuses_public_container_on_machines() {
        let error = PreflightRunner::new()
            .run_build_time_preflights(&stack_with_public_container(), Platform::Machines)
            .await
            .expect_err("a public container must fail a machines build");
        let ErrorData::ValidationFailed { results, .. } =
            error.error.expect("validation failure carries its results")
        else {
            panic!("expected a validation failure");
        };
        let messages: Vec<&String> = results
            .iter()
            .flat_map(|result| result.errors.iter())
            .collect();
        assert_eq!(messages, vec![WEB_REFUSED]);
    }

    /// A release published before this check existed may already carry such a container. The
    /// checks that run while deploying it, and while rendering its setup template, must keep
    /// accepting it so its deployments can still update.
    #[tokio::test]
    async fn deployment_and_template_preflights_keep_accepting_released_stacks() {
        let stack = stack_with_public_container();
        let runner = PreflightRunner::new();

        let compile = runner
            .run_compile_time_checks(&stack, Platform::Machines)
            .await
            .expect("compile-time checks should run");
        assert!(compile.success, "{:?}", compile.results);
        runner
            .run_template_preflights(&stack, Platform::Machines)
            .await
            .expect("template preflights should accept the released stack");
    }

    #[tokio::test]
    async fn daemon_with_public_endpoint_and_private_container_pass() {
        let daemon = Daemon::new("gateway".to_string())
            .code(DaemonCode::Image {
                image: "example.test/gateway:latest".to_string(),
            })
            .public_endpoint(http_endpoint())
            .permissions("test".to_string())
            .build();
        let stack = Stack::new("test-stack".to_string())
            .add(daemon, ResourceLifecycle::Live)
            .add(container("worker"), ResourceLifecycle::Live)
            .build();

        let result = MachinesContainerPublicEndpointsCheck
            .check(&stack, Platform::Machines)
            .await
            .expect("check should run");
        assert!(result.success, "{:?}", result.errors);
        assert!(result.errors.is_empty());
    }

    /// Other platforms route container endpoints through a load balancer or ingress.
    #[tokio::test]
    async fn other_platforms_do_not_run_the_check() {
        let stack = stack_with_public_container();
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
        ] {
            assert!(!MachinesContainerPublicEndpointsCheck.should_run(&stack, platform));
        }
        assert!(MachinesContainerPublicEndpointsCheck.should_run(&stack, Platform::Machines));
    }
}
