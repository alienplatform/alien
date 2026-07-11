//! Commands Polling Module
//!
//! Polls for commands from a lease endpoint and delivers them via gRPC.
//! Independent of transport - can be enabled alongside any transport.

use std::{sync::Arc, time::Duration};

use alien_commands::{
    runtime::{command_budget, submit_response, LeaseClient},
    types::{CommandResponse, CommandTarget, CommandTargetType, LeaseInfo, LeaseRequest},
    DEFAULT_LEASE_SECONDS, DEFAULT_MAX_LEASES, DEFAULT_POLL_INTERVAL_SECS,
};
use alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID;
use alien_error::{AlienError, Context};
use alien_worker_protocol::{
    control::{task::Payload, ArcCommand, Task},
    ControlGrpcServer,
};
use chrono::{DateTime, Utc};
use reqwest::Url;
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use crate::error::{ErrorData, Result};

/// Commands polling configuration
pub struct CommandsPolling {
    /// Shared lease client: holds the fully-built `…/commands/leases` endpoint
    /// (constructed once, so a bad base URL fails at startup) and the token.
    lease_client: LeaseClient,
    interval: Duration,
    deployment_id: String,
    /// The specific stack resource this runtime polls leases for (its own
    /// resource id). Leases are scoped to this target.
    target_resource_id: String,
    control_server: Arc<ControlGrpcServer>,
}

impl CommandsPolling {
    pub fn new(
        lease_client: LeaseClient,
        interval: Duration,
        deployment_id: String,
        target_resource_id: String,
        control_server: Arc<ControlGrpcServer>,
    ) -> Self {
        Self {
            lease_client,
            interval,
            deployment_id,
            target_resource_id,
            control_server,
        }
    }

