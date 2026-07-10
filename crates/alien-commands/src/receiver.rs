//! App-owned pull command receiver for Containers and Daemons.
//!
//! A [`Receiver`] leases commands addressed to its own target resource from
//! the command server over outbound HTTPS (no inbound connections, no gRPC),
//! dispatches them to in-process handlers, and submits responses through the
//! envelope's response-handling flow (inline or presigned storage upload).
//!
//! # Bootstrap
//!
//! ```no_run
//! # async fn example() -> alien_commands::error::Result<()> {
//! let mut receiver = alien_commands::Receiver::from_env()?;
//! receiver.handle("generate-report", |ctx| async move {
//!     let params: serde_json::Value = ctx.input_json()?;
//!     Ok(serde_json::json!({ "report": params }))
//! });
//! receiver.run().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Execution budget
//!
//! Every command runs under a budget of `min(envelope.deadline, lease_expiry
//! − LEASE_SAFETY_MARGIN)` — there is no lease-renew call. Subtracting the
//! safety margin guarantees the response is submitted (or the handler
//! abandoned) before the lease actually expires, so an expired lease never
//! races an in-flight duplicate. When the budget expires the handler *future*
//! is aborted (dropped), `ctx.cancellation` is cancelled, and a
//! `HANDLER_TIMEOUT` error response is submitted.
//!
//! Dropping the handler future does not, by itself, stop any background work
//! the handler spawned onto its own tasks (`tokio::spawn`, detached I/O,
//! etc.) — those keep running unless they observe `ctx.cancellation`
//! themselves. Handlers that spawn such work should race it against
//! `ctx.cancellation.cancelled()`, e.g.:
//!
//! ```no_run
//! # use alien_commands::receiver::Context;
//! # async fn handle(ctx: Context) -> alien_commands::receiver::HandlerResult<()> {
//! tokio::select! {
//!     result = do_cooperative_work() => { result?; }
//!     _ = ctx.cancellation.cancelled() => {
//!         // budget expired: stop cleanly instead of leaking work.
//!     }
//! }
//! # Ok(())
//! # }
//! # async fn do_cooperative_work() -> alien_commands::receiver::HandlerResult<()> { Ok(()) }
//! ```
//!
//! # At-least-once delivery
//!
//! A lease that expires without a submitted response is redelivered, so
//! handlers must tolerate at-least-once execution. `ctx.attempt` carries the
//! delivery attempt (starting at 1); a value greater than 1 means redelivery.
//!
//! # Shutdown
//!
//! [`Receiver::run`] returns after [`ShutdownHandle::shutdown`] is called.
//! Worded precisely: no new lease poll *starts* once draining begins (this is
//! checked at the top of each poll-loop iteration) — a poll already in
//! flight when shutdown is raised still completes, and any leases it returns
//! are dispatched and drained like the rest of the batch. Every in-flight
//! command finishes within its own budget before `run` returns, and no
//! command created after shutdown is ever leased. Wire the handle to your
//! process signal handling (e.g. `tokio::signal::ctrl_c`) as needed.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use alien_core::{
    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
    ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_COMMANDS_URL, ENV_ALIEN_DEPLOYMENT_ID,
};
use alien_error::{AlienError, Context as _, IntoAlienError};
use chrono::{DateTime, Utc};
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::task::JoinSet;
pub use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::{
    error::{ErrorData, Result},
    runtime::{command_budget, decode_params_bytes, submit_response, LeaseClient},
    types::{
        CommandResponse, CommandTarget, CommandTargetType, LeaseInfo, LeaseRequest,
        DEFAULT_LEASE_SECONDS, DEFAULT_MAX_LEASES, DEFAULT_POLL_INTERVAL_SECS,
    },
};

/// Error code submitted when a leased command has no registered handler.
pub const ERROR_CODE_UNKNOWN_COMMAND: &str = "UNKNOWN_COMMAND";
/// Error code submitted when a handler exceeds its execution budget.
pub const ERROR_CODE_HANDLER_TIMEOUT: &str = "HANDLER_TIMEOUT";
/// Error code submitted when a handler returns an error (or its response
/// fails to serialize).
pub const ERROR_CODE_HANDLER_ERROR: &str = "HANDLER_ERROR";

/// Error type handlers may return: anything convertible into a boxed error,
/// so `?` works on most error types inside handlers.
pub type HandlerError = Box<dyn std::error::Error + Send + Sync>;

