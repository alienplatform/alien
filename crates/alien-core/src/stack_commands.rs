//! Command-target discovery and the command env-var injection helpers for
//! [`Stack`]. Workers receive authenticated platform pushes; Containers and
//! Daemons run the pull receiver. Both are derived from the same ordered
//! [`Stack::command_targets`] list.

use crate::error::{ErrorData, Result};
use crate::{EnvironmentVariable, EnvironmentVariableType, Stack};
use alien_error::AlienError;

/// Builds a single [`EnvironmentVariable`] scoped to exactly one resource.
///
/// Every command env var is scoped via `target_resources` so it reaches only
/// its own resource; this collapses the otherwise-verbatim struct literals in
/// the polling/receiver builders below into one place.
fn scoped(
    name: &str,
    value: impl Into<String>,
    var_type: EnvironmentVariableType,
    target_id: &str,
) -> EnvironmentVariable {
    EnvironmentVariable {
        name: name.to_string(),
        value: value.into(),
        var_type,
        target_resources: Some(vec![target_id.to_string()]),
    }
}

impl Stack {
    /// Returns the ordered list of command-capable targets in this stack:
    /// Worker, Container, and Daemon resources with `commands_enabled` set,
    /// in stack declaration order.
    ///
    /// Declaration order is the order resources were added to the stack
    /// (via `StackBuilder::add`/`add_with_dependencies`/`add_with_remote_access`).
    /// `resources` is an `IndexMap`, which preserves insertion order, so
    /// iterating it directly yields declaration order without any extra
    /// bookkeeping.
    pub fn command_targets(&self) -> Vec<crate::commands_types::CommandTarget> {
        use crate::commands_types::{CommandTarget, CommandTargetType};

        self.resources
            .iter()
            .filter_map(|(id, entry)| {
                if let Some(worker) = entry.config.downcast_ref::<crate::Worker>() {
                    worker
                        .commands_enabled
                        .then(|| CommandTarget::new(id.clone(), CommandTargetType::Worker))
                } else if let Some(container) = entry.config.downcast_ref::<crate::Container>() {
                    container
                        .commands_enabled
                        .then(|| CommandTarget::new(id.clone(), CommandTargetType::Container))
                } else if let Some(daemon) = entry.config.downcast_ref::<crate::Daemon>() {
                    daemon
                        .commands_enabled
                        .then(|| CommandTarget::new(id.clone(), CommandTargetType::Daemon))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns the authentication token used by the manager/operator to push
    /// commands to each command-enabled Local/Kubernetes Worker runtime.
    /// Cloud-native Worker transports authenticate through their platform and
    /// do not use this helper.
    pub fn worker_command_push_env_vars(
        &self,
        commands_token: Option<&str>,
    ) -> Result<Vec<EnvironmentVariable>> {
        use crate::commands_types::CommandTargetType;

        let mut vars = Vec::new();
        for target in self
            .command_targets()
            .into_iter()
            .filter(|target| target.resource_type == CommandTargetType::Worker)
        {
            let id = target.resource_id;
            let token = require_command_token(commands_token, &id)?;
            vars.push(scoped(
                crate::ENV_ALIEN_COMMANDS_TOKEN,
                token,
                EnvironmentVariableType::Secret,
                &id,
            ));
        }
        Ok(vars)
    }

    /// Returns the command *receiver* environment variables for each
    /// command-enabled Container and Daemon in this stack, scoped via
    /// `target_resources` so every var reaches only its own resource.
    ///
    /// Workers receive platform pushes; Containers and Daemons run the pull
    /// receiver, which reads this fixed environment contract:
    ///   - `ALIEN_COMMANDS_URL` (Plain) — base receiver URL
    ///   - `ALIEN_COMMANDS_TOKEN` (Secret, only if a token is present)
    ///   - `ALIEN_COMMANDS_TARGET_RESOURCE_ID` (Plain) — this resource's id
    ///   - `ALIEN_COMMANDS_TARGET_RESOURCE_TYPE` (Plain) — `container`/`daemon`
    ///
    /// `ALIEN_DEPLOYMENT_ID` is intentionally NOT emitted here: the manager and
    /// operator already inject it deployment-wide (`target_resources: None`), so
    /// it reaches every Container/Daemon via that path — re-scoping it per
    /// resource would be redundant. This mirrors the worker helper, which also
    /// relies on the deployment-wide `ALIEN_DEPLOYMENT_ID`.
    ///
    /// Workers are excluded here, and a
    /// commands-disabled Container/Daemon is never a `command_targets()` entry,
    /// so it receives nothing — the receiver fail-fasts on a partial config, so
    /// a deployment-wide flag would crash it at startup.
    pub fn receiver_command_env_vars(
        &self,
        commands_url: &str,
        commands_token: Option<&str>,
    ) -> Result<Vec<EnvironmentVariable>> {
        use crate::commands_types::CommandTargetType;

        let mut vars = Vec::new();
        for target in self.command_targets().into_iter().filter(|target| {
            matches!(
                target.resource_type,
                CommandTargetType::Container | CommandTargetType::Daemon
            )
        }) {
            let resource_type = target.resource_type.as_str();
            let id = target.resource_id;
            // Four of the five receiver vars without the token would make
            // `Receiver::from_env` crash-loop the workload at startup; fail
            // the deploy here instead, where the cause is nameable.
            let token = require_command_token(commands_token, &id)?;
            vars.extend([
                scoped(
                    crate::ENV_ALIEN_COMMANDS_URL,
                    commands_url,
                    EnvironmentVariableType::Plain,
                    &id,
                ),
                scoped(
                    crate::ENV_ALIEN_COMMANDS_TOKEN,
                    token,
                    EnvironmentVariableType::Secret,
                    &id,
                ),
                scoped(
                    crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
                    resource_type,
                    EnvironmentVariableType::Plain,
                    &id,
                ),
                scoped(
                    crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
                    id.clone(),
                    EnvironmentVariableType::Plain,
                    &id,
                ),
            ]);
        }
        Ok(vars)
    }
}

/// The deployment token, or a deploy-time error naming the command-enabled
/// resource that needs it — a runtime crash-loop on a missing env var is the
/// only alternative.
fn require_command_token<'a>(token: Option<&'a str>, resource_id: &str) -> Result<&'a str> {
    token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::CommandTokenMissing {
                resource_id: resource_id.to_string(),
                reason: "the deployment record carries no non-empty deployment token".to_string(),
            })
        })
}