    /// Build the shared lease client from a base URL, failing fast (config
    /// error) if the base cannot carry the `commands/leases` path.
    fn lease_client_from_base(url: &Url, token: String) -> Result<LeaseClient> {
        LeaseClient::from_base(url, token).ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!(
                    "Invalid commands polling URL '{url}': must be an HTTP/HTTPS URL with a path"
                ),
                field: Some("ALIEN_COMMANDS_POLLING_URL".to_string()),
            })
        })
    }

    /// Create CommandsPolling from environment variables and secrets.
    ///
    /// Reads configuration from:
    /// - `env_vars`: ALIEN_COMMANDS_POLLING_URL, ALIEN_DEPLOYMENT_ID,
    ///   ALIEN_COMMANDS_TARGET_RESOURCE_ID
    /// - `secrets`: ALIEN_COMMANDS_TOKEN
    ///
    /// Returns None if commands polling is not enabled. Errors (fail fast) if
    /// polling is enabled but a required variable — including
    /// `ALIEN_COMMANDS_TARGET_RESOURCE_ID` — is absent.
    pub fn from_env(
        env_vars: &std::collections::HashMap<String, String>,
        secrets: &std::collections::HashMap<String, String>,
        control_server: Arc<ControlGrpcServer>,
    ) -> Result<Option<Self>> {
        if env_vars
            .get("ALIEN_COMMANDS_POLLING_ENABLED")
            .map(|s| s.as_str())
            != Some("true")
        {
            return Ok(None);
        }

        info!("Starting commands polling from environment variables");

        debug!(
            secrets_count = secrets.len(),
            "Loaded secrets available for commands polling"
        );

        // Required environment variables
        let url_str = env_vars.get("ALIEN_COMMANDS_POLLING_URL").ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message:
                    "ALIEN_COMMANDS_POLLING_URL required when ALIEN_COMMANDS_POLLING_ENABLED=true"
                        .to_string(),
                field: Some("ALIEN_COMMANDS_POLLING_URL".to_string()),
            })
        })?;

        let deployment_id = env_vars.get("ALIEN_DEPLOYMENT_ID").ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: "ALIEN_DEPLOYMENT_ID required when ALIEN_COMMANDS_POLLING_ENABLED=true"
                    .to_string(),
                field: Some("ALIEN_DEPLOYMENT_ID".to_string()),
            })
        })?;

        // ALIEN-219: the target resource id names which stack resource this
        // runtime polls leases for. Required when polling is enabled — fail
        // fast, naming the variable, rather than silently leasing at the wrong
        // scope.
        let target_resource_id = env_vars
            .get(ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationInvalid {
                    message: format!(
                        "{ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID} required when \
                         ALIEN_COMMANDS_POLLING_ENABLED=true"
                    ),
                    field: Some(ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string()),
                })
            })?;

        // Token can come from secrets (managed mode) or plain env vars (dev/standalone mode)
        let token = secrets
            .get("ALIEN_COMMANDS_TOKEN")
            .or_else(|| env_vars.get("ALIEN_COMMANDS_TOKEN"))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationInvalid {
                    message:
                        "ALIEN_COMMANDS_TOKEN required when ALIEN_COMMANDS_POLLING_ENABLED=true"
                            .to_string(),
                    field: Some("ALIEN_COMMANDS_TOKEN".to_string()),
                })
            })?;

        let url = Url::parse(url_str).map_err(|e| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!("Invalid commands polling URL: {}", e),
                field: Some("ALIEN_COMMANDS_POLLING_URL".to_string()),
            })
        })?;

        let lease_client = Self::lease_client_from_base(&url, token.clone())?;

        Ok(Some(Self::new(
            lease_client,
            Duration::from_secs(DEFAULT_POLL_INTERVAL_SECS),
            deployment_id.clone(),
            target_resource_id.clone(),
            control_server,
        )))
    }

    /// Run the polling loop
    pub async fn run(&self) -> Result<()> {
        info!(
            endpoint = %self.lease_client.endpoint(),
            interval = ?self.interval,
            "Starting commands polling"
        );

        loop {
            match self.poll_once().await {
                Ok(count) => {
                    if count > 0 {
                        debug!(count = count, "Processed commands");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Commands polling error, will retry");
                }
            }

            tokio::time::sleep(self.interval).await;
        }
    }

    /// Poll once for commands.
    ///
    /// The leased batch is processed **concurrently** (each command on its own
    /// task): a command slower than the lease no longer blocks the rest of the
    /// batch, and — combined with the per-command lease budget in
    /// [`Self::process_lease`] — the manager never races a redelivery against
    /// an in-flight duplicate.
    async fn poll_once(&self) -> Result<usize> {
        let leases = self.acquire_leases().await?;
        let count = leases.len();

        let mut in_flight: JoinSet<Result<()>> = JoinSet::new();
        for lease in leases {
            in_flight.spawn(Self::process_lease(self.control_server.clone(), lease));
        }

        while let Some(joined) = in_flight.join_next().await {
            match joined {
                Ok(Ok(())) => {}
                Ok(Err(e)) => error!(error = %e, "Failed to process lease"),
                Err(e) => error!(error = %e, "Command processing task panicked"),
            }
        }

        Ok(count)
    }

    /// Task timeout for a leased command: the time remaining until its
    /// execution budget (`min(deadline, lease_expiry − safety margin)`),
    /// clamped to zero. Bounding the gRPC `send_task` timeout by the lease —
    /// rather than a fixed wall-clock constant — guarantees the runtime stops
    /// the command before the manager can redeliver it, so a slow command can
    /// never be pushed twice. Mirrors the pull receiver's `command_budget`.
    fn task_timeout(deadline: Option<DateTime<Utc>>, lease_expires_at: DateTime<Utc>) -> Duration {
        let budget = command_budget(deadline, lease_expires_at);
        (budget - Utc::now()).to_std().unwrap_or(Duration::ZERO)
    }

    /// Build the lease request this runtime sends to the manager.
    ///
    /// Extracted as a pure function (no I/O) so the request shape — in
    /// particular that it names this runtime's own target resource, typed as
    /// a Worker (a K8s/Local runtime is always a Worker target — Container/
    /// Daemon poll their own runtimes) — is directly unit-testable without a
    /// mock HTTP server.
    fn build_lease_request(&self) -> LeaseRequest {
        LeaseRequest {
            deployment_id: self.deployment_id.clone(),
            target: CommandTarget::new(self.target_resource_id.clone(), CommandTargetType::Worker),
            max_leases: DEFAULT_MAX_LEASES,
            lease_seconds: DEFAULT_LEASE_SECONDS,
        }
    }

    /// Acquire leases from command server.
    ///
    /// ALIEN-219: lease scoped to this runtime's own target resource. The
    /// manager scans only this target's pending index (a K8s/Local runtime is
    /// always a Worker target — Container/Daemon poll their own runtimes). The
    /// endpoint surgery and transport/status error shaping live in the shared
    /// `LeaseClient`; the runtime-specific error enum is applied at this
    /// boundary.
    async fn acquire_leases(&self) -> Result<Vec<LeaseInfo>> {
        self.lease_client
            .acquire(&self.build_lease_request())
            .await
            .context(ErrorData::NetworkRequestFailed {
                url: self.lease_client.endpoint().to_string(),
                method: Some("POST".to_string()),
                message: "Failed to acquire leases".to_string(),
            })
    }

    /// Process a single lease - deliver command via gRPC.
    ///
    /// Associated (not `&self`) so the leased batch can be dispatched
    /// concurrently onto a [`JoinSet`]; the only runtime state it needs is the
    /// shared control server, passed in cloned.
    async fn process_lease(control_server: Arc<ControlGrpcServer>, lease: LeaseInfo) -> Result<()> {
        let command_id = lease.command_id.clone();
        let lease_expires_at = lease.lease_expires_at;
        let envelope = lease.envelope;

        info!(
            command_id = %command_id,
            command = %envelope.command,
            "Processing command"
        );

        // Decode params. A decode failure must SUBMIT a typed error response
        // (matching the pull receiver twins and the Lambda transport) rather
        // than propagate: propagating leaves the command unanswered, so its
        // lease TTL-expires and the server redelivers it forever — a command
        // with a permanently broken params blob would never reach a terminal
        // state.
        let params = match alien_commands::runtime::decode_params_bytes(&envelope).await {
            Ok(params) => params,
            Err(e) => {
                error!(
                    command_id = %command_id,
                    error = %e,
                    "Failed to decode command params; submitting decode error"
                );
                super::shared::submit_decode_error(&envelope, &e).await;
                return Ok(());
            }
        };

        // Create task for gRPC delivery
        let task = Task {
            task_id: command_id.clone(),
            payload: Some(Payload::ArcCommand(ArcCommand {
                command_id: command_id.clone(),
                command_name: envelope.command.clone(),
                params,
                attempt: envelope.attempt,
                deadline: envelope.deadline.map(|d| prost_types::Timestamp {
                    seconds: d.timestamp(),
                    nanos: d.timestamp_subsec_nanos() as i32,
                }),
                response_url: envelope.response_handling.submit_response_url.clone(),
                storage_upload_url: envelope.response_handling.storage_upload_request.url(),
                max_inline_bytes: envelope.response_handling.max_inline_bytes,
            })),
        };

        // Bound the task by the lease: the runtime must finish (or abort) the
        // command before the lease expires, or the manager redelivers it and
        // the same command runs twice. Derive the timeout from the lease
        // expiry and the envelope deadline instead of a fixed 300s constant.
        let timeout = Self::task_timeout(envelope.deadline, lease_expires_at);

        // Send task and wait for result
        match control_server.send_task(task, timeout).await {
            Ok(result) => {
                // Submit response to commands server
                let command_response = if result.success {
                    if result.response_data.is_empty() {
                        CommandResponse::success(b"{}")
                    } else {
                        CommandResponse::success(&result.response_data)
                    }
                } else {
                    CommandResponse::error(
                        result.error_code.unwrap_or_else(|| "UNKNOWN".to_string()),
                        result
                            .error_message
                            .unwrap_or_else(|| "Unknown error".to_string()),
                    )
                };

                // submit_response returns AlienError from alien-arc, use .context()
                submit_response(&envelope, command_response).await.context(
                    ErrorData::NetworkRequestFailed {
                        url: envelope.response_handling.submit_response_url.clone(),
                        method: Some("POST".to_string()),
                        message: "Failed to submit response".to_string(),
                    },
                )?;
            }
            Err(e) => {
                // Submit error response
                let command_response = CommandResponse::error("HANDLER_ERROR", &e);
                let _ = submit_response(&envelope, command_response).await;
                return Err(AlienError::new(ErrorData::EventProcessingFailed {
                    event_type: "Command".to_string(),
                    reason: e,
                }));
            }
        }

        info!(command_id = %command_id, "Command processed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use std::collections::HashMap;

    fn control_server() -> Arc<ControlGrpcServer> {
        Arc::new(ControlGrpcServer::new())
    }

    #[test]
    fn task_timeout_never_exceeds_lease_minus_margin() {
        // A 60s lease with no envelope deadline: the task timeout is bounded by
        // the lease expiry minus the 5s safety margin (≈55s remaining), so the
        // runtime always stops before the manager can redeliver. It must never
        // be the old fixed 300s, which outlived the lease and caused duplicate
        // pushes.
        let lease_expires_at = Utc::now() + ChronoDuration::seconds(60);
        let timeout = CommandsPolling::task_timeout(None, lease_expires_at);
        assert!(
            timeout <= Duration::from_secs(55),
            "timeout {timeout:?} must not exceed lease_expiry − margin (55s)"
        );
        // Sanity: it is close to the full margined budget, not collapsed to 0.
        assert!(
            timeout >= Duration::from_secs(53),
            "timeout {timeout:?} unexpectedly small for a fresh 60s lease"
        );
    }

    #[test]
    fn task_timeout_respects_earlier_envelope_deadline() {
        // A deadline earlier than the margined lease bound wins: the command
        // gets at most its deadline, never the longer lease budget.
        let lease_expires_at = Utc::now() + ChronoDuration::seconds(60);
        let deadline = Utc::now() + ChronoDuration::seconds(10);
        let timeout = CommandsPolling::task_timeout(Some(deadline), lease_expires_at);
        assert!(
            timeout <= Duration::from_secs(10),
            "timeout {timeout:?} must be bounded by the earlier deadline"
        );
    }

    #[test]
    fn task_timeout_clamps_to_zero_when_lease_within_margin() {
        // A lease whose remaining time is already inside the safety margin
        // yields a zero timeout (never a negative/underflowed Duration): the
        // command is not run rather than run past its lease.
        let lease_expires_at = Utc::now() + ChronoDuration::seconds(2);
        assert_eq!(
            CommandsPolling::task_timeout(None, lease_expires_at),
            Duration::ZERO
        );
    }

    /// A fully-populated polling env *except* the target resource id.
    fn base_env() -> HashMap<String, String> {
        HashMap::from([
            (
                "ALIEN_COMMANDS_POLLING_ENABLED".to_string(),
                "true".to_string(),
            ),
            (
                "ALIEN_COMMANDS_POLLING_URL".to_string(),
                "https://commands.example.com".to_string(),
            ),
            ("ALIEN_DEPLOYMENT_ID".to_string(), "dep-123".to_string()),
            ("ALIEN_COMMANDS_TOKEN".to_string(), "tok".to_string()),
        ])
    }

    #[test]
    fn from_env_returns_none_when_disabled() {
        let env = HashMap::new();
        let out = CommandsPolling::from_env(&env, &HashMap::new(), control_server()).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn from_env_requires_target_resource_id() {
        // Everything present except ALIEN_COMMANDS_TARGET_RESOURCE_ID.
        let env = base_env();
        let err = match CommandsPolling::from_env(&env, &HashMap::new(), control_server()) {
            Err(e) => e,
            Ok(_) => panic!("missing target resource id must fail fast"),
        };
        assert_eq!(err.code, "CONFIGURATION_INVALID");
        assert!(
            err.message.contains(ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID),
            "error must name the missing variable, got: {}",
            err.message
        );
    }

    #[test]
    fn from_env_populates_target_resource_id() {
        let mut env = base_env();
        env.insert(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
            "worker-7".to_string(),
        );
        let polling = CommandsPolling::from_env(&env, &HashMap::new(), control_server())
            .unwrap()
            .expect("polling should be enabled");
        assert_eq!(polling.target_resource_id, "worker-7");
        assert_eq!(polling.deployment_id, "dep-123");
        // The lease endpoint is built once at config time from the base URL.
        assert_eq!(
            polling.lease_client.endpoint().as_str(),
            "https://commands.example.com/commands/leases"
        );
    }

    #[test]
    fn from_env_rejects_cannot_be_a_base_url() {
        // A URL that parses but cannot carry the `commands/leases` path is a
        // permanent config error — it must fail fast at construction, not be
        // retried (and misread as transient) on every poll.
        let mut env = base_env();
        env.insert(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
            "worker-7".to_string(),
        );
        env.insert(
            "ALIEN_COMMANDS_POLLING_URL".to_string(),
            "mailto:commands@example.com".to_string(),
        );
        let err = match CommandsPolling::from_env(&env, &HashMap::new(), control_server()) {
            Err(e) => e,
            Ok(_) => panic!("cannot-be-a-base URL must fail fast"),
        };
        assert_eq!(err.code, "CONFIGURATION_INVALID");
    }

    #[test]
    fn lease_request_carries_worker_target() {
        // Exercise the real request-building path off a `CommandsPolling`
        // constructed via `from_env` (not a hand-built `LeaseRequest`
        // literal) — proves `acquire_leases` would actually send this shape.
        let mut env = base_env();
        env.insert(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
            "worker-7".to_string(),
        );
        let polling = CommandsPolling::from_env(&env, &HashMap::new(), control_server())
            .unwrap()
            .expect("polling should be enabled");

        let request = polling.build_lease_request();

        assert_eq!(request.deployment_id, "dep-123");
        assert_eq!(request.target.resource_id, "worker-7");
        assert_eq!(request.target.resource_type, CommandTargetType::Worker);
        assert_eq!(request.max_leases, 10);
        assert_eq!(request.lease_seconds, 60);
    }

    #[test]
    fn build_lease_request_reflects_constructor_target() {
        // Two runtimes built via `new()` with different target resource ids
        // never build a lease request for the other's target — the request
        // shape is derived purely from `self`, not from any shared state.
        let lease_client = || {
            LeaseClient::from_base(
                &Url::parse("https://commands.example.com").unwrap(),
                "tok".to_string(),
            )
            .expect("valid base URL")
        };
        let polling_a = CommandsPolling::new(
            lease_client(),
            Duration::from_secs(5),
            "dep-123".to_string(),
            "worker-a".to_string(),
            control_server(),
        );
        let polling_b = CommandsPolling::new(
            lease_client(),
            Duration::from_secs(5),
            "dep-123".to_string(),
            "worker-b".to_string(),
            control_server(),
        );

        assert_eq!(
            polling_a.build_lease_request().target.resource_id,
            "worker-a"
        );
        assert_eq!(
            polling_b.build_lease_request().target.resource_id,
            "worker-b"
        );
    }
}
