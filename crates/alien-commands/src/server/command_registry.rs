//! Command registry abstraction for command server
//!
//! The CommandRegistry is the **source of truth** for all command metadata.
//! It tracks command state, timestamps, sizes, and errors.
//!
//! Implementations:
//! - `InMemoryCommandRegistry`: In-memory implementation for tests and local dev (in this crate)
//! - `PlatformCommandRegistry`: Platform API integration (in alien-manager)
//!
//! The command KV store holds only operational data: params/response blobs, pending indices, leases.

use crate::error::{ErrorData, Result};
use alien_core::{CommandDeliveryMode, CommandState, CommandTarget, CommandTargetType};
use alien_error::AlienError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// A command target resolved by the registry, plus how commands reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommandTarget {
    /// The specific resource the command is addressed to
    pub target: CommandTarget,
    /// How commands are delivered to this target (Push or Pull)
    pub delivery_mode: CommandDeliveryMode,
}

/// Metadata returned when creating a command
#[derive(Debug, Clone)]
pub struct CommandMetadata {
    /// Unique command ID
    pub command_id: String,
    /// The specific resource the command is addressed to
    pub target: CommandTarget,
    /// How to dispatch the command (Push or Pull)
    pub delivery_mode: CommandDeliveryMode,
    /// Project ID for routing/authorization
    pub project_id: String,
}

/// Data needed to build an envelope during lease acquisition
#[derive(Debug, Clone)]
pub struct CommandEnvelopeData {
    pub command_id: String,
    pub deployment_id: String,
    pub command: String, // command name
    pub attempt: u32,
    pub deadline: Option<DateTime<Utc>>,
    pub state: CommandState,
    pub target: CommandTarget,
    pub delivery_mode: CommandDeliveryMode,
}

/// Full status for GET /commands/{id}
#[derive(Debug, Clone)]
pub struct CommandStatus {
    pub command_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub deployment_id: String,
    pub command: String, // command name
    pub state: CommandState,
    pub attempt: u32,
    pub deadline: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub dispatched_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<serde_json::Value>,
    pub request_size_bytes: Option<u64>,
    pub response_size_bytes: Option<u64>,
    pub target: CommandTarget,
}

/// Canonical ownership fields needed to authorize command reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAccessContext {
    pub workspace_id: String,
    pub project_id: String,
    pub deployment_id: String,
}

/// Internal command record stored in memory
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandRecord {
    id: String,
    deployment_id: String,
    command: String,
    state: CommandState,
    attempt: u32,
    deadline: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    dispatched_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    request_size_bytes: Option<u64>,
    response_size_bytes: Option<u64>,
    error: Option<serde_json::Value>,
    target: CommandTarget,
    delivery_mode: CommandDeliveryMode,
    project_id: String,
}

/// Reject a command-target resource id that would break the `:`-delimited key
/// grammar used by the pending index (`target:{dep}:{rid}:pending:…`) and the
/// idempotency key (`{dep}:{rid}:{command}:{key}`).
///
/// An id containing `:` could forge or collide with another target's key
/// segments, so it is rejected at the commands layer with a typed error. Only
/// the delimiter that can forge index segments is enforced at this layer.
pub fn validate_command_target_id(resource_id: &str) -> Result<()> {
    if resource_id.contains(':') {
        return Err(AlienError::new(ErrorData::CommandTargetIdInvalid {
            resource_id: resource_id.to_string(),
        }));
    }
    Ok(())
}

/// Reject a command name that would break the same `:`-delimited idempotency
/// key grammar (`{dep}:{rid}:{command}:{key}`).
///
/// The trailing client key routinely contains ':', so a ':' in the command
/// name would be indistinguishable from the key boundary: (command="a:b",
/// key="c") and (command="a", key="b:c") would forge the same key. Rejecting
/// ':' in the command name keeps the command segment unambiguous — the twin of
/// [`validate_command_target_id`] for the command segment.
pub fn validate_command_name(command: &str) -> Result<()> {
    if command.contains(':') {
        return Err(AlienError::new(ErrorData::InvalidCommand {
            message: format!("Command name '{command}' must not contain ':'"),
        }));
    }
    Ok(())
}

