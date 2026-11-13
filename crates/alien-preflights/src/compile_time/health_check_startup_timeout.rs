use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Daemon, Platform, Stack};
use async_trait::async_trait;

/// The longest startup timeout a daemon can set: 24 hours.
const MAX_STARTUP_TIMEOUT_SECONDS: u32 = 86_400;

/// Validates `healthCheck.startupTimeoutSeconds`: it applies to daemons on the Machines platform
/// only, and has to be between 1 second and 24 hours.
pub struct HealthCheckStartupTimeoutCheck;

#[async_trait]
impl CompileTimeCheck for HealthCheckStartupTimeoutCheck {
    fn description(&self) -> &'static str {
        "Validate health check startup timeouts"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources().any(|(_, entry)| {
            entry.config.downcast_ref::<Daemon>().is_some_and(|daemon| {
                daemon
                    .health_check
                    .as_ref()
                    .is_some_and(|check| check.startup_timeout_seconds.is_some())
            }) || entry
                .config
                .downcast_ref::<Container>()
                .is_some_and(|container| {
                    container
                        .health_check
                        .as_ref()
                        .is_some_and(|check| check.startup_timeout_seconds.is_some())
                })
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> crate::error::Result<CheckResult> {
        let mut failures = Vec::new();
        for (_, entry) in stack.resources() {
            if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                if let Some(seconds) = daemon
                    .health_check
                    .as_ref()
                    .and_then(|check| check.startup_timeout_seconds)
                {
                    if platform != Platform::Machines {
                        failures.push(format!(
                            "Daemon '{}': healthCheck.startupTimeoutSeconds is only enforced on the \
                             Machines platform; remove it for {}",
                            daemon.id,
                            platform.as_str()
                        ));
                        continue;
                    }
                    if !(1..=MAX_STARTUP_TIMEOUT_SECONDS).contains(&seconds) {
                        failures.push(format!(
                            "Daemon '{}': healthCheck.startupTimeoutSeconds must be between 1 and \
                             {MAX_STARTUP_TIMEOUT_SECONDS} (24 hours), got {seconds}",
                            daemon.id
                        ));
                    }
                }
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                if container
                    .health_check
                    .as_ref()
                    .is_some_and(|check| check.startup_timeout_seconds.is_some())
                {
                    failures.push(format!(
                        "Container '{}': healthCheck.startupTimeoutSeconds only applies to daemons",
                        container.id
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
    use alien_core::{ContainerCode, DaemonCode, HealthCheck, ResourceLifecycle, ResourceSpec};

    fn health_check(startup_timeout_seconds: Option<u32>) -> HealthCheck {
        HealthCheck {
            startup_timeout_seconds,
            ..HealthCheck::default()
        }
    }

    fn daemon_stack(startup_timeout_seconds: Option<u32>) -> Stack {
        let daemon = Daemon::new("agent".to_string())
            .code(DaemonCode::Image {
                image: "agent:1".to_string(),
            })
            .health_check(health_check(startup_timeout_seconds))
            .build();
        Stack::new("test".to_string())
            .add(daemon, ResourceLifecycle::Live)
            .build()
    }

    fn container_stack(startup_timeout_seconds: Option<u32>) -> Stack {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "api:1".to_string(),
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
            .health_check(health_check(startup_timeout_seconds))
            .permissions("execution".to_string())
            .build();
        Stack::new("test".to_string())
            .add(container, ResourceLifecycle::Live)
            .build()
    }

    async fn errors(stack: &Stack) -> Vec<String> {
        errors_on(stack, Platform::Machines).await
    }

    async fn errors_on(stack: &Stack, platform: Platform) -> Vec<String> {
        HealthCheckStartupTimeoutCheck
            .check(stack, platform)
            .await
            .expect("check should run")
            .errors
    }

    #[tokio::test]
    async fn accepts_one_second_through_24_hours_and_unset() {
        for seconds in [None, Some(1), Some(600), Some(86_400)] {
            assert!(
                errors(&daemon_stack(seconds)).await.is_empty(),
                "{seconds:?}"
            );
        }
        assert!(errors(&container_stack(None)).await.is_empty());
    }

    #[tokio::test]
    async fn rejects_zero_and_more_than_24_hours() {
        for seconds in [0, 86_401] {
            let errors = errors(&daemon_stack(Some(seconds))).await;
            assert_eq!(errors.len(), 1, "{seconds}");
            assert!(errors[0].contains("between 1 and 86400"), "{errors:?}");
        }
    }

    #[tokio::test]
    async fn rejects_it_on_a_container() {
        let errors = errors(&container_stack(Some(600))).await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("only applies to daemons"), "{errors:?}");
    }

    #[tokio::test]
    async fn rejects_it_on_a_platform_that_does_not_enforce_it() {
        let errors = errors_on(&daemon_stack(Some(600)), Platform::Kubernetes).await;
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].contains("only enforced on the Machines platform"),
            "{errors:?}"
        );
        assert!(errors_on(&daemon_stack(None), Platform::Kubernetes)
            .await
            .is_empty());
    }
}