#[cfg(test)]
mod tests {
    use crate::resource::ResourceLifecycle;
    use crate::{
        Container, ContainerCode, Daemon, DaemonCode, ResourceSpec, Stack, Storage, Worker,
        WorkerCode,
    };

    #[test]
    fn command_targets_returns_only_commands_enabled_resources_in_declaration_order() {
        let worker_enabled = Worker::new("worker-a".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let container_disabled = Container::new("container-b".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .build();

        let daemon_enabled = Daemon::new("daemon-c".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();

        let container_enabled = Container::new("container-d".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();

        let worker_disabled = Worker::new("worker-e".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let storage = Storage::new("bucket-f".to_string()).build();

        // Declaration order: worker-a (enabled), container-b (disabled),
        // daemon-c (enabled), container-d (enabled), worker-e (disabled),
        // bucket-f (not a command-capable resource type at all).
        let stack = Stack::new("command-targets-stack".to_string())
            .add(worker_enabled, ResourceLifecycle::Live)
            .add(container_disabled, ResourceLifecycle::Live)
            .add(daemon_enabled, ResourceLifecycle::Live)
            .add(container_enabled, ResourceLifecycle::Live)
            .add(worker_disabled, ResourceLifecycle::Live)
            .add(storage, ResourceLifecycle::Frozen)
            .build();

        let targets = stack.command_targets();

        assert_eq!(
            targets,
            vec![
                crate::commands_types::CommandTarget::new(
                    "worker-a",
                    crate::commands_types::CommandTargetType::Worker
                ),
                crate::commands_types::CommandTarget::new(
                    "daemon-c",
                    crate::commands_types::CommandTargetType::Daemon
                ),
                crate::commands_types::CommandTarget::new(
                    "container-d",
                    crate::commands_types::CommandTargetType::Container
                ),
            ]
        );
    }

    #[test]
    fn command_targets_empty_when_no_commands_enabled_resources() {
        let worker = Worker::new("worker-only".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let stack = Stack::new("no-targets-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();

        assert!(stack.command_targets().is_empty());
    }

    #[test]
    fn worker_command_push_env_vars_scopes_secret_per_worker() {
        let worker_a = Worker::new("worker-a".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let worker_b = Worker::new("worker-b".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        // Commands-disabled Workers must not expose a command push endpoint.
        let worker_disabled = Worker::new("worker-off".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let daemon_enabled = Daemon::new("daemon-c".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("worker-push-env-stack".to_string())
            .add(worker_a, ResourceLifecycle::Live)
            .add(worker_b, ResourceLifecycle::Live)
            .add(worker_disabled, ResourceLifecycle::Live)
            .add(daemon_enabled, ResourceLifecycle::Live)
            .build();

        let vars = stack
            .worker_command_push_env_vars(Some("tok"))
            .expect("token present");

        // Every var is scoped to exactly one command-enabled Worker — nothing
        // is deployment-wide, and neither the disabled Worker nor the Daemon
        // is ever a scope target.
        assert!(vars.iter().all(|v| {
            v.target_resources == Some(vec!["worker-a".to_string()])
                || v.target_resources == Some(vec!["worker-b".to_string()])
        }));

        // Each command-enabled Worker gets only its scoped push token.
        for worker_id in ["worker-a", "worker-b"] {
            let scoped: Vec<_> = vars
                .iter()
                .filter(|v| v.target_resources == Some(vec![worker_id.to_string()]))
                .collect();
            assert_eq!(scoped.len(), 1, "expected one push token for {worker_id}");
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TOKEN
                    && v.value == "tok"
                    && v.var_type == crate::EnvironmentVariableType::Secret
            }));
        }
    }

    #[test]
    fn worker_command_push_env_vars_fails_without_token() {
        let worker = Worker::new("worker-a".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("worker-no-token-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();

        // A command-enabled worker without a token would expose no usable push
        // endpoint, so deployment must fail loudly.
        for token in [None, Some(""), Some("   \t")] {
            let error = stack
                .worker_command_push_env_vars(token)
                .expect_err("missing or blank token must fail the env build");
            assert_eq!(error.code, "COMMAND_TOKEN_MISSING");
            assert!(error.to_string().contains("worker-a"));
        }

        let original = "  nonempty-token  ";
        let vars = stack
            .worker_command_push_env_vars(Some(original))
            .expect("non-empty token");
        assert_eq!(vars[0].value, original, "token bytes must be preserved");
    }

    #[test]
    fn receiver_command_env_vars_scopes_contract_per_container_and_daemon() {
        let container_a = Container::new("container-a".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();

        let daemon_b = Daemon::new("daemon-b".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();

        // Commands-DISABLED container: must receive NONE of the receiver vars.
        let container_off = Container::new("container-off".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .build();

        // Commands-enabled Worker gets push auth, not the receiver contract.
        let worker_enabled = Worker::new("worker-c".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("receiver-env-stack".to_string())
            .add(container_a, ResourceLifecycle::Live)
            .add(daemon_b, ResourceLifecycle::Live)
            .add(container_off, ResourceLifecycle::Live)
            .add(worker_enabled, ResourceLifecycle::Live)
            .build();

        let vars = stack
            .receiver_command_env_vars("https://cmd.example.test/v1", Some("tok"))
            .expect("token present");

        // Every var is scoped to exactly one command-enabled Container/Daemon —
        // nothing is deployment-wide, and neither the disabled container nor the
        // Worker is ever a scope target.
        assert!(vars.iter().all(|v| {
            v.target_resources == Some(vec!["container-a".to_string()])
                || v.target_resources == Some(vec!["daemon-b".to_string()])
        }));

        // ALIEN_DEPLOYMENT_ID remains deployment-wide and is not duplicated.
        assert!(!vars
            .iter()
            .any(|v| v.name == crate::ENV_ALIEN_DEPLOYMENT_ID));

        for (resource_id, expected_type) in [("container-a", "container"), ("daemon-b", "daemon")] {
            let scoped: Vec<_> = vars
                .iter()
                .filter(|v| v.target_resources == Some(vec![resource_id.to_string()]))
                .collect();
            assert_eq!(
                scoped.len(),
                4,
                "expected 4 receiver vars for {resource_id}"
            );
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_URL
                    && v.value == "https://cmd.example.test/v1"
                    && v.var_type == crate::EnvironmentVariableType::Plain
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TOKEN
                    && v.value == "tok"
                    && v.var_type == crate::EnvironmentVariableType::Secret
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID && v.value == resource_id
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE
                    && v.value == expected_type
                    && v.var_type == crate::EnvironmentVariableType::Plain
            }));
        }
    }

    #[test]
    fn receiver_command_env_vars_fails_without_token() {
        let container = Container::new("container-a".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();

        let daemon = Daemon::new("daemon-b".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("receiver-no-token-stack".to_string())
            .add(container, ResourceLifecycle::Live)
            .add(daemon, ResourceLifecycle::Live)
            .build();

        // Four of the five receiver vars without the token would make
        // Receiver::from_env crash-loop at startup; the deploy must fail.
        for token in [None, Some(""), Some("   \n")] {
            let error = stack
                .receiver_command_env_vars("https://cmd.example.test/v1", token)
                .expect_err("missing or blank token must fail the env build");
            assert_eq!(error.code, "COMMAND_TOKEN_MISSING");
            assert!(error.to_string().contains("container-a"));
        }

        let original = "  nonempty-token  ";
        let vars = stack
            .receiver_command_env_vars("https://cmd.example.test/v1", Some(original))
            .expect("non-empty token");
        assert!(vars
            .iter()
            .filter(|var| var.name == crate::ENV_ALIEN_COMMANDS_TOKEN)
            .all(|var| var.value == original));
    }

    #[test]
    fn receiver_command_env_vars_empty_without_command_targets() {
        let worker = Worker::new("worker-only".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("receiver-worker-only-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();

        // Only a Worker target exists → receiver helper yields nothing.
        assert!(stack
            .receiver_command_env_vars("https://cmd.example.test/v1", Some("tok"))
            .expect("no command targets")
            .is_empty());
    }
}