/// Target-selection rules shared by both the
/// in-memory and SQLite registries route through.
///
/// - `requested = Some(id)`: the id must be well-formed (no `:`) and name an
///   existing command-capable target, else `COMMAND_TARGET_NOT_FOUND`. An empty
///   id never falls back to shorthand.
/// - `requested = None` (single-target shorthand): exactly one target must
///   exist, else `COMMAND_TARGET_AMBIGUOUS` (more than one) or
///   `NO_COMMAND_TARGETS` (none).
///
/// The resolved target's own id is also validated, so a target registered with
/// a `:`-bearing id can never resolve into the key grammar.
pub fn select_command_target(
    deployment_id: &str,
    targets: &[CommandTarget],
    requested: Option<&str>,
) -> Result<CommandTarget> {
    let target = match requested {
        Some(resource_id) => {
            // Reject ids that would break the key grammar before any lookup.
            validate_command_target_id(resource_id)?;
            // An empty resource id is never a valid target — in particular it
            // must NOT silently fall back to shorthand resolution.
            let found = if resource_id.is_empty() {
                None
            } else {
                targets.iter().find(|t| t.resource_id == resource_id)
            };
            found
                .ok_or_else(|| {
                    AlienError::new(ErrorData::CommandTargetNotFound {
                        resource_id: resource_id.to_string(),
                        deployment_id: deployment_id.to_string(),
                    })
                })?
                .clone()
        }
        None => match targets {
            [] => {
                return Err(AlienError::new(ErrorData::NoCommandTargets {
                    deployment_id: deployment_id.to_string(),
                }))
            }
            [single] => single.clone(),
            _ => {
                return Err(AlienError::new(ErrorData::CommandTargetAmbiguous {
                    deployment_id: deployment_id.to_string(),
                }))
            }
        },
    };

    // A registered target whose id breaks the key grammar must never resolve.
    validate_command_target_id(&target.resource_id)?;
    Ok(target)
}

/// Pinned per-type delivery rule — the single implementation both registries
/// route through. Container and Daemon targets are always Pull; a Worker target
/// follows `worker_mode`, the caller's derived worker context (production: Push
/// only when the platform has a push path AND `stack_settings.deployment_model`
/// is Push).
pub fn delivery_mode_for(
    resource_type: CommandTargetType,
    worker_mode: CommandDeliveryMode,
) -> CommandDeliveryMode {
    match resource_type {
        CommandTargetType::Container | CommandTargetType::Daemon => CommandDeliveryMode::Pull,
        CommandTargetType::Worker => worker_mode,
    }
}

/// Abstraction for command metadata storage and lifecycle tracking.
///
/// The CommandRegistry is the source of truth for all command metadata.
/// Implementations store command state, timestamps, and result information.
#[async_trait]
pub trait CommandRegistry: Send + Sync {
    /// Resolve which command-capable resource a command is addressed to.
    ///
    /// - `requested = Some(id)`: the target must exist and be command-capable,
    ///   else `COMMAND_TARGET_NOT_FOUND` (an empty id never resolves).
    /// - `requested = None` (single-target shorthand): exactly one
    ///   command-capable target must exist, else `COMMAND_TARGET_AMBIGUOUS`
    ///   (more than one) or `NO_COMMAND_TARGETS` (none).
    ///
    /// The returned delivery mode is derived from the target: Container and
    /// Daemon targets are always Pull. Worker delivery is resolved from the
    /// deployment model and platform: Kubernetes uses its in-cluster operator
    /// relay, Local supports embedded Push and remote Pull, and cloud Workers
    /// use their provider push path only for Push deployments.
    async fn resolve_target(
        &self,
        deployment_id: &str,
        requested: Option<&str>,
    ) -> Result<ResolvedCommandTarget>;

