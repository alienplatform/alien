use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Daemon, Platform, Stack};
use async_trait::async_trait;

/// Refuses AWS, GCP and Azure daemons that combine workload permissions with host access.
///
/// On these platforms a daemon's `permissions` profile becomes a cloud identity assigned to the
/// daemon alone. That assignment is only enforceable for a daemon isolated from its machine. A
/// privileged daemon, or one on the host network, in the host PID namespace or with host mounts,
/// can reach the machine's own credentials, so the compute runtime refuses to start it with an
/// assigned identity. Without this check the stack builds and releases, and the refusal only
/// shows up when the daemon is provisioned.
///
/// Machines daemons get no assigned identity, and Kubernetes and Local schedule daemons
/// differently, so only AWS, GCP and Azure run the check.
///
/// This is a build-time check only. A stack released before it existed already fails when the
/// runtime refuses the daemon, so deployment preflights don't repeat it.
pub struct DaemonHostAccessPermissionsCheck;

#[async_trait]
impl CompileTimeCheck for DaemonHostAccessPermissionsCheck {
    fn code(&self) -> Option<&'static str> {
        Some("DAEMON_HOST_ACCESS_WITH_PERMISSIONS")
    }

    fn description(&self) -> &'static str {
        "Daemons with workload permissions run without host access"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && stack
                .resources()
                .any(|(_, entry)| entry.config.downcast_ref::<Daemon>().is_some())
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> crate::error::Result<CheckResult> {
        let errors: Vec<String> = stack
            .resources()
            .filter_map(|(resource_id, entry)| {
                let daemon = entry.config.downcast_ref::<Daemon>()?;
                let profile = daemon.permissions.as_deref()?;
                let host_access = host_access_settings(daemon);
                (!host_access.is_empty()).then(|| {
                    format!(
                        "Daemon '{resource_id}' has permissions '{profile}' and host access ({}). On {platform}, a daemon with permissions gets its own cloud identity, which only an isolated daemon can hold: with host access it could use the machine's credentials instead. Remove the permissions to run it as a trusted host daemon, or remove the host access settings.",
                        host_access.join(", ")
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

/// The runtime settings that give a daemon access to its machine, as the user wrote them.
fn host_access_settings(daemon: &Daemon) -> Vec<&'static str> {
    let Some(runtime) = &daemon.runtime else {
        return Vec::new();
    };
    let mut settings = Vec::new();
    if runtime.privileged == Some(true) {
        settings.push("runtime.privileged");
    }
    if runtime.network_mode.as_deref() == Some("host") {
        settings.push("runtime.networkMode 'host'");
    }
    if runtime.pid_namespace.as_deref() == Some("host") {
        settings.push("runtime.pidNamespace 'host'");
    }
    if !runtime.mounts.is_empty() {
        settings.push("runtime.mounts");
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ErrorData, runner::PreflightRunner};
    use alien_core::{
        DaemonCode, DaemonRuntime, DaemonRuntimeMount, PermissionProfile, ResourceLifecycle,
    };

    const CODE: &str = "DAEMON_HOST_ACCESS_WITH_PERMISSIONS";

    fn daemon(id: &str, permissions: bool, runtime: Option<DaemonRuntime>) -> Daemon {
        Daemon::new(id.to_string())
            .code(DaemonCode::Image {
                image: format!("example.test/{id}:latest"),
            })
            .maybe_permissions(permissions.then(|| "probe".to_string()))
            .maybe_runtime(runtime)
            .build()
    }

    fn privileged() -> DaemonRuntime {
        DaemonRuntime {
            privileged: Some(true),
            ..DaemonRuntime::default()
        }
    }

    fn stack_of(daemons: Vec<Daemon>) -> Stack {
        let mut builder = Stack::new("test-stack".to_string());
        for daemon in daemons {
            builder = builder.add(daemon, ResourceLifecycle::Live);
        }
        let mut stack = builder.build();
        stack.permissions.profiles.insert(
            "probe".to_string(),
            PermissionProfile::new().global(Vec::<&str>::new()),
        );
        stack
    }

    /// Every host access setting with permissions, next to the combinations the runtime accepts.
    fn mixed_stack() -> Stack {
        stack_of(vec![
            daemon("privileged-sa", true, Some(privileged())),
            daemon(
                "host-network-sa",
                true,
                Some(DaemonRuntime {
                    network_mode: Some("host".to_string()),
                    ..DaemonRuntime::default()
                }),
            ),
            daemon(
                "host-pid-sa",
                true,
                Some(DaemonRuntime {
                    pid_namespace: Some("host".to_string()),
                    ..DaemonRuntime::default()
                }),
            ),
            daemon(
                "mount-sa",
                true,
                Some(DaemonRuntime {
                    mounts: vec![DaemonRuntimeMount {
                        source: "/var/run".to_string(),
                        target: "/host/var/run".to_string(),
                        options: Some("ro".to_string()),
                    }],
                    ..DaemonRuntime::default()
                }),
            ),
            // Accepted: isolated settings with permissions, and host access without them.
            daemon(
                "isolated-sa",
                true,
                Some(DaemonRuntime {
                    privileged: Some(false),
                    network_mode: Some("appnet".to_string()),
                    pid_namespace: Some("private".to_string()),
                    user: Some("1000".to_string()),
                    ..DaemonRuntime::default()
                }),
            ),
            daemon("default-sa", true, None),
            daemon("privileged-no-sa", false, Some(privileged())),
        ])
    }

    fn validation_results(error: alien_error::AlienError<ErrorData>) -> Vec<CheckResult> {
        let ErrorData::ValidationFailed { results, .. } =
            error.error.expect("validation failure carries its results")
        else {
            panic!("expected a validation failure");
        };
        results
    }

    fn refused(results: &[CheckResult]) -> Vec<&str> {
        results
            .iter()
            .filter(|result| result.code.as_deref() == Some(CODE))
            .flat_map(|result| result.errors.iter())
            .map(|error| {
                error
                    .split('\'')
                    .nth(1)
                    .expect("each failure names its daemon")
            })
            .collect()
    }

    /// `alien build` refuses exactly the daemons the runtime would refuse at provisioning, on
    /// every platform that assigns daemons a cloud identity.
    #[tokio::test]
    async fn build_refuses_daemons_with_permissions_and_host_access() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let error = PreflightRunner::new()
                .run_build_time_preflights(&mixed_stack(), platform)
                .await
                .expect_err("a daemon with permissions and host access must fail the build");
            let results = validation_results(error);

            assert_eq!(
                refused(&results),
                vec![
                    "privileged-sa",
                    "host-network-sa",
                    "host-pid-sa",
                    "mount-sa"
                ],
                "{platform}"
            );
            let other_failures: Vec<_> = results
                .iter()
                .filter(|result| !result.success && result.code.as_deref() != Some(CODE))
                .collect();
            assert!(other_failures.is_empty(), "{platform}: {other_failures:?}");

            let message = results
                .iter()
                .filter(|result| result.code.as_deref() == Some(CODE))
                .flat_map(|result| result.errors.iter())
                .next()
                .expect("a failure message");
            assert!(
                message.contains("permissions 'probe'") && message.contains("runtime.privileged"),
                "{message}"
            );
        }
    }

    /// `alien release --prebuilt` runs the same rule on a saved stack.
    #[tokio::test]
    async fn prebuilt_release_refuses_daemons_with_permissions_and_host_access() {
        let error = PreflightRunner::new()
            .run_prebuilt_release_checks(&mixed_stack(), Platform::Aws)
            .await
            .expect_err("a saved stack with such a daemon must be refused");

        assert_eq!(
            refused(&validation_results(error)),
            vec![
                "privileged-sa",
                "host-network-sa",
                "host-pid-sa",
                "mount-sa"
            ]
        );
    }

    /// The accepted combinations build on their own.
    #[tokio::test]
    async fn build_accepts_isolated_daemons_with_permissions_and_host_daemons_without() {
        let stack = stack_of(vec![
            daemon("default-sa", true, None),
            daemon("privileged-no-sa", false, Some(privileged())),
        ]);
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let summary = PreflightRunner::new()
                .run_build_time_preflights(&stack, platform)
                .await
                .expect("these daemons must build");
            assert!(summary.success, "{platform}: {:?}", summary.results);
        }
    }

    /// Machines assigns daemons no identity; Kubernetes and Local run daemons differently.
    #[tokio::test]
    async fn other_platforms_do_not_run_the_check() {
        let stack = mixed_stack();
        for platform in [Platform::Machines, Platform::Kubernetes, Platform::Local] {
            assert!(
                !DaemonHostAccessPermissionsCheck.should_run(&stack, platform),
                "{platform}"
            );
        }
    }
}