/// Result type returned by command handlers.
pub type HandlerResult<T> = std::result::Result<T, HandlerError>;

/// Type-erased handler: takes a [`Context`], returns JSON response bytes or
/// an error message (submitted as `HANDLER_ERROR`).
///
/// Not part of the public API — exposed only so the observability tests in
/// `tests/receiver_tests.rs` can drive [`process_lease`]/[`box_handler`]
/// directly.
#[doc(hidden)]
pub type BoxedHandler = Arc<
    dyn Fn(Context) -> Pin<Box<dyn Future<Output = std::result::Result<Vec<u8>, String>> + Send>>
        + Send
        + Sync,
>;

/// Per-command context passed to handlers.
///
/// Mirrors the TypeScript receiver's handler context fields
/// (`input`, `signal`, `deadline`, `commandId`, `attempt`).
#[derive(Debug, Clone)]
pub struct Context {
    /// Decoded command params (raw bytes; JSON for JSON-invoked commands).
    /// Use [`Context::input_json`] to deserialize.
    pub input: Vec<u8>,
    /// The command's effective execution budget:
    /// `min(envelope.deadline, lease_expiry − LEASE_SAFETY_MARGIN)`. Under
    /// lease-based delivery this is always `Some` (the lease expiry bounds
    /// it); kept optional to mirror the TS context shape.
    pub deadline: Option<DateTime<Utc>>,
    /// Unique command identifier.
    pub command_id: String,
    /// Delivery attempt, starting at 1. Greater than 1 means redelivery
    /// (at-least-once semantics).
    pub attempt: u32,
    /// Cancelled when the execution budget expires. The handler future is
    /// aborted regardless; use this to stop cooperative work the handler
    /// spawned (the `ctx.signal` equivalent of the TS receiver).
    pub cancellation: CancellationToken,
}

impl Context {
    /// Deserialize the command input as JSON.
    pub fn input_json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.input)
            .into_alien_error()
            .context(ErrorData::SerializationFailed {
                message: "Failed to parse command input as JSON".to_string(),
                data_type: Some(std::any::type_name::<T>().to_string()),
            })
    }
}

/// Handle to stop a running [`Receiver`]; obtained via
/// [`Receiver::shutdown_handle`] and safe to clone across tasks.
#[derive(Debug, Clone)]
pub struct ShutdownHandle(CancellationToken);

impl ShutdownHandle {
    /// Signal the receiver to stop: no new lease poll starts once draining
    /// begins (a poll already in flight still completes and its leases are
    /// processed), in-flight commands drain within their budgets, then
    /// [`Receiver::run`] returns. See the module docs' "Shutdown" section
    /// for the precise semantics.
    pub fn shutdown(&self) {
        self.0.cancel();
    }
}

/// Pull command receiver: leases commands addressed to this process's
/// target resource and dispatches them to registered handlers.
pub struct Receiver {
    lease_client: LeaseClient,
    deployment_id: String,
    target: CommandTarget,
    poll_interval: Duration,
    max_leases: usize,
    lease_seconds: u64,
    handlers: HashMap<String, BoxedHandler>,
    shutdown: CancellationToken,
}