    /// Create a new command addressed to a previously resolved target and
    /// return metadata for routing.
    ///
    /// The registry generates the command_id and stores all metadata
    /// (state, target, timestamps, etc.).
    async fn create_command(
        &self,
        deployment_id: &str,
        command_name: &str,
        target: &ResolvedCommandTarget,
        initial_state: CommandState,
        deadline: Option<DateTime<Utc>>,
        request_size_bytes: Option<u64>,
    ) -> Result<CommandMetadata>;

    /// Get metadata needed to build an envelope during lease acquisition.
    ///
    /// Returns None if command doesn't exist.
    async fn get_command_metadata(&self, command_id: &str) -> Result<Option<CommandEnvelopeData>>;

    /// Get full command status for status endpoint.
    ///
    /// Returns None if command doesn't exist.
    async fn get_command_status(&self, command_id: &str) -> Result<Option<CommandStatus>>;

    /// Get the canonical ownership fields used to authorize command reads.
    ///
    /// The status record is the command registry's source of truth for these
    /// fields, so this does not require a deployment lookup.
    async fn get_command_access_context(
        &self,
        command_id: &str,
    ) -> Result<Option<CommandAccessContext>> {
        Ok(self
            .get_command_status(command_id)
            .await?
            .map(|status| CommandAccessContext {
                workspace_id: status.workspace_id,
                project_id: status.project_id,
                deployment_id: status.deployment_id,
            }))
    }

