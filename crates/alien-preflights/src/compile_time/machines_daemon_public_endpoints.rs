use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Daemon, Platform, Stack};
use async_trait::async_trait;

/// Refuses public endpoints on machines daemons that do not use host networking.
///
/// On machines, a public endpoint publishes an `https://` address that only a process listening
/// on port 443 of the machine itself can answer. A daemon with `runtime.networkMode: "host"`
/// binds its ports on the machine and can do that. A daemon without it runs on the bridge
/// network, where its public port is a dynamic port on the machine, so nothing answers the
/// published address. An unset network mode is bridge networking.
///
/// Only the network mode is checked. A host-network daemon may route its endpoint to another
/// backend port while serving 443 itself, so the endpoint port says nothing here.
///
/// This is a build-time check only, like `MachinesContainerPublicEndpointsCheck`. Stacks released
/// before it existed keep deploying, updating and rendering setup templates.
pub struct MachinesDaemonPublicEndpointsCheck;

#[async_trait]
impl CompileTimeCheck for MachinesDaemonPublicEndpointsCheck {
    fn code(&self) -> Option<&'static str> {
        Some("MACHINES_DAEMON_PUBLIC_ENDPOINT")
    }

    fn description(&self) -> &'static str {
        "Machines daemons serve public endpoints only on host networking"
    }

    fn should_run(&self, _stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Machines
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> crate::error::Result<CheckResult> {
        let errors: Vec<String> = stack
            .resources()
            .filter_map(|(resource_id, entry)| {
                let daemon = entry.config.downcast_ref::<Daemon>()?;
                (!daemon.public_endpoints.is_empty() && !uses_host_network(daemon)).then(|| {
                    format!(
                        "Public endpoints on daemon '{resource_id}' are not supported on platform 'machines' without host networking. On machines, public endpoints are served only by a daemon with runtime.networkMode 'host' that serves HTTPS on port 443 itself. Set runtime.networkMode to 'host', or remove the public endpoints."
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

fn uses_host_network(daemon: &Daemon) -> bool {
    daemon
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.network_mode.as_deref())
        == Some("host")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ErrorData, runner::PreflightRunner};
    use alien_core::{
        DaemonCode, DaemonRuntime, ExposeProtocol, PermissionProfile, PublicEndpoint,
        ResourceLifecycle,
    };

    const CODE: &str = "MACHINES_DAEMON_PUBLIC_ENDPOINT";

    fn daemon(id: &str, network_mode: Option<&str>, public: bool) -> Daemon {
        let builder = Daemon::new(id.to_string())
            .code(DaemonCode::Image {
                image: format!("example.test/{id}:latest"),
            })
            .permissions("test".to_string())
            .maybe_runtime(network_mode.map(|mode| DaemonRuntime {
                network_mode: Some(mode.to_string()),
                ..DaemonRuntime::default()
            }));
        if public {
            // Host-network daemons in use route their endpoint to a backend port such as 8080
            // while serving 443 themselves, so the fixtures do the same.
            builder
                .public_endpoint(PublicEndpoint {
                    name: id.to_string(),
                    port: 8080,
                    protocol: ExposeProtocol::Http,
                    host_label: None,
                    wildcard_subdomains: false,
                })
                .build()
        } else {
            builder.build()
        }
    }

    fn stack_of(daemons: Vec<Daemon>) -> Stack {
        let mut builder = Stack::new("test-stack".to_string());
        for daemon in daemons {
            builder = builder.add(daemon, ResourceLifecycle::Live);
        }
        let mut stack = builder.build();
        stack.permissions.profiles.insert(
            "test".to_string(),
            PermissionProfile::new().global(Vec::<&str>::new()),
        );
        stack
    }

    /// A stack mixing every daemon kind: only the public bridge daemons may be refused.
    fn mixed_stack() -> Stack {
        stack_of(vec![
            daemon("host-public", Some("host"), true),
            daemon("unset-public", None, true),
            daemon("appnet-public", Some("appnet"), true),
            daemon("unset-private", None, false),
            daemon("appnet-private", Some("appnet"), false),
        ])
    }

    /// Resources named by this check's failures, in stack order.
    fn refused(results: &[CheckResult]) -> Vec<String> {
        results
            .iter()
            .filter(|result| result.code.as_deref() == Some(CODE))
            .flat_map(|result| result.errors.iter())
            .map(|error| {
                error
                    .split('\'')
                    .nth(1)
                    .expect("each failure names its daemon")
                    .to_string()
            })
            .collect()
    }

    fn validation_results(error: alien_error::AlienError<ErrorData>) -> Vec<CheckResult> {
        let ErrorData::ValidationFailed { results, .. } =
            error.error.expect("validation failure carries its results")
        else {
            panic!("expected a validation failure");
        };
        results
    }

    /// `alien build` refuses exactly the public daemons without host networking.
    #[tokio::test]
    async fn build_refuses_public_daemons_without_host_networking() {
        let error = PreflightRunner::new()
            .run_build_time_preflights(&mixed_stack(), Platform::Machines)
            .await
            .expect_err("public bridge daemons must fail a machines build");
        let results = validation_results(error);

        assert_eq!(refused(&results), vec!["unset-public", "appnet-public"]);
        let other_failures: Vec<_> = results
            .iter()
            .filter(|result| !result.success && result.code.as_deref() != Some(CODE))
            .collect();
        assert!(other_failures.is_empty(), "{other_failures:?}");
    }

    /// `alien release --prebuilt` runs the same rule on a saved stack, so a build saved by an
    /// older CLI cannot skip it.
    #[tokio::test]
    async fn prebuilt_release_refuses_public_daemons_without_host_networking() {
        let error = PreflightRunner::new()
            .run_prebuilt_release_checks(&mixed_stack(), Platform::Machines)
            .await
            .expect_err("a saved machines stack with a public bridge daemon must be refused");

        assert_eq!(
            refused(&validation_results(error)),
            vec!["unset-public", "appnet-public"]
        );
    }

    /// The configuration in use today, public host-network daemons next to private daemons,
    /// passes every build-time check.
    #[tokio::test]
    async fn build_accepts_public_host_network_daemons() {
        let stack = stack_of(vec![
            daemon("host-public", Some("host"), true),
            daemon("unset-private", None, false),
        ]);

        let summary = PreflightRunner::new()
            .run_build_time_preflights(&stack, Platform::Machines)
            .await
            .expect("a public host-network daemon must build for machines");
        assert!(summary.success, "{:?}", summary.results);
    }

    /// Releases published before this check may already carry such a daemon. Deploying,
    /// updating and rendering their setup template must keep accepting them.
    #[tokio::test]
    async fn deployment_and_template_preflights_keep_accepting_released_stacks() {
        let stack = mixed_stack();
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

    /// Other platforms route daemon endpoints through a load balancer or ingress.
    #[tokio::test]
    async fn other_platforms_do_not_run_the_check() {
        let stack = mixed_stack();
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            assert!(!MachinesDaemonPublicEndpointsCheck.should_run(&stack, platform));
        }
        assert!(MachinesDaemonPublicEndpointsCheck.should_run(&stack, Platform::Machines));
    }
}