impl std::fmt::Debug for Receiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Manual impl: handlers are opaque closures and the token is secret.
        f.debug_struct("Receiver")
            .field("endpoint", &self.lease_client.endpoint().as_str())
            .field("deployment_id", &self.deployment_id)
            .field("target", &self.target)
            .field("poll_interval", &self.poll_interval)
            .field("max_leases", &self.max_leases)
            .field("lease_seconds", &self.lease_seconds)
            .field("handlers", &self.handlers.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Receiver {
    /// Build a receiver from the process environment.
    ///
    /// Required variables (all fail fast with
    /// `COMMAND_RECEIVER_CONFIG_INVALID` naming the variable when missing,
    /// empty, or invalid):
    ///
    /// - `ALIEN_COMMANDS_URL` — base URL of the command server API
    /// - `ALIEN_COMMANDS_TOKEN` — bearer token for outbound requests
    /// - `ALIEN_DEPLOYMENT_ID` — deployment the commands belong to
    /// - `ALIEN_COMMANDS_TARGET_RESOURCE_ID` — this resource's id
    /// - `ALIEN_COMMANDS_TARGET_RESOURCE_TYPE` — `container` or `daemon`
    pub fn from_env() -> Result<Self> {
        let env: HashMap<String, String> = std::env::vars().collect();
        Self::from_env_vars(&env)
    }

    /// Build a receiver from an explicit environment map. Same contract as
    /// [`Receiver::from_env`]; useful for tests and embedding.
    pub fn from_env_vars(env: &HashMap<String, String>) -> Result<Self> {
        let url_str = require_env(env, ENV_ALIEN_COMMANDS_URL)?;
        let url = Url::parse(url_str).map_err(|e| {
            AlienError::new(ErrorData::CommandReceiverConfigInvalid {
                message: format!("{ENV_ALIEN_COMMANDS_URL} is not a valid URL: {e}"),
                env_var: ENV_ALIEN_COMMANDS_URL.to_string(),
            })
        })?;
        let token = require_env(env, ENV_ALIEN_COMMANDS_TOKEN)?.clone();
        let deployment_id = require_env(env, ENV_ALIEN_DEPLOYMENT_ID)?.clone();
        let resource_id = require_env(env, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID)?.clone();
        let resource_type =
            match require_env(env, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE)?.as_str() {
                "container" => CommandTargetType::Container,
                "daemon" => CommandTargetType::Daemon,
                other => {
                    return Err(AlienError::new(ErrorData::CommandReceiverConfigInvalid {
                        message: format!(
                            "{ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE} must be 'container' or \
                         'daemon', got '{other}'"
                        ),
                        env_var: ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE.to_string(),
                    }))
                }
            };

        // Build the `…/commands/leases` endpoint once, at config time. A base
        // URL that cannot be a hierarchical URL is a permanent misconfiguration
        // and must fail fast here — not be re-derived (and misread as a
        // transient error) on every poll.
        let lease_client = LeaseClient::from_base(&url, token).ok_or_else(|| {
            AlienError::new(ErrorData::CommandReceiverConfigInvalid {
                message: format!("{ENV_ALIEN_COMMANDS_URL} '{url}' must be an HTTP(S) URL with a path"),
                env_var: ENV_ALIEN_COMMANDS_URL.to_string(),
            })
        })?;

        Ok(Self {
            lease_client,
            deployment_id,
            target: CommandTarget::new(resource_id, resource_type),
            poll_interval: Duration::from_secs(DEFAULT_POLL_INTERVAL_SECS),
            max_leases: DEFAULT_MAX_LEASES,
            lease_seconds: DEFAULT_LEASE_SECONDS,
            handlers: HashMap::new(),
            shutdown: CancellationToken::new(),
        })
    }

    /// Override the lease poll interval (default 5s). Mainly for tests.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Override the requested lease duration (default 60s). The lease expiry
    /// bounds each command's execution budget. Mainly for tests.
    pub fn with_lease_seconds(mut self, lease_seconds: u64) -> Self {
        self.lease_seconds = lease_seconds;
        self
    }

    /// Register a handler for a command name.
    ///
    /// The handler receives a [`Context`] and returns any serializable
    /// value, submitted as the command's JSON success response. A returned
    /// error is submitted as a `HANDLER_ERROR` response. Registering the
    /// same name twice replaces the previous handler.
    pub fn handle<F, Fut, T>(&mut self, name: impl Into<String>, handler: F) -> &mut Self
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<T>> + Send + 'static,
        T: Serialize + 'static,
    {
        self.handlers.insert(name.into(), box_handler(handler));
        self
    }

    /// Get a handle that stops this receiver's [`run`](Receiver::run) loop.
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle(self.shutdown.clone())
    }

    /// Drive the lease loop until shutdown.
    ///
    /// Polls `POST {url}/commands/leases` every poll interval, dispatches
    /// each leased command to its handler concurrently, and submits
    /// responses. Transient lease errors are logged and retried on the next
    /// interval. Returns after [`ShutdownHandle::shutdown`]: no new lease
    /// poll starts once draining begins (a poll already in flight still
    /// completes and its leases are processed), and every in-flight command
    /// is awaited, each bounded by its own budget. See the module docs'
    /// "Shutdown" section for the precise semantics.
    pub async fn run(&self) -> Result<()> {
        info!(
            endpoint = %self.lease_client.endpoint(),
            deployment_id = %self.deployment_id,
            target_resource_id = %self.target.resource_id,
            target_resource_type = ?self.target.resource_type,
            "Starting command receiver"
        );

        let mut in_flight: JoinSet<()> = JoinSet::new();

        loop {
            if self.shutdown.is_cancelled() {
                break;
            }

            match self.acquire_leases().await {
                Ok(leases) => {
                    for mut lease in leases {
                        // The manager mints the pre-authorized submit URL from
                        // its own base address, which may not be reachable from
                        // behind a container/NAT boundary. The configured
                        // commands URL is the address the platform corrected
                        // for this receiver's network (leases flow through it),
                        // and the submit endpoint lives on the same manager —
                        // rebase onto that origin, keeping the pre-authorized
                        // path and response token. Twin of the TS receiver's
                        // `rebaseOntoCommandsOrigin`.
                        lease.envelope.response_handling.submit_response_url =
                            rebase_onto_commands_origin(
                                &lease.envelope.response_handling.submit_response_url,
                                self.lease_client.endpoint(),
                            );
                        let handler = self.handlers.get(&lease.envelope.command).cloned();
                        in_flight.spawn(process_lease(handler, lease, self.target.clone()));
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to acquire command leases, will retry");
                }
            }

            // Reap finished commands so the set doesn't grow unbounded.
            while in_flight.try_join_next().is_some() {}

            tokio::select! {
                _ = self.shutdown.cancelled() => break,
                _ = tokio::time::sleep(self.poll_interval) => {}
            }
        }

        if !in_flight.is_empty() {
            info!(
                in_flight = in_flight.len(),
                "Receiver shutting down, draining in-flight commands"
            );
        }
        while in_flight.join_next().await.is_some() {}

        info!("Command receiver stopped");
        Ok(())
    }

    /// Build the lease request this receiver sends. Pure (no I/O) so the
    /// request shape is directly unit-testable.
    fn build_lease_request(&self) -> LeaseRequest {
        LeaseRequest {
            deployment_id: self.deployment_id.clone(),
            target: self.target.clone(),
            max_leases: self.max_leases,
            lease_seconds: self.lease_seconds,
        }
    }

    async fn acquire_leases(&self) -> Result<Vec<LeaseInfo>> {
        // The endpoint and error mapping live in the shared `LeaseClient`; the
        // receiver just supplies its own request shape. Errors already carry
        // this crate's `ErrorData`, so no boundary remap is needed.
        self.lease_client.acquire(&self.build_lease_request()).await
    }
}

/// Type-erase a handler: run it, then serialize its return value to JSON
/// bytes (a serialization failure is reported like a handler error).
///
/// Not part of the public API — see [`BoxedHandler`].
#[doc(hidden)]
pub fn box_handler<F, Fut, T>(handler: F) -> BoxedHandler
where
    F: Fn(Context) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<T>> + Send + 'static,
    T: Serialize + 'static,
{
    Arc::new(move |ctx| {
        let fut = handler(ctx);
        Box::pin(async move {
            let value = fut.await.map_err(|e| e.to_string())?;
            serde_json::to_vec(&value)
                .map_err(|e| format!("Failed to serialize handler response: {e}"))
        })
    })
}

/// Handler-status label for a produced response: `"success"` for a success
/// response, otherwise the error code (`UNKNOWN_COMMAND` / `HANDLER_ERROR` /
/// `HANDLER_TIMEOUT` / a params-decode code). Twin of the TypeScript
/// receiver's `commandResponseStatus`.
fn command_response_status(response: &CommandResponse) -> &str {
    match response {
        CommandResponse::Success { .. } => "success",
        CommandResponse::Error { code, .. } => code,
    }
}

/// Rebase a manager-minted absolute URL onto the origin of the receiver's
/// configured commands endpoint, preserving path and query.
///
/// Twin of the TS receiver's `rebaseOntoCommandsOrigin` — see the call site in
/// [`Receiver::run`] for why. Returns the input unchanged when either URL
/// fails to parse (an unparseable target then fails at submit time with the
/// real error; the configured endpoint was already validated at construction).
///
/// Known limitation: only the origin is swapped — a reverse proxy that mounts
/// the manager under a path prefix the manager itself does not know (base
/// `https://edge/prefix/v1` vs minted `…/v1/commands/…`) still breaks, because
/// the prefix cannot be reconstructed client-side. The manager's own base-URL
/// path (e.g. `/v1`) rides inside the minted path and is preserved.
fn rebase_onto_commands_origin(target: &str, commands_endpoint: &Url) -> String {
    let Ok(target_url) = Url::parse(target) else {
        return target.to_string();
    };
    if target_url.origin() == commands_endpoint.origin() {
        return target.to_string();
    }
    let mut rebased = commands_endpoint.clone();
    rebased.set_path(target_url.path());
    rebased.set_query(target_url.query());
    rebased.set_fragment(None);
    rebased.to_string()
}

/// Process one leased command end to end: execute (or reject) it and submit
/// the response through the envelope's response-handling flow.
///
/// Emits one structured `Command processed` observability event carrying the
/// pinned receiver fields — command id, lease id, target resource id/type,
/// attempt, deadline, handler status, submit-response status. The TypeScript
/// twin (`processLease`) logs the same field set.
///
/// Not part of the public API — exposed only so the observability tests in
/// `tests/receiver_tests.rs` can assert the pinned event fields directly.
#[doc(hidden)]
pub async fn process_lease(
    handler: Option<BoxedHandler>,
    lease: LeaseInfo,
    target: CommandTarget,
) {
    let LeaseInfo {
        lease_id,
        lease_expires_at,
        command_id,
        attempt,
        envelope,
    } = lease;
    let deadline = envelope.deadline;

    debug!(
        command_id = %command_id,
        command = %envelope.command,
        attempt = attempt,
        "Processing command"
    );

    let response = execute_lease(handler, &envelope, lease_expires_at, attempt).await;
    let handler_status = command_response_status(&response).to_string();

    let submit_status = match submit_response(&envelope, response).await {
        Ok(()) => "submitted",
        Err(e) => {
            error!(
                command_id = %command_id,
                lease_id = %lease_id,
                error = %e,
                "Failed to submit command response"
            );
            "failed"
        }
    };

    info!(
        command_id = %command_id,
        lease_id = %lease_id,
        target_resource_id = %target.resource_id,
        target_resource_type = %target.resource_type.as_str(),
        attempt = attempt,
        deadline = deadline.map(|d| d.to_rfc3339()),
        handler_status = %handler_status,
        submit_status = %submit_status,
        "Command processed"
    );
}

/// Execute a leased command under its budget and produce the response to
/// submit. Never performs response submission itself (unit-testable).
async fn execute_lease(
    handler: Option<BoxedHandler>,
    envelope: &crate::types::Envelope,
    lease_expires_at: DateTime<Utc>,
    attempt: u32,
) -> CommandResponse {
    let Some(handler) = handler else {
        return CommandResponse::error(
            ERROR_CODE_UNKNOWN_COMMAND,
            format!("No handler registered for command '{}'", envelope.command),
        );
    };

    let input = match decode_params_bytes(envelope).await {
        Ok(input) => input,
        Err(e) => return CommandResponse::error(&e.code, e.to_string()),
    };

    let budget = command_budget(envelope.deadline, lease_expires_at);
    let cancellation = CancellationToken::new();
    let ctx = Context {
        input,
        deadline: Some(budget),
        command_id: envelope.command_id.clone(),
        attempt,
        cancellation: cancellation.clone(),
    };

    let remaining = (budget - Utc::now()).to_std().unwrap_or(Duration::ZERO);

    tokio::select! {
        result = handler(ctx) => match result {
            Ok(bytes) => CommandResponse::success(&bytes),
            Err(message) => CommandResponse::error(ERROR_CODE_HANDLER_ERROR, message),
        },
        _ = tokio::time::sleep(remaining) => {
            // Budget expired: the handler future is dropped (aborted) by
            // this select; cancel the token for cooperative work it spawned.
            cancellation.cancel();
            warn!(
                command_id = %envelope.command_id,
                command = %envelope.command,
                budget = %budget,
                "Command exceeded its execution budget, aborting handler"
            );
            CommandResponse::error(
                ERROR_CODE_HANDLER_TIMEOUT,
                format!(
                    "Command '{}' exceeded its execution budget ({budget})",
                    envelope.command
                ),
            )
        }
    }
}

fn require_env<'a>(env: &'a HashMap<String, String>, var: &str) -> Result<&'a String> {
    match env.get(var) {
        Some(value) if !value.is_empty() => Ok(value),
        Some(_) => Err(AlienError::new(ErrorData::CommandReceiverConfigInvalid {
            message: format!("{var} must not be empty"),
            env_var: var.to_string(),
        })),
        None => Err(AlienError::new(ErrorData::CommandReceiverConfigInvalid {
            message: format!("{var} is required"),
            env_var: var.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use alien_core::presigned::{PresignedOperation, PresignedRequest};
    use chrono::Duration as ChronoDuration;

    use super::*;
    use crate::types::{BodySpec, Envelope, ResponseHandling};
    use crate::PROTOCOL_VERSION;

    fn full_env() -> HashMap<String, String> {
        HashMap::from([
            (
                ENV_ALIEN_COMMANDS_URL.to_string(),
                "https://commands.example.com/v1/".to_string(),
            ),
            (ENV_ALIEN_COMMANDS_TOKEN.to_string(), "tok".to_string()),
            (ENV_ALIEN_DEPLOYMENT_ID.to_string(), "dep-123".to_string()),
            (
                ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
                "agent".to_string(),
            ),
            (
                ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE.to_string(),
                "daemon".to_string(),
            ),
        ])
    }

    fn test_envelope(command: &str, deadline: Option<DateTime<Utc>>) -> Envelope {
        Envelope {
            protocol: PROTOCOL_VERSION.to_string(),
            deployment_id: "dep-123".to_string(),
            target: CommandTarget::new("agent", CommandTargetType::Daemon),
            command_id: "cmd_1".to_string(),
            attempt: 1,
            deadline,
            command: command.to_string(),
            params: BodySpec::inline(br#"{"key":"value"}"#),
            response_handling: ResponseHandling {
                max_inline_bytes: crate::INLINE_MAX_BYTES as u64,
                submit_response_url: "https://commands.example.com/v1/commands/cmd_1/response"
                    .to_string(),
                storage_upload_request: PresignedRequest::new_http(
                    "https://storage.example.com/upload".to_string(),
                    "PUT".to_string(),
                    HashMap::new(),
                    PresignedOperation::Put,
                    "test-path".to_string(),
                    Utc::now() + ChronoDuration::hours(1),
                ),
            },
        }
    }

    #[test]
    fn from_env_vars_happy_path() {
        let receiver = Receiver::from_env_vars(&full_env()).expect("valid env");
        assert_eq!(receiver.deployment_id, "dep-123");
        assert_eq!(receiver.target.resource_id, "agent");
        assert_eq!(receiver.target.resource_type, CommandTargetType::Daemon);
        // The `…/commands/leases` endpoint is built once at config time from
        // the base URL (trailing slash collapsed, not doubled).
        assert_eq!(
            receiver.lease_client.endpoint().as_str(),
            "https://commands.example.com/v1/commands/leases"
        );
        assert_eq!(receiver.poll_interval, Duration::from_secs(5));
        assert_eq!(receiver.max_leases, 10);
        assert_eq!(receiver.lease_seconds, 60);
    }

    #[test]
    fn from_env_vars_container_type() {
        let mut env = full_env();
        env.insert(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE.to_string(),
            "container".to_string(),
        );
        let receiver = Receiver::from_env_vars(&env).expect("valid env");
        assert_eq!(receiver.target.resource_type, CommandTargetType::Container);
    }

    #[test]
    fn from_env_vars_missing_each_required_var_names_it() {
        for var in [
            ENV_ALIEN_COMMANDS_URL,
            ENV_ALIEN_COMMANDS_TOKEN,
            ENV_ALIEN_DEPLOYMENT_ID,
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
        ] {
            let mut env = full_env();
            env.remove(var);
            let err = Receiver::from_env_vars(&env)
                .err()
                .unwrap_or_else(|| panic!("missing {var} must fail fast"));
            assert_eq!(err.code, "COMMAND_RECEIVER_CONFIG_INVALID");
            assert!(
                err.message.contains(var),
                "error must name '{var}', got: {}",
                err.message
            );
        }
    }

    #[test]
    fn from_env_vars_empty_value_rejected() {
        let mut env = full_env();
        env.insert(ENV_ALIEN_COMMANDS_URL.to_string(), String::new());
        let err = Receiver::from_env_vars(&env).expect_err("empty URL must fail");
        assert_eq!(err.code, "COMMAND_RECEIVER_CONFIG_INVALID");
        assert!(err.message.contains(ENV_ALIEN_COMMANDS_URL));
    }

    #[test]
    fn from_env_vars_invalid_url_rejected() {
        let mut env = full_env();
        env.insert(ENV_ALIEN_COMMANDS_URL.to_string(), "not a url".to_string());
        let err = Receiver::from_env_vars(&env).expect_err("invalid URL must fail");
        assert_eq!(err.code, "COMMAND_RECEIVER_CONFIG_INVALID");
        assert!(err.message.contains(ENV_ALIEN_COMMANDS_URL));
    }

    #[test]
    fn from_env_vars_rejects_cannot_be_a_base_url() {
        // A URL that parses but cannot be a hierarchical (HTTP(S)) URL — so the
        // `commands/leases` path can never be appended — is a permanent config
        // error. It must fail at construction, not be retried every poll.
        let mut env = full_env();
        env.insert(
            ENV_ALIEN_COMMANDS_URL.to_string(),
            "mailto:commands@example.com".to_string(),
        );
        let err = Receiver::from_env_vars(&env).expect_err("cannot-be-a-base URL must fail");
        assert_eq!(err.code, "COMMAND_RECEIVER_CONFIG_INVALID");
        assert!(err.message.contains(ENV_ALIEN_COMMANDS_URL));
    }

    #[test]
    fn from_env_vars_rejects_worker_target_type() {
        // A receiver is the Container/Daemon path; the worker runtime has its
        // own polling transport. Never guess a Worker target.
        let mut env = full_env();
        env.insert(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE.to_string(),
            "worker".to_string(),
        );
        let err = Receiver::from_env_vars(&env).expect_err("worker type must fail");
        assert_eq!(err.code, "COMMAND_RECEIVER_CONFIG_INVALID");
        assert!(err.message.contains("container"));
        assert!(err.message.contains("daemon"));
    }

    #[test]
    fn from_env_missing_everything_names_url_first() {
        // Process env without any ALIEN_COMMANDS_URL: from_env must fail with
        // the receiver config code naming the pinned variable.
        temp_env::with_var(ENV_ALIEN_COMMANDS_URL, None::<&str>, || {
            let err = Receiver::from_env().expect_err("missing env must fail");
            assert_eq!(err.code, "COMMAND_RECEIVER_CONFIG_INVALID");
            assert!(err.message.contains(ENV_ALIEN_COMMANDS_URL));
        });
    }

    #[test]
    fn lease_request_carries_typed_target_and_defaults() {
        let receiver = Receiver::from_env_vars(&full_env()).expect("valid env");
        let request = receiver.build_lease_request();
        assert_eq!(request.deployment_id, "dep-123");
        assert_eq!(request.target.resource_id, "agent");
        assert_eq!(request.target.resource_type, CommandTargetType::Daemon);
        assert_eq!(request.max_leases, DEFAULT_MAX_LEASES);
        assert_eq!(request.lease_seconds, DEFAULT_LEASE_SECONDS);
    }

    #[test]
    fn budget_is_min_of_deadline_and_safety_margined_lease_expiry() {
        let lease_expiry = Utc::now() + ChronoDuration::seconds(60);
        // The lease bound is the expiry minus the 5s safety margin, not the
        // raw expiry: a command must finish before the lease is really gone.
        let margined = lease_expiry - ChronoDuration::seconds(5);
        let early_deadline = Utc::now() + ChronoDuration::seconds(10);
        let late_deadline = Utc::now() + ChronoDuration::seconds(120);

        // No deadline: budget is the safety-margined lease expiry, never the
        // raw expiry.
        assert_eq!(command_budget(None, lease_expiry), margined);
        // Deadline earlier than the margined lease bound wins.
        assert_eq!(
            command_budget(Some(early_deadline), lease_expiry),
            early_deadline
        );
        // Deadline later than the margined lease bound is clamped to it (the
        // raw expiry would leak past the safety margin).
        assert_eq!(command_budget(Some(late_deadline), lease_expiry), margined);
    }

    #[test]
    fn budget_clamps_to_now_when_lease_already_within_margin() {
        // A lease whose remaining time is already inside the safety margin
        // yields a budget clamped to now (never a time in the past), so the
        // handler is given zero budget rather than a negative one.
        let before = Utc::now();
        let nearly_expired = before + ChronoDuration::seconds(2);
        let budget = command_budget(None, nearly_expired);
        let after = Utc::now();
        assert!(
            budget >= before && budget <= after,
            "budget must clamp to now, got {budget} (window {before}..{after})"
        );
    }

    #[test]
    fn context_input_json_parses_and_rejects() {
        let ctx = Context {
            input: br#"{"key":"value"}"#.to_vec(),
            deadline: None,
            command_id: "cmd_1".to_string(),
            attempt: 1,
            cancellation: CancellationToken::new(),
        };
        let parsed: serde_json::Value = ctx.input_json().expect("valid JSON input");
        assert_eq!(parsed["key"], "value");

        let bad = Context {
            input: b"not-json".to_vec(),
            ..ctx
        };
        let err = bad
            .input_json::<serde_json::Value>()
            .expect_err("invalid JSON must fail");
        assert_eq!(err.code, "SERIALIZATION_FAILED");
    }

    #[test]
    fn command_response_status_labels_success_and_error_codes() {
        assert_eq!(
            command_response_status(&CommandResponse::success(b"{}")),
            "success"
        );
        assert_eq!(
            command_response_status(&CommandResponse::error(ERROR_CODE_UNKNOWN_COMMAND, "x")),
            ERROR_CODE_UNKNOWN_COMMAND
        );
        assert_eq!(
            command_response_status(&CommandResponse::error(ERROR_CODE_HANDLER_ERROR, "boom")),
            ERROR_CODE_HANDLER_ERROR
        );
        assert_eq!(
            command_response_status(&CommandResponse::error(ERROR_CODE_HANDLER_TIMEOUT, "late")),
            ERROR_CODE_HANDLER_TIMEOUT
        );
    }

    #[tokio::test]
    async fn execute_lease_success_serializes_handler_return() {
        let handler = box_handler(|ctx: Context| async move {
            let input: serde_json::Value = ctx.input_json().map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "echo": input["key"], "attempt": ctx.attempt }))
        });

        let envelope = test_envelope("echo", None);
        let response = execute_lease(
            Some(handler),
            &envelope,
            Utc::now() + ChronoDuration::seconds(60),
            3,
        )
        .await;

        let CommandResponse::Success { response: body } = response else {
            panic!("expected success response");
        };
        let bytes = body.decode_inline().expect("inline body");
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json body");
        assert_eq!(json["echo"], "value");
        assert_eq!(json["attempt"], 3);
    }

    #[tokio::test]
    async fn execute_lease_unknown_command() {
        let envelope = test_envelope("nobody-home", None);
        let response =
            execute_lease(None, &envelope, Utc::now() + ChronoDuration::seconds(60), 1).await;

        let CommandResponse::Error { code, message, .. } = response else {
            panic!("expected error response");
        };
        assert_eq!(code, ERROR_CODE_UNKNOWN_COMMAND);
        assert!(message.contains("nobody-home"));
    }

    #[tokio::test]
    async fn execute_lease_handler_error_becomes_handler_error_response() {
        let handler = box_handler(|_ctx: Context| async move {
            Err::<serde_json::Value, HandlerError>("database on fire".into())
        });

        let envelope = test_envelope("burn", None);
        let response = execute_lease(
            Some(handler),
            &envelope,
            Utc::now() + ChronoDuration::seconds(60),
            1,
        )
        .await;

        let CommandResponse::Error { code, message, .. } = response else {
            panic!("expected error response");
        };
        assert_eq!(code, ERROR_CODE_HANDLER_ERROR);
        assert!(message.contains("database on fire"));
    }

    #[tokio::test(start_paused = true)]
    async fn execute_lease_budget_expiry_aborts_handler_and_reports_timeout() {
        static COMPLETED: AtomicBool = AtomicBool::new(false);
        COMPLETED.store(false, Ordering::SeqCst);

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<CancellationToken>();
        let cancel_tx = std::sync::Mutex::new(Some(cancel_tx));
        let handler = box_handler(move |ctx: Context| {
            if let Some(tx) = cancel_tx.lock().expect("lock").take() {
                let _ = tx.send(ctx.cancellation.clone());
            }
            async move {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                COMPLETED.store(true, Ordering::SeqCst);
                Ok(serde_json::json!({ "done": true }))
            }
        });

        // Budget from the envelope deadline: 2s, well below the lease expiry.
        let envelope = test_envelope("slow", Some(Utc::now() + ChronoDuration::seconds(2)));
        let response = execute_lease(
            Some(handler),
            &envelope,
            Utc::now() + ChronoDuration::seconds(60),
            1,
        )
        .await;

        let CommandResponse::Error { code, message, .. } = response else {
            panic!("expected error response");
        };
        assert_eq!(code, ERROR_CODE_HANDLER_TIMEOUT);
        assert!(message.contains("slow"));
        assert!(
            !COMPLETED.load(Ordering::SeqCst),
            "handler future must be aborted at budget expiry"
        );
        let token = cancel_rx.await.expect("handler ran");
        assert!(
            token.is_cancelled(),
            "ctx.cancellation must fire at budget expiry"
        );
    }
}