    /// Atomically update a non-terminal command's lifecycle state.
    ///
    /// Returns `false` when the command became terminal before the update.
    /// Terminal transitions use [`Self::complete_command`] instead.
    async fn update_command_state(
        &self,
        command_id: &str,
        state: CommandState,
        dispatched_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> Result<bool>;

    /// Atomically transition a command from any NON-terminal state to the
    /// given terminal `state`.
    ///
    /// Returns `false` when the command was already terminal — a concurrent
    /// submitter won the race — so a terminal record can never be
    /// overwritten by a late duplicate (redelivered execution racing the
    /// original whose lease expired).
    async fn complete_command(
        &self,
        command_id: &str,
        state: CommandState,
        completed_at: DateTime<Utc>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> Result<bool>;

    /// Atomically mark a command Dispatched unless it is already terminal.
    ///
    /// Returns `false` when the command reached a terminal state in the
    /// meantime — e.g. a lease TTL-expired, a new poller won the takeover
    /// put, and the ORIGINAL holder's submit landed between the poller's
    /// terminal check and this write. An unconditional write there would
    /// flip a committed terminal state back to Dispatched.
    async fn mark_dispatched_if_not_terminal(
        &self,
        command_id: &str,
        dispatched_at: DateTime<Utc>,
    ) -> Result<bool>;

    /// Increment attempt count (on lease release/expiry).
    ///
    /// Returns the new attempt number.
    async fn increment_attempt(&self, command_id: &str) -> Result<u32>;
}

/// In-memory implementation for tests and local development.
///
/// Tracks command metadata in memory. Targets are registered explicitly via
/// [`register_target`](Self::register_target); resolution then follows exactly
/// the production rules documented on [`CommandRegistry::resolve_target`].
pub struct InMemoryCommandRegistry {
    commands: Arc<RwLock<HashMap<String, CommandRecord>>>,
    /// Registered command-capable targets, in registration (declaration) order.
    ///
    /// Not scoped per deployment: this registry models a single local
    /// deployment universe, so every deployment id resolves against the same
    /// target set (the per-call resolution rules are identical to production).
    targets: Arc<RwLock<Vec<CommandTarget>>>,
    /// Delivery mode for Worker targets.
    ///
    /// In production this is derived from the deployment's platform and stack
    /// settings (Push only when the platform has a push path AND
    /// `stack_settings.deployment_model == Push`). The registrant supplies
    /// that derived context here once, and the registry applies the pinned
    /// per-type rule itself — so it is impossible to register a Container or
    /// Daemon target with a Push mode that production could never produce.
    worker_delivery_mode: CommandDeliveryMode,
}

impl InMemoryCommandRegistry {
    /// Create a new in-memory registry whose Worker targets use Pull delivery
    /// (the safe default: matches platforms without a push path).
    pub fn new() -> Self {
        Self::with_worker_delivery_mode(CommandDeliveryMode::Pull)
    }

    /// Create a new in-memory registry with the specified Worker delivery mode.
    ///
    /// Container/Daemon targets are always Pull regardless of this setting.
    pub fn with_worker_delivery_mode(worker_delivery_mode: CommandDeliveryMode) -> Self {
        Self {
            commands: Arc::new(RwLock::new(HashMap::new())),
            targets: Arc::new(RwLock::new(Vec::new())),
            worker_delivery_mode,
        }
    }

    /// Register a command-capable target that commands can resolve to.
    ///
    /// Mirrors production, where the target set comes from the deployment's
    /// stack (`Stack::command_targets()`: Worker/Container/Daemon resources
    /// with `commands_enabled`). The id is validated through the same shared
    /// guard as resolution, so a `:`-bearing id is rejected here at registration
    /// rather than surfacing later as a key-grammar collision.
    pub async fn register_target(
        &self,
        resource_id: impl Into<String>,
        resource_type: CommandTargetType,
    ) -> Result<()> {
        let resource_id = resource_id.into();
        validate_command_target_id(&resource_id)?;
        self.targets
            .write()
            .await
            .push(CommandTarget::new(resource_id, resource_type));
        Ok(())
    }

    /// List all command IDs (useful for debugging/testing)
    #[allow(dead_code)]
    pub async fn list_command_ids(&self) -> Vec<String> {
        let commands = self.commands.read().await;
        commands.keys().cloned().collect()
    }
}

impl Default for InMemoryCommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandRegistry for InMemoryCommandRegistry {
    async fn resolve_target(
        &self,
        deployment_id: &str,
        requested: Option<&str>,
    ) -> Result<ResolvedCommandTarget> {
        let targets = self.targets.read().await;
        let target = select_command_target(deployment_id, &targets, requested)?;
        let delivery_mode = delivery_mode_for(target.resource_type, self.worker_delivery_mode);
        Ok(ResolvedCommandTarget {
            target,
            delivery_mode,
        })
    }

    async fn create_command(
        &self,
        deployment_id: &str,
        command_name: &str,
        target: &ResolvedCommandTarget,
        initial_state: CommandState,
        deadline: Option<DateTime<Utc>>,
        request_size_bytes: Option<u64>,
    ) -> Result<CommandMetadata> {
        let command_id = format!("cmd_{}", Uuid::new_v4());

        let record = CommandRecord {
            id: command_id.clone(),
            deployment_id: deployment_id.to_string(),
            command: command_name.to_string(),
            state: initial_state,
            attempt: 1,
            deadline,
            created_at: Utc::now(),
            dispatched_at: None,
            completed_at: None,
            request_size_bytes,
            response_size_bytes: None,
            error: None,
            target: target.target.clone(),
            delivery_mode: target.delivery_mode,
            project_id: "local-dev".to_string(),
        };

        self.commands
            .write()
            .await
            .insert(command_id.clone(), record);

        Ok(CommandMetadata {
            command_id,
            target: target.target.clone(),
            delivery_mode: target.delivery_mode,
            project_id: "local-dev".to_string(),
        })
    }

    async fn get_command_metadata(&self, command_id: &str) -> Result<Option<CommandEnvelopeData>> {
        let commands = self.commands.read().await;

        Ok(commands.get(command_id).map(|r| CommandEnvelopeData {
            command_id: r.id.clone(),
            deployment_id: r.deployment_id.clone(),
            command: r.command.clone(),
            attempt: r.attempt,
            deadline: r.deadline,
            state: r.state,
            target: r.target.clone(),
            delivery_mode: r.delivery_mode,
        }))
    }

    async fn get_command_status(&self, command_id: &str) -> Result<Option<CommandStatus>> {
        let commands = self.commands.read().await;

        Ok(commands.get(command_id).map(|r| CommandStatus {
            command_id: r.id.clone(),
            workspace_id: "default".to_string(),
            project_id: r.project_id.clone(),
            deployment_id: r.deployment_id.clone(),
            command: r.command.clone(),
            state: r.state,
            attempt: r.attempt,
            deadline: r.deadline,
            created_at: r.created_at,
            dispatched_at: r.dispatched_at,
            completed_at: r.completed_at,
            error: r.error.clone(),
            request_size_bytes: r.request_size_bytes,
            response_size_bytes: r.response_size_bytes,
            target: r.target.clone(),
        }))
    }

    async fn update_command_state(
        &self,
        command_id: &str,
        state: CommandState,
        dispatched_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> Result<bool> {
        let mut commands = self.commands.write().await;

        if let Some(record) = commands.get_mut(command_id) {
            if record.state.is_terminal() {
                return Ok(false);
            }
            record.state = state;

            if let Some(ts) = dispatched_at {
                record.dispatched_at = Some(ts);
            }

            if let Some(ts) = completed_at {
                record.completed_at = Some(ts);
            }

            if let Some(size) = response_size_bytes {
                record.response_size_bytes = Some(size);
            }

            if let Some(err) = error {
                record.error = Some(err);
            }
            return Ok(true);
        }

        Ok(false)
    }

    async fn complete_command(
        &self,
        command_id: &str,
        state: CommandState,
        completed_at: DateTime<Utc>,
        response_size_bytes: Option<u64>,
        error: Option<serde_json::Value>,
    ) -> Result<bool> {
        let mut commands = self.commands.write().await;
        let Some(record) = commands.get_mut(command_id) else {
            return Ok(false);
        };
        if record.state.is_terminal() {
            return Ok(false);
        }
        record.state = state;
        record.completed_at = Some(completed_at);
        if let Some(size) = response_size_bytes {
            record.response_size_bytes = Some(size);
        }
        if let Some(err) = error {
            record.error = Some(err);
        }
        Ok(true)
    }

    async fn mark_dispatched_if_not_terminal(
        &self,
        command_id: &str,
        dispatched_at: DateTime<Utc>,
    ) -> Result<bool> {
        let mut commands = self.commands.write().await;
        let Some(record) = commands.get_mut(command_id) else {
            return Ok(false);
        };
        if record.state.is_terminal() {
            return Ok(false);
        }
        record.state = CommandState::Dispatched;
        record.dispatched_at = Some(dispatched_at);
        Ok(true)
    }

    async fn increment_attempt(&self, command_id: &str) -> Result<u32> {
        let mut commands = self.commands.write().await;

        if let Some(record) = commands.get_mut(command_id) {
            record.attempt += 1;
            Ok(record.attempt)
        } else {
            Ok(1) // Default if not found
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{CommandDeliveryMode, CommandTarget, CommandTargetType};

    async fn resolved(
        registry: &InMemoryCommandRegistry,
        requested: Option<&str>,
    ) -> Result<ResolvedCommandTarget> {
        registry.resolve_target("dep-1", requested).await
    }

    #[tokio::test]
    async fn test_resolve_explicit_target_found() {
        let registry = InMemoryCommandRegistry::new();
        registry
            .register_target("worker-a", CommandTargetType::Worker)
            .await
            .unwrap();
        registry
            .register_target("daemon-b", CommandTargetType::Daemon)
            .await
            .unwrap();

        let result = resolved(&registry, Some("daemon-b")).await.unwrap();
        assert_eq!(
            result.target,
            CommandTarget::new("daemon-b", CommandTargetType::Daemon)
        );
    }

    #[tokio::test]
    async fn test_resolve_explicit_unknown_target_is_not_found() {
        let registry = InMemoryCommandRegistry::new();
        registry
            .register_target("worker-a", CommandTargetType::Worker)
            .await
            .unwrap();

        let err = resolved(&registry, Some("no-such-resource"))
            .await
            .unwrap_err();
        assert_eq!(err.code, "COMMAND_TARGET_NOT_FOUND");
        assert_eq!(err.http_status_code, Some(404));
    }

    #[tokio::test]
    async fn test_resolve_explicit_empty_string_is_not_found() {
        let registry = InMemoryCommandRegistry::new();
        registry
            .register_target("worker-a", CommandTargetType::Worker)
            .await
            .unwrap();

        // An explicitly requested empty resource id must never resolve (in
        // particular it must NOT fall back to shorthand resolution).
        let err = resolved(&registry, Some("")).await.unwrap_err();
        assert_eq!(err.code, "COMMAND_TARGET_NOT_FOUND");
    }

    #[tokio::test]
    async fn test_resolve_shorthand_single_target_resolves() {
        let registry = InMemoryCommandRegistry::new();
        registry
            .register_target("container-1", CommandTargetType::Container)
            .await
            .unwrap();

        let result = resolved(&registry, None).await.unwrap();
        assert_eq!(
            result.target,
            CommandTarget::new("container-1", CommandTargetType::Container)
        );
    }

    #[tokio::test]
    async fn test_resolve_shorthand_two_targets_is_ambiguous() {
        let registry = InMemoryCommandRegistry::new();
        registry
            .register_target("worker-a", CommandTargetType::Worker)
            .await
            .unwrap();
        registry
            .register_target("worker-b", CommandTargetType::Worker)
            .await
            .unwrap();

        let err = resolved(&registry, None).await.unwrap_err();
        assert_eq!(err.code, "COMMAND_TARGET_AMBIGUOUS");
        assert_eq!(err.http_status_code, Some(409));
    }

    #[tokio::test]
    async fn test_resolve_shorthand_zero_targets_is_no_targets() {
        let registry = InMemoryCommandRegistry::new();

        let err = resolved(&registry, None).await.unwrap_err();
        assert_eq!(err.code, "NO_COMMAND_TARGETS");
        assert_eq!(err.http_status_code, Some(422));
    }

    #[tokio::test]
    async fn test_delivery_mode_container_and_daemon_always_pull() {
        // Even with a Push-capable worker context, Container/Daemon are Pull.
        let registry =
            InMemoryCommandRegistry::with_worker_delivery_mode(CommandDeliveryMode::Push);
        registry
            .register_target("container-1", CommandTargetType::Container)
            .await
            .unwrap();
        registry
            .register_target("daemon-1", CommandTargetType::Daemon)
            .await
            .unwrap();

        let container = resolved(&registry, Some("container-1")).await.unwrap();
        assert_eq!(container.delivery_mode, CommandDeliveryMode::Pull);

        let daemon = resolved(&registry, Some("daemon-1")).await.unwrap();
        assert_eq!(daemon.delivery_mode, CommandDeliveryMode::Pull);
    }

    #[tokio::test]
    async fn test_delivery_mode_worker_follows_registered_context() {
        let push_registry =
            InMemoryCommandRegistry::with_worker_delivery_mode(CommandDeliveryMode::Push);
        push_registry
            .register_target("worker-1", CommandTargetType::Worker)
            .await
            .unwrap();
        let push_worker = push_registry
            .resolve_target("dep-1", Some("worker-1"))
            .await
            .unwrap();
        assert_eq!(push_worker.delivery_mode, CommandDeliveryMode::Push);

        // A manager-side pending path (e.g. Kubernetes operator relay, or a
        // cloud stack whose deployment model is Pull).
        let pull_registry =
            InMemoryCommandRegistry::with_worker_delivery_mode(CommandDeliveryMode::Pull);
        pull_registry
            .register_target("worker-1", CommandTargetType::Worker)
            .await
            .unwrap();
        let pull_worker = pull_registry
            .resolve_target("dep-1", Some("worker-1"))
            .await
            .unwrap();
        assert_eq!(pull_worker.delivery_mode, CommandDeliveryMode::Pull);
    }

    #[tokio::test]
    async fn test_create_command_stores_target_in_status_and_envelope_data() {
        let registry = InMemoryCommandRegistry::new();
        registry
            .register_target("daemon-1", CommandTargetType::Daemon)
            .await
            .unwrap();

        let resolved_target = registry.resolve_target("dep-1", None).await.unwrap();
        let metadata = registry
            .create_command(
                "dep-1",
                "sync-data",
                &resolved_target,
                CommandState::Pending,
                None,
                None,
            )
            .await
            .unwrap();

        let expected = CommandTarget::new("daemon-1", CommandTargetType::Daemon);
        assert_eq!(metadata.target, expected);
        assert_eq!(metadata.delivery_mode, CommandDeliveryMode::Pull);

        let status = registry
            .get_command_status(&metadata.command_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.target, expected);

        let envelope_data = registry
            .get_command_metadata(&metadata.command_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(envelope_data.target, expected);
        assert_eq!(envelope_data.delivery_mode, CommandDeliveryMode::Pull);
    }

    #[tokio::test]
    async fn non_terminal_update_cannot_resurrect_completed_command() {
        let registry = InMemoryCommandRegistry::new();
        registry
            .register_target("daemon-1", CommandTargetType::Daemon)
            .await
            .unwrap();
        let target = registry.resolve_target("dep-1", None).await.unwrap();
        let command = registry
            .create_command(
                "dep-1",
                "run",
                &target,
                CommandState::Dispatched,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(registry
            .complete_command(
                &command.command_id,
                CommandState::Succeeded,
                Utc::now(),
                None,
                None,
            )
            .await
            .unwrap());
        assert!(!registry
            .update_command_state(
                &command.command_id,
                CommandState::Pending,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap());
        assert_eq!(
            registry
                .get_command_status(&command.command_id)
                .await
                .unwrap()
                .unwrap()
                .state,
            CommandState::Succeeded
        );
    }

    #[tokio::test]
    async fn test_register_target_rejects_colon_in_id() {
        let registry = InMemoryCommandRegistry::new();
        let err = registry
            .register_target("evil:pending:x", CommandTargetType::Worker)
            .await
            .unwrap_err();
        assert_eq!(err.code, "COMMAND_TARGET_ID_INVALID");
        assert_eq!(err.http_status_code, Some(400));
    }

    #[tokio::test]
    async fn test_resolve_explicit_colon_id_is_invalid() {
        let registry = InMemoryCommandRegistry::new();
        registry
            .register_target("worker-a", CommandTargetType::Worker)
            .await
            .unwrap();

        // A requested id containing ':' is rejected with a typed error before
        // any lookup — it can never be resolved into the key grammar.
        let err = resolved(&registry, Some("worker-a:pending:1"))
            .await
            .unwrap_err();
        assert_eq!(err.code, "COMMAND_TARGET_ID_INVALID");
    }

    #[test]
    fn test_select_command_target_rejects_registered_colon_id() {
        // Even if a `:`-bearing target slips into the slice (e.g. an unvalidated
        // upstream path), selecting it fails loudly rather than resolving.
        let targets = vec![CommandTarget::new("a:pending:x", CommandTargetType::Worker)];
        let err = select_command_target("dep-1", &targets, None).unwrap_err();
        assert_eq!(err.code, "COMMAND_TARGET_ID_INVALID");
    }
}
